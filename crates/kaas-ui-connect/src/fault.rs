//! Why a Connect call produced no answer.

use reqwest::StatusCode;

/// What Jersey answers for a path it has no resource for.
///
/// The only thing that tells an older worker's missing endpoint apart from a
/// missing connector: both are `404`, and a missing connector says
/// `"Connector x not found"` or `"No status found for connector x"`. Measured
/// against the Strimzi worker, 2026-10-20.
const NO_SUCH_PATH: &str = "HTTP 404 Not Found";

/// Why a Connect call produced no answer.
///
/// Every variant carries the sentence to render. They are separate because
/// they render differently: one is somebody else's outage, one is a line in
/// the config file, one is a feature this worker does not have, one is a name
/// that does not exist.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectFault {
    /// It could not be reached, timed out, or answered with a server error.
    #[error("{0}")]
    Unreachable(String),
    /// It refused kaas-ui's credentials.
    ///
    /// Never a `401` to the *user*: Connect rejecting the credentials kaas-ui
    /// was configured with says nothing about who is signed in, and logging
    /// them out over it is the trap `Error::Authentication` already taught.
    #[error("{0}")]
    Refused(String),
    /// It answered with something that is not the Connect API.
    #[error("{0}")]
    Misconfigured(String),
    /// The workers are rebalancing. Retry shortly.
    #[error("{0}")]
    Rebalancing(String),
    /// The named connector, task or plugin does not exist.
    #[error("{0}")]
    NotFound(String),
    /// This worker has no such endpoint — an older release, or a feature
    /// switched off. Absence, rendered by hiding a panel; never an error page.
    #[error("{0}")]
    Absent(String),
    /// Any other refusal, verbatim.
    #[error("{0}")]
    Rejected(String),
}

impl ConnectFault {
    /// Whether retrying is worth offering.
    #[must_use]
    pub fn retriable(&self) -> bool {
        matches!(self, Self::Unreachable(_) | Self::Rebalancing(_))
    }

    /// Classify a non-2xx response.
    pub(crate) fn of_status(
        status: StatusCode,
        message: Option<String>,
        id: &str,
        url: &str,
        path: &str,
    ) -> Self {
        let said = message.clone().unwrap_or_else(|| status.to_string());
        if status.is_server_error() {
            return Self::Unreachable(format!(
                "Connect cluster {id:?} at {url} answered {status} for {path}: {said}"
            ));
        }
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Self::Refused(format!(
                "Connect cluster {id:?} at {url} refused kaas-ui's credentials ({status}): check \
                 `username`/`password` or `bearer_token` on its `connect_clusters` entry"
            )),
            StatusCode::CONFLICT => Self::Rebalancing(format!(
                "Connect cluster {id:?} is rebalancing and cannot answer {path} right now: {said}"
            )),
            StatusCode::NOT_FOUND if message.as_deref() == Some(NO_SUCH_PATH) => {
                Self::Absent(format!(
                    "Connect cluster {id:?} has no {path} endpoint — an older worker, or a \
                     feature switched off"
                ))
            }
            StatusCode::NOT_FOUND => Self::NotFound(said),
            _ => Self::Rejected(format!(
                "Connect cluster {id:?} answered {status} for {path}: {said}"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(status: u16, message: &str) -> ConnectFault {
        ConnectFault::of_status(
            StatusCode::from_u16(status).unwrap(),
            Some(message.to_owned()),
            "connect",
            "http://connect:8083",
            "/connectors/x/offsets",
        )
    }

    #[test]
    fn a_missing_path_and_a_missing_connector_are_two_answers() {
        // Both measured on the Strimzi worker.
        assert!(matches!(
            of(404, "HTTP 404 Not Found"),
            ConnectFault::Absent(_)
        ));
        assert!(matches!(
            of(404, "Connector nope not found"),
            ConnectFault::NotFound(_)
        ));
        assert!(matches!(
            of(404, "No status found for connector nope"),
            ConnectFault::NotFound(_)
        ));
    }

    #[test]
    fn credentials_and_rebalances_and_outages_are_kept_apart() {
        assert!(matches!(of(401, "x"), ConnectFault::Refused(_)));
        assert!(matches!(of(403, "x"), ConnectFault::Refused(_)));
        assert!(matches!(of(409, "x"), ConnectFault::Rebalancing(_)));
        assert!(matches!(of(503, "x"), ConnectFault::Unreachable(_)));
        assert!(of(409, "x").retriable());
        assert!(!of(401, "x").retriable());
    }
}
