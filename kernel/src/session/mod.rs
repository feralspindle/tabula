//! active session management: one actor task per live session.
//!
//! the actor owns the session's `World`, plugin instance, schema registry, and
//! grants, and is the session's ONLY writer. serializing every command through
//! its mailbox is what makes seq gapless and the record broadcast total-ordered
//! - no locks, no interleaving.

mod actor;
pub mod protocol;
pub mod ws;

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use uuid::Uuid;

use tabula_core::{LogRecord, World};

use crate::error::AppError;
use crate::runtime::CommandInput;
use crate::state::AppState;

pub use actor::SessionRow;

/// sent to the session actor by WS connections.
pub enum SessionMsg {
    Command {
        input: CommandInput,
        reply: oneshot::Sender<Result<LogRecord, AppError>>,
    },
    /// current projection + seq, for the join-time snapshot frame.
    Snapshot {
        reply: oneshot::Sender<(World, u64)>,
    },
    /// records with seq > after_seq, for reconnect tail replay.
    Tail {
        after_seq: u64,
        reply: oneshot::Sender<Result<Vec<LogRecord>, AppError>>,
    },
}

#[derive(Clone)]
pub struct SessionHandle {
    pub tx: mpsc::Sender<SessionMsg>,
    pub broadcast: broadcast::Sender<protocol::ServerFrame>,
}

impl SessionHandle {
    pub async fn command(&self, input: CommandInput) -> Result<LogRecord, AppError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(SessionMsg::Command { input, reply })
            .await
            .map_err(|_| AppError::NotFound)?;
        rx.await
            .map_err(|_| AppError::Plugin("session actor died".into()))?
    }

    pub async fn snapshot(&self) -> Result<(World, u64), AppError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(SessionMsg::Snapshot { reply })
            .await
            .map_err(|_| AppError::NotFound)?;
        rx.await
            .map_err(|_| AppError::Plugin("session actor died".into()))
    }

    pub async fn tail(&self, after_seq: u64) -> Result<Vec<LogRecord>, AppError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(SessionMsg::Tail { after_seq, reply })
            .await
            .map_err(|_| AppError::NotFound)?;
        rx.await
            .map_err(|_| AppError::Plugin("session actor died".into()))?
    }
}

/// registry of live session actors. actors idle out when every handle drops
/// and the mailbox closes.
#[derive(Default)]
pub struct SessionRegistry {
    live: Mutex<HashMap<Uuid, SessionHandle>>,
}

impl SessionRegistry {
    /// returns the live handle for a session, booting an actor (cold load:
    /// snapshot + tail fold, plugin instantiation) if none is running.
    pub async fn get_or_spawn(
        &self,
        state: &AppState,
        session_id: Uuid,
    ) -> Result<SessionHandle, AppError> {
        let mut live = self.live.lock().await;
        if let Some(handle) = live.get(&session_id) {
            // A previous actor may have shut down; a closed mailbox means we
            // must boot a fresh one.
            if !handle.tx.is_closed() {
                return Ok(handle.clone());
            }
            live.remove(&session_id);
        }

        let handle = actor::spawn(state.clone(), session_id).await?;
        live.insert(session_id, handle.clone());
        Ok(handle)
    }
}

/// fetches the sessions row backing an actor boot or an authz decision.
pub async fn load_session_row(
    pool: &sqlx::PgPool,
    session_id: Uuid,
) -> Result<SessionRow, AppError> {
    let row: Option<SessionRow> = sqlx::query_as(
        "select id, owner_id, name, system_plugin_id, system_plugin_version from sessions where id = $1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    row.ok_or(AppError::NotFound)
}

pub type SharedRegistry = Arc<SessionRegistry>;
