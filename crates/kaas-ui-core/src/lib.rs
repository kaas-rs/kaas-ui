//! Configuration, the cluster registry, and the domain types kaas-ui speaks.
//!
//! This crate knows about kaas-lib and nothing about HTTP. It is where the
//! four properties the plan cares about are preserved rather than flattened:
//! per-item results stay per-item, [`kafka_conn::Error`] stays typed until the
//! boundary, `snapshot.age()` rides along, and the version table is projected
//! rather than recomputed.
//!
//! No upstream type appears in a public signature — [`dto`] owns the shapes
//! that reach the wire, and the `From` impls that build them are the only
//! place a `kafka_meta` or `kafka_admin` type is touched by anything outside
//! this crate.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod capabilities;
pub mod config;
pub mod decode;
pub mod dto;
pub mod envelope;
pub mod error;
pub mod health;
pub mod registry;

pub use config::{ClusterEntry, Config};
pub use envelope::{Envelope, ResourceError};
pub use error::ErrorKind;
pub use health::ClusterHealth;
pub use registry::{ClusterHandle, Registry};
