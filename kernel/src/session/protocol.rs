//! WS wire protocol. JSON frames (MVP encoding decision, spec §10).
//!
//! client → server: `command`.
//! server → client: `snapshot` on join, then seq-ordered `record` frames;
//! `error` goes only to the client whose command failed (rule errors are never
//! logged to the delta log).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use tabula_core::{LogRecord, World};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ClientFrame {
    Command {
        /// client-minted correlation id; lands in the record's cause and in
        /// any error frame, so the client can match responses.
        id: Uuid,
        name: String,
        #[serde(default)]
        payload: Value,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ServerFrame {
    /// current projection at join/reconnect. clients replace local state, then
    /// fold subsequent records with seq > seq.
    Snapshot { seq: u64, world: World },
    /// one applied log record; clients mirror the kernel's fold.
    Record { record: LogRecord },
    /// command rejected (rule error, validation, authz). sent only to the
    /// issuing client.
    Error {
        command_id: Option<Uuid>,
        message: String,
    },
}
