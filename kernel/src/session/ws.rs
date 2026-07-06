//! WebSocket endpoint: `GET /api/sessions/{id}/ws?token=…[&after_seq=N]`.
//!
//! browsers cannot set Authorization headers on WebSocket upgrades, so the
//! supabase JWT rides in the `token` query parameter and is verified before
//! upgrading. on connect: subscribe to the session broadcast, then send either
//! a `snapshot` frame (fresh join) or the record tail (reconnect with
//! `after_seq`), then forward broadcast records, subscription-before-snapshot
//! plus client-side seq dedup makes the handoff gapless.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use uuid::Uuid;

use super::protocol::{ClientFrame, ServerFrame};
use super::SessionHandle;
use crate::auth::jwt;
use crate::authz;
use crate::error::AppError;
use crate::runtime::CommandInput;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct WsParams {
    token: String,
    /// last seq the client has folded; present on reconnect.
    #[serde(default)]
    after_seq: Option<u64>,
}

pub async fn ws_handler(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Query(params): Query<WsParams>,
    upgrade: WebSocketUpgrade,
) -> Result<impl IntoResponse, AppError> {
    let claims = jwt::verify(&params.token, &state.jwks()).map_err(|e| {
        tracing::warn!("ws jwt verification failed: {e}");
        AppError::Unauthorized
    })?;
    let user_id = claims.sub;

    if state.rate_limiter().check_key(&user_id).is_err() {
        return Err(AppError::TooManyRequests);
    }

    if !authz::is_session_member(state.pool(), user_id, session_id).await? {
        return Err(AppError::Forbidden);
    }
    let is_gm = authz::is_session_gm(state.pool(), user_id, session_id).await?;

    let handle = state.sessions().get_or_spawn(&state, session_id).await?;

    Ok(upgrade.on_upgrade(move |socket| {
        connection(socket, state, handle, user_id, is_gm, params.after_seq)
    }))
}

async fn connection(
    socket: WebSocket,
    state: AppState,
    handle: SessionHandle,
    user_id: Uuid,
    is_gm: bool,
    after_seq: Option<u64>,
) {
    metrics::gauge!("ws_connections").increment(1.0);
    let (mut sink, mut stream) = socket.split();

    // subscribe BEFORE snapshotting so no record can fall between them.
    let mut records = handle.broadcast.subscribe();

    // join: full snapshot. reconnect: tail replay from the client's seq.
    let catch_up = match after_seq {
        None => match handle.snapshot().await {
            Ok((world, seq)) => vec![ServerFrame::Snapshot { seq, world }],
            Err(e) => {
                let _ = send(&mut sink, &error_frame(None, &e)).await;
                return;
            }
        },
        Some(seq) => match handle.tail(seq).await {
            Ok(tail) => tail
                .into_iter()
                .map(|record| ServerFrame::Record { record })
                .collect(),
            Err(e) => {
                let _ = send(&mut sink, &error_frame(None, &e)).await;
                return;
            }
        },
    };
    for frame in &catch_up {
        if send(&mut sink, frame).await.is_err() {
            metrics::gauge!("ws_connections").decrement(1.0);
            return;
        }
    }

    // two-way pump: client commands in, broadcast records out.
    loop {
        tokio::select! {
            broadcasted = records.recv() => {
                match broadcasted {
                    Ok(frame) => {
                        if send(&mut sink, &frame).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        // slow consumer lost records; force a resync.
                        tracing::warn!(%user_id, lagged = n, "ws consumer lagged, resyncing");
                        match handle.snapshot().await {
                            Ok((world, seq)) => {
                                if send(&mut sink, &ServerFrame::Snapshot { seq, world }).await.is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = stream.next() => {
                let Some(Ok(msg)) = incoming else { break };
                let Message::Text(text) = msg else { continue };

                // per-user rate limit applies to WS commands like HTTP requests.
                if state.rate_limiter().check_key(&user_id).is_err() {
                    let _ = send(&mut sink, &error_frame(None, &AppError::TooManyRequests)).await;
                    continue;
                }

                let frame: ClientFrame = match serde_json::from_str(&text) {
                    Ok(frame) => frame,
                    Err(e) => {
                        let _ = send(&mut sink, &ServerFrame::Error {
                            command_id: None,
                            message: format!("malformed frame: {e}"),
                        }).await;
                        continue;
                    }
                };

                let ClientFrame::Command { id, name, payload } = frame;
                let input = CommandInput {
                    id,
                    name,
                    actor: user_id,
                    actor_is_gm: is_gm,
                    payload,
                };
                // success arrives via the broadcast subscription; only errors
                // are answered directly (to this client alone).
                if let Err(e) = handle.command(input).await {
                    if send(&mut sink, &error_frame(Some(id), &e)).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
    metrics::gauge!("ws_connections").decrement(1.0);
}

fn error_frame(command_id: Option<Uuid>, e: &AppError) -> ServerFrame {
    ServerFrame::Error {
        command_id,
        message: e.to_string(),
    }
}

async fn send(
    sink: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    frame: &ServerFrame,
) -> Result<(), axum::Error> {
    let text = serde_json::to_string(frame).expect("server frame serializes");
    sink.send(Message::Text(text.into())).await
}
