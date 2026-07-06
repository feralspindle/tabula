//! host-import implementations, the complete capability surface of a plugin.
//!
//! all nondeterminism a plugin can observe flows through here (invariant #4):
//! dice, RNG, the session clock, and reads of the session projection.

use std::sync::{Arc, Mutex};

use tabula_core::{ComponentKey, EntityId, World};
use ttrpg_dice_engine::engine::LiveRng;

use super::tabula::plugin::host::Host;
use super::tabula::plugin::types::{GameTime, LogLevel, RollResult};

pub struct HostCtx {
    /// the owning session's live projection. the session actor never holds the
    /// lock across a plugin call, so host imports can always take it.
    world: Arc<Mutex<World>>,
}

impl HostCtx {
    pub fn new(world: Arc<Mutex<World>>) -> Self {
        Self { world }
    }

    fn world(&self) -> std::sync::MutexGuard<'_, World> {
        self.world.lock().expect("world lock poisoned")
    }
}

// the types-only interface generates an empty Host trait that still must be
// implemented for the linker.
impl super::tabula::plugin::types::Host for HostCtx {}

impl Host for HostCtx {
    fn roll(&mut self, expr: String) -> Result<RollResult, String> {
        let mut rng = LiveRng::new();
        let result = ttrpg_dice_engine::roll(&expr, &mut rng).map_err(|e| e.to_string())?;
        let detail = serde_json::to_string(&result).map_err(|e| e.to_string())?;
        Ok(RollResult {
            expr,
            total: result.total,
            detail,
        })
    }

    fn random(&mut self, min: u64, max: u64) -> u64 {
        let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
        rand::Rng::gen_range(&mut rand::thread_rng(), lo..=hi)
    }

    fn now(&mut self) -> GameTime {
        GameTime {
            unix_millis: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    fn get_component(&mut self, entity: String, key: String) -> Option<String> {
        let entity: EntityId = entity.parse().ok()?;
        let key = ComponentKey::new(key).ok()?;
        self.world().get(entity, &key).map(|v| v.to_string())
    }

    fn entities_with(&mut self, key: String) -> Vec<String> {
        let Ok(key) = ComponentKey::new(key) else {
            return Vec::new();
        };
        self.world()
            .entities_with(&key)
            .into_iter()
            .map(|e| e.to_string())
            .collect()
    }

    fn new_entity_id(&mut self) -> String {
        EntityId::new().to_string()
    }

    fn log(&mut self, level: LogLevel, msg: String) {
        match level {
            LogLevel::Debug => tracing::debug!(target: "plugin", "{msg}"),
            LogLevel::Info => tracing::info!(target: "plugin", "{msg}"),
            LogLevel::Warn => tracing::warn!(target: "plugin", "{msg}"),
            LogLevel::Error => tracing::error!(target: "plugin", "{msg}"),
        }
    }
}
