//! The HTTP surface.
//!
//! Knows about [`kaas_ui_core`] and axum, and never opens a socket. Two rules
//! hold this crate together:
//!
//! * **Every data route is a `GET`.** There is no mutating endpoint — not
//!   disabled, not 403, absent from the router. A CI check greps for `.post(`,
//!   `.put(`, `.patch(` and `.delete(` and fails on any of them.
//! * **The registry is reached through one lookup.** [`AppState::cluster`] is
//!   the only way to a handle, and a cluster the caller cannot see is `404`
//!   rather than `403`, so ids are not enumerable by probing.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use axum::Router;
use axum::routing::get;
use kaas_ui_core::registry::{ClusterHandle, Registry};
use kafka_admin::Admin;

pub mod error;
pub mod openapi;
pub mod routes;

pub use error::{ApiError, ApiResult};

/// How long any one cluster call may take before the request gives up.
///
/// A generous ceiling rather than a tuning knob: kaas-lib has its own per-request
/// timeout, and this exists so a broker that accepts a connection and then says
/// nothing cannot pin a request handler open indefinitely.
const CALL_TIMEOUT: Duration = Duration::from_secs(20);

/// Everything a handler needs.
///
/// The registry sits behind an [`ArcSwap`] so a config reload can replace it
/// wholesale — reusing the handles that did not change — without any handler
/// holding a lock.
#[derive(Debug, Clone)]
pub struct AppState {
    registry: Arc<ArcSwap<Registry>>,
}

impl AppState {
    /// Wrap a registry.
    pub fn new(registry: Arc<ArcSwap<Registry>>) -> Self {
        Self { registry }
    }

    /// The current registry.
    pub fn registry(&self) -> arc_swap::Guard<Arc<Registry>> {
        self.registry.load()
    }

    /// **The only cluster lookup.**
    ///
    /// A cluster that is not configured — or, from the auth phase onward, one
    /// the caller may not see — is `404`. Not `403`: a 403 confirms the id
    /// exists, and confirming ids is how a registry becomes enumerable.
    pub fn cluster(&self, id: &str) -> ApiResult<Arc<ClusterHandle>> {
        self.registry()
            .get(id)
            .map(Arc::clone)
            .ok_or_else(|| ApiError::not_found(format!("no cluster {id:?}")))
    }

    /// A connected cluster, or `503` with what the connector last saw.
    ///
    /// Never waits for a connection. A cluster that is still connecting, or
    /// that failed, answers immediately — and gets nudged to retry now rather
    /// than at the end of its backoff, which is what the card's retry button
    /// is wired to.
    pub fn connected(&self, id: &str) -> ApiResult<(Arc<ClusterHandle>, Arc<Admin>)> {
        let handle = self.cluster(id)?;
        match handle.admin() {
            Some(admin) => Ok((handle, admin)),
            None => {
                handle.request_retry();
                let detail = match handle.health().as_ref() {
                    kaas_ui_core::ClusterHealth::Unreachable { error, .. } => error.clone(),
                    _ => "no connection attempt has finished yet".to_owned(),
                };
                Err(ApiError::not_connected(id, &detail))
            }
        }
    }
}

/// Run a cluster call under the request ceiling.
///
/// The caller decides what the failure means: `?` turns it into a failed
/// request, and [`CallError::into_resource_error`] turns it into one named
/// entry in the envelope instead.
pub(crate) async fn call<T>(
    what: &str,
    future: impl Future<Output = Result<T, kafka_conn::Error>>,
) -> Result<T, error::CallError> {
    match tokio::time::timeout(CALL_TIMEOUT, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(error::CallError::Kafka(error)),
        Err(_) => Err(error::CallError::TimedOut {
            what: what.to_owned(),
            after: CALL_TIMEOUT,
        }),
    }
}

/// The whole application router.
///
/// `/health` sits outside `/api` because a liveness probe is not part of the
/// data surface and must never gain a dependency on one.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health::health))
        .nest("/api", api_router())
        .with_state(state)
}

fn api_router() -> Router<AppState> {
    use routes::{capabilities, clusters, configs, groups, messages, spec, topics};

    Router::new()
        // The document that describes everything below it, including itself.
        .route("/openapi.json", get(spec::spec))
        .route("/clusters", get(clusters::list))
        .route("/clusters/{id}", get(clusters::detail))
        .route(
            "/clusters/{id}/capabilities",
            get(capabilities::capabilities),
        )
        .route("/clusters/{id}/brokers", get(clusters::brokers))
        .route(
            "/clusters/{id}/brokers/{node}/log-dirs",
            get(clusters::log_dirs),
        )
        .route("/clusters/{id}/configs", get(configs::cluster_configs))
        .route("/clusters/{id}/topics", get(topics::list))
        .route("/clusters/{id}/topics/{topic}", get(topics::detail))
        .route(
            "/clusters/{id}/topics/{topic}/configs",
            get(configs::topic_configs),
        )
        .route(
            "/clusters/{id}/topics/{topic}/offsets",
            get(topics::offsets),
        )
        .route(
            "/clusters/{id}/topics/{topic}/messages/tail",
            get(messages::tail),
        )
        .route("/clusters/{id}/groups", get(groups::list))
        .route("/clusters/{id}/groups/{group}", get(groups::detail))
        .route(
            "/clusters/{id}/groups/{group}/offsets",
            get(groups::offsets),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaas_ui_core::Config;

    fn state() -> AppState {
        let config = Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
"#,
        )
        .unwrap();
        AppState::new(Arc::new(ArcSwap::from_pointee(Registry::from_config(
            &config,
        ))))
    }

    #[test]
    fn an_invisible_cluster_is_not_found_rather_than_forbidden() {
        let error = state().cluster("secret").unwrap_err();
        assert_eq!(error.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn an_unconnected_cluster_is_unavailable_not_a_bad_gateway() {
        // Nothing was asked of a broker, so this is not 502: the process
        // simply has not finished connecting, and the card says so.
        let error = state().connected("kaas").unwrap_err();
        assert_eq!(error.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }
}
