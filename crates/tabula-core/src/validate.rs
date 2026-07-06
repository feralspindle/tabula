//! the capability/consistency validator, the gate between `decide` and the log.
//!
//! every delta batch a plugin returns passes through here before it may be
//! appended. rejection is atomic: one bad delta rejects the whole batch, so a
//! record is either fully valid or never written (invariant #6). `apply` can then
//! stay total because nothing ill-formed ever reaches it.

use std::collections::BTreeSet;

use crate::{ComponentKey, Delta, EntityId, SchemaError, SchemaRegistry, World};

/// write capability for one plugin: the component namespaces it declared in its
/// manifest, plus any `core.*` keys the host granted it (invariant #7).
#[derive(Debug, Clone, Default)]
pub struct Grants {
    namespaces: BTreeSet<String>,
    core_keys: BTreeSet<ComponentKey>,
}

impl Grants {
    pub fn new() -> Self {
        Self::default()
    }

    /// grants write access to a whole namespace (from the plugin manifest).
    /// `core` is host-reserved and cannot be granted wholesale.
    pub fn allow_namespace(&mut self, namespace: impl Into<String>) -> &mut Self {
        let ns = namespace.into();
        assert_ne!(
            ns, "core",
            "core namespace is granted per-key, never wholesale"
        );
        self.namespaces.insert(ns);
        self
    }

    /// grants write access to a single host-owned `core.*` key (e.g. `core.name`).
    pub fn allow_core_key(&mut self, key: ComponentKey) -> &mut Self {
        assert_eq!(
            key.namespace(),
            "core",
            "allow_core_key takes core.* keys only"
        );
        self.core_keys.insert(key);
        self
    }

    pub fn permits(&self, key: &ComponentKey) -> bool {
        if key.namespace() == "core" {
            self.core_keys.contains(key)
        } else {
            self.namespaces.contains(key.namespace())
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeltaViolation {
    #[error("spawn of entity {0} which already exists")]
    SpawnExisting(EntityId),
    #[error("{op} on unknown entity {entity}")]
    UnknownEntity { op: &'static str, entity: EntityId },
    #[error("plugin wrote undeclared component namespace: {0}")]
    NamespaceNotGranted(ComponentKey),
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

/// validates a delta batch against the current world, the schema registry, and
/// the producing plugin's grants. batch-local effects are tracked so that e.g.
/// `[Spawn(e), Set(e, …)]` validates even though `e` is not yet in the world.
pub fn validate_deltas(
    world: &World,
    registry: &SchemaRegistry,
    grants: &Grants,
    deltas: &[Delta],
) -> Result<(), DeltaViolation> {
    let mut spawned: BTreeSet<EntityId> = BTreeSet::new();
    let mut despawned: BTreeSet<EntityId> = BTreeSet::new();

    let exists = |e: EntityId, spawned: &BTreeSet<EntityId>, despawned: &BTreeSet<EntityId>| {
        (world.contains(e) || spawned.contains(&e)) && !despawned.contains(&e)
    };

    for delta in deltas {
        match delta {
            Delta::Spawn { entity } => {
                if exists(*entity, &spawned, &despawned) {
                    return Err(DeltaViolation::SpawnExisting(*entity));
                }
                spawned.insert(*entity);
                despawned.remove(entity);
            }
            Delta::Despawn { entity } => {
                if !exists(*entity, &spawned, &despawned) {
                    return Err(DeltaViolation::UnknownEntity {
                        op: "despawn",
                        entity: *entity,
                    });
                }
                despawned.insert(*entity);
            }
            Delta::Set {
                entity,
                component,
                value,
            } => {
                if !exists(*entity, &spawned, &despawned) {
                    return Err(DeltaViolation::UnknownEntity {
                        op: "set",
                        entity: *entity,
                    });
                }
                if !grants.permits(component) {
                    return Err(DeltaViolation::NamespaceNotGranted(component.clone()));
                }
                registry.validate(component, value)?;
            }
            Delta::Remove { entity, component } => {
                if !exists(*entity, &spawned, &despawned) {
                    return Err(DeltaViolation::UnknownEntity {
                        op: "remove",
                        entity: *entity,
                    });
                }
                if !grants.permits(component) {
                    return Err(DeltaViolation::NamespaceNotGranted(component.clone()));
                }
            }
        }
    }
    Ok(())
}
