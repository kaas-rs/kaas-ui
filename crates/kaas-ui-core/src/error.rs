//! `kafka_conn::Error` → an HTTP status and a wire-shaped error.
//!
//! Two rows of the table are load-bearing and easy to get wrong:
//!
//! * [`Error::Authentication`] is **502**, not 401. A 401 means the person
//!   using kaas-ui is not logged in. A cluster whose SASL credentials were
//!   rejected is a server-side configuration fault, and logging the user out
//!   because someone else's broker refused us is a bug.
//! * [`Error::ReadOnly`] reaching a client is **500**, not 405. The gate in
//!   kaas-lib is the second line of defence; if it fires, kaas-ui built a
//!   request it should have been incapable of building, because no mutating
//!   route exists in the router at all.

use kafka_conn::Error;
use serde::Serialize;
use utoipa::ToSchema;

/// The error taxonomy as the frontend sees it.
///
/// A projection of [`kafka_conn::Error`]'s variants, not a re-listing: the
/// frontend renders a transport failure and a closed connection identically,
/// so they share a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// The cluster could not be reached.
    Transport,
    /// A request did not come back in time.
    Timeout,
    /// The *cluster's* credentials were rejected. Never the user's.
    Auth,
    /// The cluster's principal lacks an ACL.
    Authorization,
    /// The broker answered with an error code.
    Broker,
    /// kaas-lib could not decode the response. This is a library bug.
    Decode,
    /// The cluster does not implement the api, or this build cannot speak it.
    Unsupported,
    /// The request was malformed before it left.
    Invalid,
    /// kaas-ui built a mutating request. This is a kaas-ui bug.
    ReadOnly,
    /// A variant kaas-lib grew after this build. `Error` is `non_exhaustive`,
    /// so this arm is reachable by a library bump alone; it renders as an
    /// unclassified failure rather than being misfiled under a neighbour.
    Other,
}

impl ErrorKind {
    /// The kind of an error.
    pub fn of(error: &Error) -> Self {
        match error {
            Error::Transport { .. } | Error::ConnectionClosed { .. } => Self::Transport,
            Error::Timeout { .. } => Self::Timeout,
            Error::Authentication(_) => Self::Auth,
            Error::Authorization { .. } => Self::Authorization,
            Error::Broker { .. } => Self::Broker,
            Error::Decode { .. } => Self::Decode,
            Error::ReadOnly { .. } => Self::ReadOnly,
            Error::UnsupportedApi { .. } | Error::Unsupported(_) => Self::Unsupported,
            Error::InvalidRequest(_) => Self::Invalid,
            _ => Self::Other,
        }
    }

    /// Whether an affected resource is worth retrying on its own.
    pub fn retriable(self) -> bool {
        matches!(self, Self::Transport | Self::Timeout)
    }
}

/// The status a whole-request failure maps to.
///
/// Returned as a number rather than an `axum::http::StatusCode` so this crate
/// stays free of HTTP: the router does the conversion, and there is exactly
/// one table.
pub fn http_status(error: &Error) -> u16 {
    match error {
        Error::Transport { .. } | Error::ConnectionClosed { .. } => 502,
        Error::Timeout { .. } => 504,
        // Deliberately not 401. See the module comment.
        Error::Authentication(_) => 502,
        Error::Authorization { .. } => 403,
        Error::Broker { .. } => 400,
        // "kaas-lib is wrong" and "kaas-ui is wrong" are both ours to fix.
        Error::Decode { .. } | Error::ReadOnly { .. } => 500,
        Error::UnsupportedApi { .. } => 501,
        Error::Unsupported(_) | Error::InvalidRequest(_) => 400,
        _ => 500,
    }
}

/// The two version ranges an [`Error::UnsupportedApi`] carries.
///
/// The pair is a diagnosis rather than a failure, and the three cases render
/// differently:
///
/// * `ours: None` — this build has no schema for the key; bump the codec.
/// * `broker: None` — the cluster does not implement it at all.
/// * both present but disjoint — the cluster is behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnsupportedApiDetail {
    /// The api key's name.
    pub api: String,
    /// Its number, which is the searchable thing when the name is unknown.
    pub api_key: i16,
    /// What the broker advertises, as `[min, max]`.
    pub broker: Option<[i16; 2]>,
    /// What this build speaks, as `[min, max]`.
    pub ours: Option<[i16; 2]>,
}

impl UnsupportedApiDetail {
    /// Extract the detail, if this is an [`Error::UnsupportedApi`].
    pub fn of(error: &Error) -> Option<Self> {
        match error {
            Error::UnsupportedApi {
                api_key,
                broker,
                ours,
            } => Some(Self {
                api: api_key.name().to_owned(),
                api_key: api_key.code(),
                broker: broker.map(|(min, max)| [min, max]),
                ours: ours.map(|(min, max)| [min, max]),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kafka_conn::{ApiKey, ErrorCode};

    #[test]
    fn a_clusters_bad_credentials_never_log_the_user_out() {
        let err = Error::Authentication("SASL handshake failed".into());
        assert_eq!(http_status(&err), 502);
        assert_ne!(http_status(&err), 401);
        assert_eq!(ErrorKind::of(&err), ErrorKind::Auth);
    }

    #[test]
    fn read_only_reaching_a_client_is_our_bug_not_a_method_error() {
        let err = Error::ReadOnly {
            api_key: ApiKey::CreateTopics,
        };
        assert_eq!(http_status(&err), 500);
    }

    #[test]
    fn unsupported_api_keeps_both_ranges() {
        // `DescribeCluster` on a broker that does not implement it: the broker
        // half is None and ours is not, which is a different render from the
        // reverse.
        let err = Error::UnsupportedApi {
            api_key: ApiKey::DescribeCluster,
            broker: None,
            ours: Some((0, 2)),
        };
        let detail = UnsupportedApiDetail::of(&err).unwrap();
        assert_eq!(detail.api_key, 60);
        assert_eq!(detail.broker, None);
        assert_eq!(detail.ours, Some([0, 2]));
        assert_eq!(http_status(&err), 501);
    }

    #[test]
    fn broker_errors_are_a_client_problem_not_a_server_one() {
        let err = Error::from_code(ErrorCode::UnknownTopicOrPartition, None);
        assert_eq!(http_status(&err), 400);
        assert_eq!(ErrorKind::of(&err), ErrorKind::Broker);
    }
}
