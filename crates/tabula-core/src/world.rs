//! the in-memory projection and its single fold function.
//!
//! `apply` is game-agnostic and must remain so forever, replay never executes
//! plugin code (CLAUDE.md invariant #3). it is also *total*: it cannot fail.
//! well-formedness (spawn-before-set, no double spawn, schema validity, namespace
//! capability) is enforced by `validate_deltas` BEFORE a record is appended, so by
//! the time a delta reaches `apply`, live or in replay, it is known-good.
//! defensively, ill-formed deltas degrade to no-ops rather than inventing state,
//! keeping replay deterministic even against a hand-edited log.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{ComponentKey, Delta, EntityId, LogRecord, Value};

/// `entity → component → value`. BTreeMaps so serialization order is canonical:
/// two worlds with equal contents serialize byte-identically, which the
/// replay-equivalence property tests rely on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct World {
    entities: BTreeMap<EntityId, BTreeMap<ComponentKey, Value>>,
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, entity: EntityId) -> bool {
        self.entities.contains_key(&entity)
    }

    pub fn get(&self, entity: EntityId, component: &ComponentKey) -> Option<&Value> {
        self.entities.get(&entity)?.get(component)
    }

    pub fn components(&self, entity: EntityId) -> Option<&BTreeMap<ComponentKey, Value>> {
        self.entities.get(&entity)
    }

    pub fn entities(&self) -> impl Iterator<Item = (EntityId, &BTreeMap<ComponentKey, Value>)> {
        self.entities.iter().map(|(id, c)| (*id, c))
    }

    /// entities that currently have `component`, in id (creation) order.
    pub fn entities_with(&self, component: &ComponentKey) -> Vec<EntityId> {
        self.entities
            .iter()
            .filter(|(_, comps)| comps.contains_key(component))
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

/// THE fold. total, deterministic, game-agnostic.
pub fn apply(world: &mut World, delta: &Delta) {
    match delta {
        Delta::Spawn { entity } => {
            // double-spawn is rejected by validation; degrade to no-op so a
            // replayed record can never clobber existing components.
            world.entities.entry(*entity).or_default();
        }
        Delta::Despawn { entity } => {
            world.entities.remove(entity);
        }
        Delta::Set {
            entity,
            component,
            value,
        } => {
            // set on a never-spawned entity is rejected by validation; no-op here
            // rather than implicitly spawning.
            if let Some(components) = world.entities.get_mut(entity) {
                components.insert(component.clone(), value.clone());
            }
        }
        Delta::Remove { entity, component } => {
            if let Some(components) = world.entities.get_mut(entity) {
                components.remove(component);
            }
        }
    }
}

/// folds one record. records are atomic by construction: a record is only ever
/// appended whole, and `apply` cannot fail, so applying its deltas in order is
/// all-or-nothing at the log level.
pub fn apply_record(world: &mut World, record: &LogRecord) {
    for delta in &record.deltas {
        apply(world, delta);
    }
}
