//! Whole-request failures.
//!
//! Per-resource failures do not come through here — they ride in the envelope
//! and the response is still `200 OK`. This is for the cases where there is no
//! answer at all: an unknown cluster, a cluster that has not connected, or a
//! call that failed outright.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kaas_ui_core::error::{ErrorKind, UnsupportedApiDetail, http_status};
use kafka_conn::Error;
use serde::Serialize;
use utoipa::ToSchema;

/// A request that produced no answer.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    body: ApiErrorBody,
}

/// The body of a failed request.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorBody {
    /// The failure, rendered.
    pub message: String,
    /// Its taxonomy, where it came from a cluster.
    pub kind: Option<ErrorKind>,
    /// The broker's error code name.
    pub code: Option<String>,
    /// Its number, which survives when the name does not.
    pub code_number: Option<i16>,
    /// Both version ranges, when an api was unsupported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsupported_api: Option<UnsupportedApiDetail>,
    /// Whether retrying is worth offering.
    pub retriable: bool,
}

impl ApiError {
    /// `404`. Used for a cluster id that is not configured — **and** for one
    /// the caller may not see, so ids are not enumerable by probing.
    pub fn not_found(what: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            body: ApiErrorBody {
                message: what.into(),
                kind: None,
                code: None,
                code_number: None,
                unsupported_api: None,
                retriable: false,
            },
        }
    }

    /// `403`. The caller can see this cluster but not do this to it.
    ///
    /// The narrow companion to [`ApiError::not_found`], and the distinction is
    /// the whole design: a cluster nobody may see does not exist (404), while
    /// a cluster somebody may browse but not read payloads from says so out
    /// loud (403). Confirming a topic's *existence* to a caller who already
    /// holds `metadata` on that cluster gives nothing away; confirming a
    /// cluster id to someone with no role at all does.
    pub fn forbidden(what: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            body: ApiErrorBody {
                message: what.into(),
                kind: None,
                code: None,
                code_number: None,
                unsupported_api: None,
                retriable: false,
            },
        }
    }

    /// `502`. The identity provider could not be reached or refused.
    ///
    /// Never `401`: the caller did nothing wrong, and asking them to sign in
    /// again when signing in is what failed is a loop.
    pub fn bad_gateway_login(detail: &str) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            body: ApiErrorBody {
                message: format!("the login provider is unreachable: {detail}"),
                kind: None,
                code: None,
                code_number: None,
                unsupported_api: None,
                retriable: true,
            },
        }
    }

    /// `500`. The read happened but could not be recorded, so it is refused.
    ///
    /// The payload is not sent. An audit log that a request can proceed
    /// without is not an audit log, and the only way to keep that true is for
    /// the failure to be fatal here rather than logged and shrugged at.
    pub fn audit_failed(detail: &str) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: ApiErrorBody {
                message: format!(
                    "this read was not served because it could not be recorded: {detail}"
                ),
                kind: None,
                code: None,
                code_number: None,
                unsupported_api: None,
                retriable: true,
            },
        }
    }

    /// `503`. The cluster is configured but has not connected yet, or its last
    /// attempt failed. Distinct from `502`: nothing was asked of a broker.
    pub fn not_connected(cluster: &str, detail: &str) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            body: ApiErrorBody {
                message: format!("cluster {cluster} is not connected: {detail}"),
                kind: Some(ErrorKind::Transport),
                code: None,
                code_number: None,
                unsupported_api: None,
                retriable: true,
            },
        }
    }

    /// `400`. The query string said something impossible.
    pub fn bad_request(what: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            body: ApiErrorBody {
                message: what.into(),
                kind: Some(ErrorKind::Invalid),
                code: None,
                code_number: None,
                unsupported_api: None,
                retriable: false,
            },
        }
    }

    /// `404`. There is no record at that offset.
    ///
    /// Distinct from a missing topic: the topic is there, and the offset is
    /// either past its end, below what it still retains, or — on a compacted
    /// topic — a gap where a record used to be. All three are "that row is not
    /// there", which is what a detail panel needs to say.
    pub fn offset_out_of_range(topic: &str, partition: i32, offset: i64) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            body: ApiErrorBody {
                message: format!(
                    "{topic}-{partition} holds no record at offset {offset}: it is outside the \
                     retained range, or was compacted away"
                ),
                kind: Some(ErrorKind::Broker),
                code: Some("OFFSET_OUT_OF_RANGE".to_owned()),
                code_number: None,
                unsupported_api: None,
                retriable: false,
            },
        }
    }

    /// `429`. Too many streams are open to start another.
    ///
    /// A ceiling rather than a queue: a caller who is told to wait sits on a
    /// connection doing nothing, and a message stream is exactly the kind of
    /// request someone opens five of and forgets.
    pub fn too_many_requests(what: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            body: ApiErrorBody {
                message: what.into(),
                kind: Some(ErrorKind::Invalid),
                code: None,
                code_number: None,
                unsupported_api: None,
                retriable: true,
            },
        }
    }

    /// `504`. A cluster call ran past the request ceiling.
    pub fn timed_out(what: &str, after: std::time::Duration) -> Self {
        Self {
            status: StatusCode::GATEWAY_TIMEOUT,
            body: ApiErrorBody {
                message: format!("{what} did not answer within {after:?}"),
                kind: Some(ErrorKind::Timeout),
                code: None,
                code_number: None,
                unsupported_api: None,
                retriable: true,
            },
        }
    }

    /// The status, for tests and for the timing assertions in `xtask live`.
    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        // `ReadOnly` reaching here is a kaas-ui bug, not a 405: there is no
        // mutating route in the router at all, so the gate firing means we
        // built a request we should have been incapable of building.
        if let Error::ReadOnly { api_key } = &error {
            tracing::error!(
                api_key = %api_key,
                "the read-only gate fired: kaas-ui built a mutating request, which is a bug"
            );
        }

        let code = error.code();
        let status = StatusCode::from_u16(http_status(&error)).unwrap_or(StatusCode::BAD_GATEWAY);
        Self {
            status,
            body: ApiErrorBody {
                message: error.to_string(),
                kind: Some(ErrorKind::of(&error)),
                code: code.and_then(|c| c.name()).map(str::to_owned),
                code_number: code.map(|c| c.code()),
                unsupported_api: UnsupportedApiDetail::of(&error),
                retriable: error.retriable(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

/// What a handler returns.
pub type ApiResult<T> = Result<T, ApiError>;

/// A cluster call that did not answer.
///
/// Kept distinct from [`ApiError`] because the same failure means two
/// different things depending on where it happened: a call the page is *about*
/// is a failed request, and a call that merely *enriches* the page is one
/// named entry in the envelope's `errors`. Both renderings need the original
/// error, so it is not flattened to a status until the last moment.
#[derive(Debug)]
pub enum CallError {
    /// The cluster answered with a failure.
    Kafka(Error),
    /// The call ran past the request ceiling.
    TimedOut {
        /// What was being called.
        what: String,
        /// How long it had.
        after: std::time::Duration,
    },
}

impl CallError {
    /// Render as one named resource's failure, for the envelope.
    pub fn into_resource_error(self, resource: &str) -> kaas_ui_core::ResourceError {
        match self {
            Self::Kafka(error) => kaas_ui_core::ResourceError::new(resource, &error),
            Self::TimedOut { what, after } => kaas_ui_core::ResourceError {
                resource: resource.to_owned(),
                kind: ErrorKind::Timeout,
                code: None,
                code_number: None,
                message: format!("{what} did not answer within {after:?}"),
                unsupported_api: None,
                retriable: true,
            },
        }
    }
}

impl From<CallError> for ApiError {
    fn from(error: CallError) -> Self {
        match error {
            CallError::Kafka(error) => Self::from(error),
            CallError::TimedOut { what, after } => Self::timed_out(&what, after),
        }
    }
}
