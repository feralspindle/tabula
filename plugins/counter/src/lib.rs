//! counter, the trivial test System plugin (spec §9 M2).
//!
//! declares one component (`counter.value`) and three commands that between
//! them exercise every delta variant and every interesting host import:
//! - `create-counter` → new-entity-id, Spawn + Set (+ granted core.name)
//! - `increment {entity, by}` → get-component, Set
//! - `roll-add {entity, expr}` → host roll(), Set
//! - `delete-counter {entity}` → Despawn

use serde_json::{json, Value};

wit_bindgen::generate!({
    path: "../../wit",
    world: "system-plugin",
});

use exports::tabula::plugin::guest::Guest;
use tabula::plugin::host;
use tabula::plugin::types::{
    Command, CommandDecl, ComponentSchema, Delta, PluginManifest, PluginType, RemoveOp, RuleError,
    SetOp,
};

struct Counter;

const VALUE_KEY: &str = "counter.value";

fn rule_err(message: impl Into<String>) -> RuleError {
    RuleError {
        message: message.into(),
    }
}

fn payload(cmd: &Command) -> Result<Value, RuleError> {
    serde_json::from_str(&cmd.payload).map_err(|e| rule_err(format!("invalid payload: {e}")))
}

fn entity_arg(p: &Value) -> Result<String, RuleError> {
    p.get("entity")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| rule_err("missing entity"))
}

fn current_value(entity: &str) -> Result<i64, RuleError> {
    let raw = host::get_component(entity, VALUE_KEY)
        .ok_or_else(|| rule_err("no such counter"))?;
    serde_json::from_str::<i64>(&raw).map_err(|e| rule_err(format!("corrupt counter: {e}")))
}

fn set_value(entity: &str, value: i64) -> Delta {
    Delta::Set(SetOp {
        entity: entity.to_string(),
        component: VALUE_KEY.to_string(),
        value: value.to_string(),
    })
}

impl Guest for Counter {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: "counter".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            plugin_type: PluginType::System,
            components: vec![ComponentSchema {
                key: VALUE_KEY.to_string(),
                version: 1,
                schema: json!({ "type": "integer" }).to_string(),
            }],
            commands: vec![
                CommandDecl {
                    name: "create-counter".to_string(),
                    params_schema: json!({
                        "type": "object",
                        "properties": { "name": { "type": "string" } },
                        "additionalProperties": false
                    })
                    .to_string(),
                },
                CommandDecl {
                    name: "increment".to_string(),
                    params_schema: json!({
                        "type": "object",
                        "properties": {
                            "entity": { "type": "string" },
                            "by": { "type": "integer" }
                        },
                        "required": ["entity"],
                        "additionalProperties": false
                    })
                    .to_string(),
                },
                CommandDecl {
                    name: "roll-add".to_string(),
                    params_schema: json!({
                        "type": "object",
                        "properties": {
                            "entity": { "type": "string" },
                            "expr": { "type": "string" }
                        },
                        "required": ["entity", "expr"],
                        "additionalProperties": false
                    })
                    .to_string(),
                },
                CommandDecl {
                    name: "delete-counter".to_string(),
                    params_schema: json!({
                        "type": "object",
                        "properties": { "entity": { "type": "string" } },
                        "required": ["entity"],
                        "additionalProperties": false
                    })
                    .to_string(),
                },
            ],
            sheet_layout: json!({
                "title": "Counter",
                "nameComponent": "core.name",
                "create": {
                    "command": "create-counter",
                    "label": "New Counter",
                    "fields": [{ "name": "name", "label": "Name", "type": "text" }]
                },
                "sections": [{
                    "label": "Counter",
                    "fields": [
                        { "widget": "number", "label": "Value", "component": VALUE_KEY, "field": "" }
                    ]
                }]
            })
            .to_string(),
        }
    }

    fn decide(cmd: Command, _context: String) -> Result<Vec<Delta>, RuleError> {
        match cmd.name.as_str() {
            "create-counter" => {
                let p = payload(&cmd)?;
                let entity = host::new_entity_id();
                let name = p
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Counter")
                    .to_string();
                Ok(vec![
                    Delta::Spawn(entity.clone()),
                    Delta::Set(SetOp {
                        entity: entity.clone(),
                        component: "core.name".to_string(),
                        value: Value::String(name).to_string(),
                    }),
                    set_value(&entity, 0),
                ])
            }
            "increment" => {
                let p = payload(&cmd)?;
                let entity = entity_arg(&p)?;
                let by = p.get("by").and_then(Value::as_i64).unwrap_or(1);
                let current = current_value(&entity)?;
                Ok(vec![set_value(&entity, current + by)])
            }
            "roll-add" => {
                let p = payload(&cmd)?;
                let entity = entity_arg(&p)?;
                let expr = p
                    .get("expr")
                    .and_then(Value::as_str)
                    .ok_or_else(|| rule_err("missing expr"))?;
                let roll = host::roll(expr).map_err(|e| rule_err(format!("bad roll: {e}")))?;
                let current = current_value(&entity)?;
                host::log(
                    tabula::plugin::types::LogLevel::Info,
                    &format!("counter roll {} = {}", roll.expr, roll.total),
                );
                Ok(vec![set_value(&entity, current + roll.total)])
            }
            "delete-counter" => {
                let p = payload(&cmd)?;
                let entity = entity_arg(&p)?;
                if !cmd.actor_is_gm {
                    return Err(rule_err("only the GM may delete counters"));
                }
                // exercise Remove before Despawn so all four variants appear in logs.
                Ok(vec![
                    Delta::Remove(RemoveOp {
                        entity: entity.clone(),
                        component: VALUE_KEY.to_string(),
                    }),
                    Delta::Despawn(entity),
                ])
            }
            other => Err(rule_err(format!("unknown command {other:?}"))),
        }
    }

    fn migrate(_key: String, _old_version: u32, value: String) -> String {
        value
    }
}

export!(Counter);
