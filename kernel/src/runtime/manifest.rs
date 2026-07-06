//! parsed, validated plugin manifests.
//!
//! the raw WIT manifest carries schemas and layouts as JSON text; this module
//! parses them once at load, derives the plugin's write grants from its declared
//! component namespaces, and pre-registers schemas for the session validator.

use serde_json::Value;

use tabula_core::{ComponentKey, Grants, SchemaRegistry};

use super::wit_types;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginType {
    System,
    Module,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ComponentDef {
    pub key: ComponentKey,
    pub version: u32,
    pub schema: Value,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandDecl {
    pub name: String,
    pub params_schema: Value,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ParsedManifest {
    pub id: String,
    pub version: String,
    pub plugin_type: PluginType,
    pub components: Vec<ComponentDef>,
    pub commands: Vec<CommandDecl>,
    pub sheet_layout: Value,
}

impl ParsedManifest {
    pub fn parse(raw: wit_types::PluginManifest) -> Result<Self, AppError> {
        let bad = |what: &str, e: &dyn std::fmt::Display| {
            AppError::Plugin(format!("manifest {what}: {e}"))
        };

        if raw.id.is_empty()
            || !raw
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(AppError::Plugin(format!(
                "manifest id {:?} must be non-empty lowercase [a-z0-9-]",
                raw.id
            )));
        }

        let mut components = Vec::with_capacity(raw.components.len());
        for c in raw.components {
            let key = ComponentKey::new(&c.key).map_err(|e| bad("component key", &e))?;
            // plugins declare their own namespaces; `core.*` is host-owned and
            // never plugin-declared (invariant #7).
            if key.namespace() == "core" {
                return Err(AppError::Plugin(format!(
                    "manifest declares host-reserved component {key}"
                )));
            }
            let schema: Value =
                serde_json::from_str(&c.schema).map_err(|e| bad("component schema", &e))?;
            components.push(ComponentDef {
                key,
                version: c.version,
                schema,
            });
        }

        let mut commands = Vec::with_capacity(raw.commands.len());
        for cmd in raw.commands {
            let params_schema: Value =
                serde_json::from_str(&cmd.params_schema).map_err(|e| bad("params schema", &e))?;
            commands.push(CommandDecl {
                name: cmd.name,
                params_schema,
            });
        }

        let sheet_layout: Value =
            serde_json::from_str(&raw.sheet_layout).map_err(|e| bad("sheet layout", &e))?;

        Ok(Self {
            id: raw.id,
            version: raw.version,
            plugin_type: match raw.plugin_type {
                wit_types::PluginType::System => PluginType::System,
                wit_types::PluginType::Module => PluginType::Module,
            },
            components,
            commands,
            sheet_layout,
        })
    }

    pub fn command(&self, name: &str) -> Option<&CommandDecl> {
        self.commands.iter().find(|c| c.name == name)
    }

    /// write grants: every namespace this manifest declares components in,
    /// plus the `core.*` keys the host grants all System plugins (`core.name`).
    pub fn grants(&self) -> Grants {
        let mut grants = Grants::new();
        for c in &self.components {
            grants.allow_namespace(c.key.namespace());
        }
        grants.allow_core_key(ComponentKey::new("core.name").expect("static key"));
        grants
    }

    /// registers all declared component schemas, plus the host-owned `core.*`
    /// schemas, into a fresh session registry.
    pub fn build_registry(&self) -> Result<SchemaRegistry, AppError> {
        let mut registry = SchemaRegistry::new();
        // host-owned core components (spec §10: `core.name` at minimum).
        registry
            .register(
                ComponentKey::new("core.name").expect("static key"),
                1,
                &serde_json::json!({ "type": "string", "minLength": 1, "maxLength": 120 }),
            )
            .map_err(|e| AppError::Plugin(e.to_string()))?;

        for c in &self.components {
            registry
                .register(c.key.clone(), c.version, &c.schema)
                .map_err(|e| AppError::Plugin(format!("register {}: {e}", c.key)))?;
        }
        Ok(registry)
    }
}
