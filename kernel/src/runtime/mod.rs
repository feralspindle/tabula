//! the wasmtime plugin host.
//!
//! loads WASM Component Model plugins against the WIT contract in
//! `wit/tabula.wit`, exposes the host imports (dice, RNG, clock, bounded world
//! queries, diagnostics), and turns `decide` results back into `tabula_core`
//! deltas. everything a plugin returns is re-validated by the capability
//! validator before it can reach the log, nothing here grants authority.

mod host;
mod manifest;

pub use host::HostCtx;
pub use manifest::{CommandDecl, ComponentDef, ParsedManifest, PluginType};

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};

use tabula_core::{Delta, World};

use crate::error::AppError;

wasmtime::component::bindgen!({
    path: "../wit",
    world: "system-plugin",
});

use tabula::plugin::types as wit_types;

/// fuel budget per plugin call. generous for real rules logic, fatal for a
/// runaway loop.
const FUEL_PER_CALL: u64 = 1_000_000_000;

/// compiled plugin, shared across sessions (instantiation is per-session).
pub struct LoadedPlugin {
    pub manifest: ParsedManifest,
    component: Component,
}

/// engine + all plugins found in the plugin directory at boot, keyed by
/// manifest id.
pub struct PluginRuntime {
    engine: Engine,
    plugins: HashMap<String, Arc<LoadedPlugin>>,
}

impl PluginRuntime {
    pub fn new() -> Result<Self, AppError> {
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config).map_err(|e| AppError::Plugin(e.to_string()))?;
        Ok(Self {
            engine,
            plugins: HashMap::new(),
        })
    }

    /// loads every `*.wasm` component in `dir`, reading each manifest once
    /// against an empty throwaway world.
    pub fn load_dir(&mut self, dir: &Path) -> Result<(), AppError> {
        let entries = std::fs::read_dir(dir)
            .map_err(|e| AppError::Plugin(format!("plugin dir {}: {e}", dir.display())))?;
        for entry in entries {
            let path = entry.map_err(|e| AppError::Plugin(e.to_string()))?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
                self.load_file(&path)?;
            }
        }
        Ok(())
    }

    pub fn load_file(&mut self, path: &Path) -> Result<&ParsedManifest, AppError> {
        let component = Component::from_file(&self.engine, path)
            .map_err(|e| AppError::Plugin(format!("load {}: {e}", path.display())))?;

        // read the manifest with a throwaway instance over an empty world.
        let world = Arc::new(Mutex::new(World::new()));
        let mut instance = PluginInstance::instantiate(&self.engine, &component, world)?;
        let manifest = instance.manifest()?;

        tracing::info!(
            plugin = %manifest.id,
            version = %manifest.version,
            components = manifest.components.len(),
            commands = manifest.commands.len(),
            "loaded plugin"
        );
        let id = manifest.id.clone();
        self.plugins.insert(
            id.clone(),
            Arc::new(LoadedPlugin {
                manifest,
                component,
            }),
        );
        Ok(&self.plugins[&id].manifest)
    }

    pub fn get(&self, id: &str) -> Option<Arc<LoadedPlugin>> {
        self.plugins.get(id).cloned()
    }

    pub fn plugins(&self) -> impl Iterator<Item = &Arc<LoadedPlugin>> {
        self.plugins.values()
    }

    /// instantiates a plugin for one session, bound to that session's world.
    pub fn instantiate(
        &self,
        plugin: &LoadedPlugin,
        world: Arc<Mutex<World>>,
    ) -> Result<PluginInstance, AppError> {
        PluginInstance::instantiate(&self.engine, &plugin.component, world)
    }
}

/// A live per-session plugin instance. NOT Sync, owned by one session actor.
pub struct PluginInstance {
    store: Store<HostCtx>,
    bindings: SystemPlugin,
}

/// A command as the session actor hands it to `decide`.
pub struct CommandInput {
    pub id: uuid::Uuid,
    pub name: String,
    pub actor: uuid::Uuid,
    pub actor_is_gm: bool,
    pub payload: serde_json::Value,
}

impl PluginInstance {
    fn instantiate(
        engine: &Engine,
        component: &Component,
        world: Arc<Mutex<World>>,
    ) -> Result<Self, AppError> {
        let mut linker: Linker<HostCtx> = Linker::new(engine);
        SystemPlugin::add_to_linker(&mut linker, |ctx| ctx)
            .map_err(|e| AppError::Plugin(format!("link: {e}")))?;

        let mut store = Store::new(engine, HostCtx::new(world));
        store
            .set_fuel(FUEL_PER_CALL)
            .map_err(|e| AppError::Plugin(e.to_string()))?;

        let bindings = SystemPlugin::instantiate(&mut store, component, &linker)
            .map_err(|e| AppError::Plugin(format!("instantiate: {e}")))?;

        Ok(Self { store, bindings })
    }

    fn refuel(&mut self) -> Result<(), AppError> {
        self.store
            .set_fuel(FUEL_PER_CALL)
            .map_err(|e| AppError::Plugin(e.to_string()))
    }

    pub fn manifest(&mut self) -> Result<ParsedManifest, AppError> {
        self.refuel()?;
        let raw = self
            .bindings
            .tabula_plugin_guest()
            .call_manifest(&mut self.store)
            .map_err(|e| AppError::Plugin(format!("manifest trapped: {e}")))?;
        ParsedManifest::parse(raw)
    }

    /// the single rules entry point. `Err(Rule)` goes back to the issuing
    /// client only; `Err(Plugin)` is a plugin bug (trap, bad delta encoding).
    pub fn decide(
        &mut self,
        cmd: &CommandInput,
        context: &serde_json::Value,
    ) -> Result<Vec<Delta>, AppError> {
        self.refuel()?;
        let wit_cmd = wit_types::Command {
            id: cmd.id.to_string(),
            name: cmd.name.clone(),
            actor: cmd.actor.to_string(),
            actor_is_gm: cmd.actor_is_gm,
            payload: cmd.payload.to_string(),
        };

        let result = self
            .bindings
            .tabula_plugin_guest()
            .call_decide(&mut self.store, &wit_cmd, &context.to_string())
            .map_err(|e| AppError::Plugin(format!("decide trapped: {e}")))?;

        match result {
            Ok(deltas) => deltas.into_iter().map(parse_delta).collect(),
            Err(rule) => Err(AppError::Rule(rule.message)),
        }
    }

    /// schema evolution hook, run against component values at snapshot load.
    pub fn migrate(
        &mut self,
        key: &str,
        old_version: u32,
        value: &serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        self.refuel()?;
        let migrated = self
            .bindings
            .tabula_plugin_guest()
            .call_migrate(&mut self.store, key, old_version, &value.to_string())
            .map_err(|e| AppError::Plugin(format!("migrate trapped: {e}")))?;
        serde_json::from_str(&migrated)
            .map_err(|e| AppError::Plugin(format!("migrate returned invalid JSON: {e}")))
    }
}

fn parse_delta(d: wit_types::Delta) -> Result<Delta, AppError> {
    use tabula_core::{ComponentKey, EntityId};

    let entity = |s: &str| -> Result<EntityId, AppError> {
        s.parse()
            .map_err(|_| AppError::Plugin(format!("plugin returned invalid entity id {s:?}")))
    };
    let key = |s: &str| -> Result<ComponentKey, AppError> {
        ComponentKey::new(s)
            .map_err(|e| AppError::Plugin(format!("plugin returned invalid component key: {e}")))
    };

    Ok(match d {
        wit_types::Delta::Spawn(e) => Delta::Spawn {
            entity: entity(&e)?,
        },
        wit_types::Delta::Despawn(e) => Delta::Despawn {
            entity: entity(&e)?,
        },
        wit_types::Delta::Set(op) => Delta::Set {
            entity: entity(&op.entity)?,
            component: key(&op.component)?,
            value: serde_json::from_str(&op.value).map_err(|e| {
                AppError::Plugin(format!("plugin returned invalid JSON value: {e}"))
            })?,
        },
        wit_types::Delta::Remove(op) => Delta::Remove {
            entity: entity(&op.entity)?,
            component: key(&op.component)?,
        },
    })
}
