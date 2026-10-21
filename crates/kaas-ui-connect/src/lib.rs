//! Kafka Connect for kaas-ui.
//!
//! A Connect cluster is dialled over its REST API, so this crate is kaas-ui's
//! and not kaas-lib's: kaas-lib is a Kafka client, and Connect is not the
//! Kafka protocol. Three things live here.
//!
//! * [`ConnectClient`] — one per configured Connect cluster, lazy, read-only
//!   by construction: every call is a `GET` through one private function.
//! * [`ConnectWriter`] — Phase 10's write side, reachable only with a `Grant`
//!   from the gate, and checked against the operation and connector it names.
//! * [`redact`] — secrets taken out of connector configs before any public
//!   type holds them, by the plugin's declared `PASSWORD` keys and by name.
//! * [`ConnectorStatus`] — a connector's state and its tasks' folded into one,
//!   with paused kept apart from failed.
//!
//! No Kafka version number appears. A worker without an endpoint answers a
//! path-404, which is [`ConnectFault::Absent`], and the UI decides what that
//! absence looks like.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod client;
pub mod fault;
pub mod model;
pub mod redact;
pub mod settings;
mod wire;
pub mod writer;

pub use client::{ConnectClient, ConnectError, ConnectHealth, ConnectStatus};
pub use fault::ConnectFault;
pub use model::{
    ConnectorDetail, ConnectorStatus, ConnectorSummary, ConnectorType, MirrorKind, MirrorRole,
    Plugin, RedactedConfig, ReplicationPolicy, ServerInfo, StoredOffset, TaskConfig, TaskStatus,
    Validation, ValidationError,
};
pub use redact::{ConnectorConfigEntry, REDACTED, Redaction};
pub use settings::{ConnectAuth, ConnectSettings, ConnectTls};
pub use writer::{ConnectWriter, PreparedConfig};
