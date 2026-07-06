//! the per-session actor: the one loop everything goes through (spec §4.3).
//!
//! command → decide → capability validation → atomic append → fold → broadcast.
//! rule/validation errors reply to the issuing client only; nothing is logged.

use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

use tabula_core::{
    validate_deltas, Actor, Cause, GameTime, Grants, PluginRef, SchemaRegistry, World,
};

use super::protocol::ServerFrame;
use super::{SessionHandle, SessionMsg};
use crate::error::AppError;
use crate::runtime::{CommandInput, PluginInstance};
use crate::state::AppState;
use crate::store;

#[derive(sqlx::FromRow)]
pub struct SessionRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    pub system_plugin_id: String,
    pub system_plugin_version: String,
}

const MAILBOX: usize = 64;
const BROADCAST_BUFFER: usize = 256;

/// boots the actor for one session: loads the row, the plugin, and the cold
/// projection (snapshot + tail, a pure fold, never plugin code), then serves
/// its mailbox until every handle drops.
pub async fn spawn(state: AppState, session_id: Uuid) -> Result<SessionHandle, AppError> {
    let row = super::load_session_row(state.pool(), session_id).await?;

    let plugin = state
        .runtime()
        .get(&row.system_plugin_id)
        .ok_or_else(|| AppError::Plugin(format!("plugin {} not loaded", row.system_plugin_id)))?;

    let registry = plugin.manifest.build_registry()?;
    let grants = plugin.manifest.grants();
    let plugin_ref = PluginRef {
        id: plugin.manifest.id.clone(),
        version: plugin.manifest.version.clone(),
    };

    let (world, last_seq) = store::load_world(state.pool(), session_id).await?;
    // TODO(M6): run plugin `migrate` over snapshot-loaded component values when
    // a registered schema version is newer than the snapshot's plugin version.
    // with a single plugin build per session lifetime this is a no-op today.
    let world = Arc::new(StdMutex::new(world));

    let instance = state.runtime().instantiate(&plugin, world.clone())?;

    let (tx, rx) = mpsc::channel(MAILBOX);
    let (broadcast_tx, _) = broadcast::channel(BROADCAST_BUFFER);

    let actor = SessionActor {
        state: state.clone(),
        session_id,
        plugin_ref,
        registry,
        grants,
        world,
        instance,
        last_seq,
        broadcast: broadcast_tx.clone(),
    };

    tokio::spawn(actor.run(rx));

    Ok(SessionHandle {
        tx,
        broadcast: broadcast_tx,
    })
}

struct SessionActor {
    state: AppState,
    session_id: Uuid,
    plugin_ref: PluginRef,
    registry: SchemaRegistry,
    grants: Grants,
    world: Arc<StdMutex<World>>,
    instance: PluginInstance,
    last_seq: u64,
    broadcast: broadcast::Sender<ServerFrame>,
}

impl SessionActor {
    async fn run(mut self, mut rx: mpsc::Receiver<SessionMsg>) {
        tracing::info!(session = %self.session_id, seq = self.last_seq, "session actor up");
        while let Some(msg) = rx.recv().await {
            match msg {
                SessionMsg::Command { input, reply } => {
                    let result = self.handle_command(input).await;
                    let _ = reply.send(result);
                }
                SessionMsg::Snapshot { reply } => {
                    let world = self.world.lock().expect("world lock").clone();
                    let _ = reply.send((world, self.last_seq));
                }
                SessionMsg::Tail { after_seq, reply } => {
                    let result =
                        store::load_records_after(self.state.pool(), self.session_id, after_seq)
                            .await;
                    let _ = reply.send(result);
                }
            }
        }
        tracing::info!(session = %self.session_id, "session actor idle, shutting down");
    }

    /// the one loop: decide → validate → append → fold → broadcast.
    async fn handle_command(
        &mut self,
        input: CommandInput,
    ) -> Result<tabula_core::LogRecord, AppError> {
        // the plugin must have declared the command.
        if self
            .state
            .runtime()
            .get(&self.plugin_ref.id)
            .and_then(|p| p.manifest.command(&input.name).map(|_| ()))
            .is_none()
        {
            return Err(AppError::BadRequest(format!(
                "unknown command {:?}",
                input.name
            )));
        }

        // context is plugin-pulled via bounded query imports (spec §10), so the
        // pushed context stays minimal.
        let context = serde_json::json!({ "session": self.session_id });

        // decide() reads the projection through host imports; the actor holds
        // no lock across this call.
        let deltas = self.instance.decide(&input, &context)?;

        // nothing to record is a successful no-op, not a log entry.
        if deltas.is_empty() {
            return Err(AppError::Rule("command produced no changes".into()));
        }

        // capability validator: schema validity, namespace grants, spawn/exists
        // consistency. atomic, one bad delta rejects the whole batch.
        {
            let world = self.world.lock().expect("world lock");
            validate_deltas(&world, &self.registry, &self.grants, &deltas)
                .map_err(|e| AppError::Rule(e.to_string()))?;
        }

        let actor = if input.actor_is_gm {
            Actor::GmOverride { id: input.actor }
        } else {
            Actor::User { id: input.actor }
        };
        let cause = Cause {
            command_id: input.id,
            command: Some(input.name.clone()),
            plugin: Some(self.plugin_ref.clone()),
            actor,
        };
        let at = GameTime(chrono::Utc::now().timestamp_millis() as u64);

        // atomic append; the (session_id, seq) PK guarantees gaplessness.
        let record =
            store::append_record(self.state.pool(), self.session_id, at, &cause, &deltas).await?;
        self.last_seq = record.seq;

        // fold into the live projection, the same pure apply replay uses.
        {
            let mut world = self.world.lock().expect("world lock");
            tabula_core::apply_record(&mut world, &record);
        }

        if record.seq % self.state.snapshot_every() == 0 {
            let world = self.world.lock().expect("world lock").clone();
            if let Err(e) =
                store::save_snapshot(self.state.pool(), self.session_id, record.seq, &world).await
            {
                tracing::error!(session = %self.session_id, "snapshot failed: {e}");
            }
        }

        metrics::counter!("session_records_total").increment(1);
        let _ = self.broadcast.send(ServerFrame::Record {
            record: record.clone(),
        });
        Ok(record)
    }
}
