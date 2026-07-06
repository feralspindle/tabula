//! runtime component schema registration.
//!
//! plugins declare JSON-Schema definitions for every component key they own; the
//! kernel registers them at plugin load and validates every `Set` delta's value
//! before it can reach the log (CLAUDE.md invariant 8c).

use std::collections::HashMap;

use crate::{ComponentKey, Value};

pub struct SchemaRegistry {
    schemas: HashMap<ComponentKey, RegisteredSchema>,
}

struct RegisteredSchema {
    /// plugin-declared schema version, consumed by `migrate` at snapshot load.
    version: u32,
    validator: jsonschema::Validator,
}

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("component {0} has no registered schema")]
    Unregistered(ComponentKey),
    #[error("component {0} is already registered")]
    AlreadyRegistered(ComponentKey),
    #[error("invalid JSON Schema for {key}: {message}")]
    InvalidSchema { key: ComponentKey, message: String },
    #[error("value for {key} violates schema: {message}")]
    Violation { key: ComponentKey, message: String },
}

impl SchemaRegistry {
    pub fn new() -> Self {
        Self {
            schemas: HashMap::new(),
        }
    }

    /// registers a schema for a component key. one schema per key per session -
    /// re-registration is an error (a System plugin is loaded exactly once).
    pub fn register(
        &mut self,
        key: ComponentKey,
        version: u32,
        schema: &Value,
    ) -> Result<(), SchemaError> {
        if self.schemas.contains_key(&key) {
            return Err(SchemaError::AlreadyRegistered(key));
        }
        let validator =
            jsonschema::validator_for(schema).map_err(|e| SchemaError::InvalidSchema {
                key: key.clone(),
                message: e.to_string(),
            })?;
        self.schemas
            .insert(key, RegisteredSchema { version, validator });
        Ok(())
    }

    pub fn is_registered(&self, key: &ComponentKey) -> bool {
        self.schemas.contains_key(key)
    }

    pub fn version(&self, key: &ComponentKey) -> Option<u32> {
        self.schemas.get(key).map(|s| s.version)
    }

    /// validates a candidate `Set` value. unregistered keys are rejected: a value
    /// with no schema has no business in the log.
    pub fn validate(&self, key: &ComponentKey, value: &Value) -> Result<(), SchemaError> {
        let registered = self
            .schemas
            .get(key)
            .ok_or_else(|| SchemaError::Unregistered(key.clone()))?;
        registered
            .validator
            .validate(value)
            .map_err(|e| SchemaError::Violation {
                key: key.clone(),
                message: e.to_string(),
            })
    }
}

impl Default for SchemaRegistry {
    fn default() -> Self {
        Self::new()
    }
}
