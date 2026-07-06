//! the delta log vocabulary.
//!
//! OWNER-AUTHORED CONTRACT (irreversible #1 of 3). this file transcribes the
//! `Delta`/`LogRecord` schema exactly as specified in docs/ARCHITECTURE.md §4.2.
//! it is read-only for the implementation agent: changes here require an explicit
//! owner decision. in particular the `Delta` enum is CLOSED, never add variants.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Value;

/// opaque entity identity. UUID v7 so ids sort by creation time, which keeps the
/// log and any id-ordered scans local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(pub Uuid);

impl EntityId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for EntityId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for EntityId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// namespaced component key, `namespace.name`, e.g. `shadowdark.stats`,
/// `core.name`. the namespace (everything before the first `.`) is the unit of
/// write capability: plugins may only `Set`/`Remove` in namespaces they declared.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ComponentKey(String);

impl ComponentKey {
    /// A key must be `namespace.name` with both parts non-empty and made of
    /// lowercase alphanumerics, `-` and `_` (dots beyond the first separate
    /// nested names and are allowed).
    pub fn new(key: impl Into<String>) -> Result<Self, InvalidComponentKey> {
        let key = key.into();
        let valid_part = |p: &str| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        };
        match key.split_once('.') {
            Some((ns, rest)) if valid_part(ns) && rest.split('.').all(valid_part) => Ok(Self(key)),
            _ => Err(InvalidComponentKey(key)),
        }
    }

    pub fn namespace(&self) -> &str {
        self.0.split_once('.').map(|(ns, _)| ns).unwrap_or(&self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ComponentKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid component key {0:?}: expected namespace.name in lowercase [a-z0-9_-]")]
pub struct InvalidComponentKey(pub String);

impl std::str::FromStr for ComponentKey {
    type Err = InvalidComponentKey;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// the complete, closed instruction set of the log. host-owned. game-agnostic.
///
/// CLOSED ENUM, never add variants to solve a feature problem; solve it in
/// components or `cause` (CLAUDE.md invariant #2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum Delta {
    Spawn {
        entity: EntityId,
    },
    Despawn {
        entity: EntityId,
    },
    Set {
        entity: EntityId,
        component: ComponentKey,
        value: Value,
    },
    Remove {
        entity: EntityId,
        component: ComponentKey,
    },
}

impl Delta {
    pub fn entity(&self) -> EntityId {
        match self {
            Delta::Spawn { entity }
            | Delta::Despawn { entity }
            | Delta::Set { entity, .. }
            | Delta::Remove { entity, .. } => *entity,
        }
    }
}

/// host session clock reading, milliseconds since the Unix epoch. named
/// `GameTime` because it is the only clock a session ever sees: plugins read it
/// through the `now()` host import, records stamp it in `at`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct GameTime(pub u64);

/// who initiated the change. `Timer` and `GmOverride` exist so provenance is
/// honest even when no player issued a command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Actor {
    User { id: Uuid },
    GmOverride { id: Uuid },
    Timer,
    System,
}

/// identifies the plugin build that produced a record's deltas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRef {
    pub id: String,
    pub version: String,
}

/// provenance for a record, mandatory on every record (invariant #5). captures
/// what the delta vocabulary can't express: which command, which plugin build,
/// which actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cause {
    /// correlation id of the originating command (client-minted or host-minted).
    pub command_id: Uuid,
    /// declared command name (e.g. `roll-check`), `None` for host-internal causes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// plugin that decided the deltas, `None` for host-authored records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<PluginRef>,
    pub actor: Actor,
}

/// one atomically-applied entry of the per-session delta log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogRecord {
    /// per-session, monotonic, gapless, starting at 1.
    pub seq: u64,
    /// host session clock at append time.
    pub at: GameTime,
    pub cause: Cause,
    /// applied atomically, all or none.
    pub deltas: Vec<Delta>,
}
