//! tabula log core.
//!
//! pure, I/O-free heart of the kernel: the closed delta vocabulary
//! ([`Delta`]/[`LogRecord`]), the in-memory projection ([`World`]) with its single
//! game-agnostic [`apply`] fold, the component [`SchemaRegistry`], and the
//! capability/consistency validator ([`validate_deltas`]).
//!
//! invariants this crate embodies (see repo CLAUDE.md):
//! - The `Delta` enum is closed. host-owned. no plugin can extend it.
//! - Replay never executes plugin code: `apply` is total and game-agnostic.
//! - Every record carries a `Cause`.

mod delta;
mod schema;
mod validate;
mod world;

pub use delta::{Actor, Cause, ComponentKey, Delta, EntityId, GameTime, LogRecord, PluginRef};
pub use schema::{SchemaError, SchemaRegistry};
pub use validate::{validate_deltas, DeltaViolation, Grants};
pub use world::{apply, apply_record, World};

/// in-memory component value. JSON for the MVP (CBOR on disk is an open question
/// deferred by the spec; the log stores JSONB for now).
pub type Value = serde_json::Value;
