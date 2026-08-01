//! Per-cluster health, as a state machine the fleet card renders directly.

use std::time::SystemTime;

use serde::Serialize;
use utoipa::ToSchema;

use crate::error::ErrorKind;

/// Where one cluster is in its lifecycle.
///
/// A connect failure lands here and stops. It never propagates out of the
/// handle, because one unreachable cluster must not blank a page about the
/// eleven that are fine.
#[derive(Debug, Clone)]
pub enum ClusterHealth {
    /// No connection attempt has finished yet.
    Connecting {
        /// When the first attempt started.
        since: SystemTime,
        /// How many attempts have been made.
        attempts: u32,
    },
    /// Connected, with a metadata snapshot.
    Ready {
        /// When the connection was established.
        since: SystemTime,
    },
    /// The last attempt failed. A retry is scheduled with backoff.
    Unreachable {
        /// The failure, rendered.
        error: String,
        /// Its taxonomy — the frontend renders a timeout differently from a
        /// refused connection.
        kind: ErrorKind,
        /// When it first became unreachable.
        since: SystemTime,
        /// How many attempts have failed since.
        attempts: u32,
    },
}

impl ClusterHealth {
    /// The initial state: nothing attempted, nothing connected.
    pub fn connecting() -> Self {
        Self::Connecting {
            since: SystemTime::now(),
            attempts: 0,
        }
    }

    /// The wire form.
    pub fn status(&self) -> ClusterStatus {
        match self {
            Self::Connecting { .. } => ClusterStatus::Connecting,
            Self::Ready { .. } => ClusterStatus::Ready,
            Self::Unreachable { .. } => ClusterStatus::Unreachable,
        }
    }

    /// How many attempts have been made or failed.
    pub fn attempts(&self) -> u32 {
        match self {
            Self::Connecting { attempts, .. } | Self::Unreachable { attempts, .. } => *attempts,
            Self::Ready { .. } => 0,
        }
    }
}

/// The health state, flattened for the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ClusterStatus {
    /// Not connected yet, no failure recorded.
    Connecting,
    /// Connected.
    Ready,
    /// The last connection attempt failed.
    Unreachable,
}
