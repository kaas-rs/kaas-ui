//! The HTTP surface.
//!
//! Knows about [`kaas_ui_core`] and axum, and never opens a socket. Two rules
//! hold this crate together:
//!
//! * **Nothing here can write to a cluster.** Not because of the verbs on the
//!   routes, but because the only admin client in the workspace is built by
//!   `Admin::connect_read_only` — a handler reached by any method at all has
//!   nothing to write with. The data routes are `GET`s because reading is what
//!   they do, not because a rule forbids the alternative.
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
use axum::extract::FromRef;
use axum::routing::{get, post};
use axum_extra::extract::cookie::Key;
use kaas_ui_auth::{Audit, Policy, Provider};
use kaas_ui_core::registry::{ClusterHandle, Registry};
use kafka_admin::Admin;

pub mod auth;
pub mod error;
pub mod openapi;
pub mod routes;
pub mod session;
pub mod streaming;

pub use auth::Caller;
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
    /// Who may see what. [`Policy::open`] when nothing was configured.
    policy: Arc<Policy>,
    /// The identity provider, when one is configured.
    auth: Option<Arc<Provider>>,
    /// Encrypts the session and pending-login cookies. Generated at startup,
    /// so a restart signs everyone out — see [`session`].
    cookie_key: Key,
    /// Who read which payloads. Always present: a read-only tool's audit is
    /// its whole security story, and making it optional would make it absent.
    audit: Arc<Audit>,
    streams: Arc<streaming::StreamGovernor>,
    stopping: Arc<streaming::ShutdownSignal>,
    shutdown: streaming::Shutdown,
}

impl AppState {
    /// Wrap a registry and the policy that decides who sees it.
    pub fn new(registry: Arc<ArcSwap<Registry>>, policy: Policy) -> Self {
        let (stopping, shutdown) = streaming::shutdown_latch();
        Self {
            registry,
            policy: Arc::new(policy),
            auth: None,
            cookie_key: Key::generate(),
            audit: Arc::new(Audit::to_stdout()),
            streams: Arc::new(streaming::StreamGovernor::default()),
            stopping: Arc::new(stopping),
            shutdown,
        }
    }

    /// The current registry.
    pub fn registry(&self) -> arc_swap::Guard<Arc<Registry>> {
        self.registry.load()
    }

    /// Attach an identity provider.
    ///
    /// Absent, `/auth/*` answers 404 and every caller is anonymous — which is
    /// the development deployment and was the whole cluster until Phase 4.
    #[must_use]
    pub fn with_auth(mut self, provider: Arc<Provider>) -> Self {
        self.auth = Some(provider);
        self
    }

    /// Send the audit somewhere other than stdout. For tests.
    #[must_use]
    pub fn with_audit(mut self, audit: Arc<Audit>) -> Self {
        self.audit = audit;
        self
    }

    /// The access audit.
    pub fn audit(&self) -> &Audit {
        &self.audit
    }

    /// Record a disclosure, or fail the request that would have made it.
    ///
    /// # Errors
    ///
    /// `500` when the entry could not be written. The payload is not sent:
    /// that is what makes this an audit log rather than a log.
    pub fn record_read(&self, entry: &kaas_ui_auth::Read) -> ApiResult<()> {
        self.audit
            .record(entry)
            .map_err(|error| ApiError::audit_failed(&error.to_string()))
    }

    /// The identity provider, if there is one.
    pub fn auth(&self) -> Option<&Arc<Provider>> {
        self.auth.as_ref()
    }

    /// The authorization policy.
    ///
    /// Not behind the `ArcSwap` the registry uses: a config reload replaces
    /// clusters, and changing who may see them under a live session is a
    /// different question with different failure modes. It waits for the slice
    /// that has sessions to change.
    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// How many message streams are open.
    ///
    /// Held here rather than per-router so a configuration reload — which
    /// replaces the registry wholesale — cannot reset the count and let the
    /// ceilings be walked through by editing a file.
    pub fn streams(&self) -> &Arc<streaming::StreamGovernor> {
        &self.streams
    }

    /// The latch every open stream watches.
    pub fn shutdown(&self) -> streaming::Shutdown {
        self.shutdown.clone()
    }

    /// Tell every open stream to finish, and stay told.
    ///
    /// Called once, from the signal handler. Without it a draining server
    /// waits on SSE responses that never complete — see
    /// [`streaming::Shutdown`].
    pub fn stop_streams(&self) {
        self.stopping.stop();
    }

    /// **The only cluster lookup.**
    ///
    /// A cluster that is not configured — or, from the auth phase onward, one
    /// the caller may not see — is `404`. Not `403`: a 403 confirms the id
    /// exists, and confirming ids is how a registry becomes enumerable.
    pub fn cluster(&self, id: &str, who: &Caller) -> ApiResult<Arc<ClusterHandle>> {
        self.registry()
            .get(id, who.access())
            .map(Arc::clone)
            .ok_or_else(|| ApiError::not_found(format!("no cluster {id:?}")))
    }

    /// A connected cluster, or `503` with what the connector last saw.
    ///
    /// Never waits for a connection. A cluster that is still connecting, or
    /// that failed, answers immediately — and gets nudged to retry now rather
    /// than at the end of its backoff, which is what the card's retry button
    /// is wired to.
    pub fn connected(&self, id: &str, who: &Caller) -> ApiResult<(Arc<ClusterHandle>, Arc<Admin>)> {
        let handle = self.cluster(id, who)?;
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

/// So `PrivateCookieJar` can be extracted in a handler.
impl FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.cookie_key.clone()
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
        // Outside `/api`: these are browser navigations, not data.
        .route("/auth/login", get(routes::auth::login))
        .route("/auth/callback", get(routes::auth::callback))
        .route("/auth/logout", post(routes::auth::logout))
        .nest("/api", api_router())
        .with_state(state)
}

fn api_router() -> Router<AppState> {
    use routes::{capabilities, clusters, configs, groups, me, messages, spec, topics};

    Router::new()
        // The document that describes everything below it, including itself.
        .route("/openapi.json", get(spec::spec))
        // Who is asking. Above the clusters because it is the answer that
        // decides which of them exist for this caller.
        .route("/me", get(me::me))
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
            "/clusters/{id}/topics/{topic}/messages",
            get(messages::page),
        )
        .route(
            "/clusters/{id}/topics/{topic}/messages/tail",
            get(messages::tail),
        )
        .route(
            "/clusters/{id}/topics/{topic}/messages/stream",
            get(messages::stream),
        )
        // Two path parameters rather than a query, because a record's identity
        // *is* `{partition}-{offset}` — the same string the list keys on, the
        // SSE `id:` carries and the query cache is keyed by.
        .route(
            "/clusters/{id}/topics/{topic}/messages/{partition}/{offset}",
            get(messages::one),
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
    use kaas_ui_auth::{Access, Grant, Principal, Role};
    use kaas_ui_core::Config;

    fn state(policy: Policy) -> AppState {
        let config = Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
    labels: { env: dev }
"#,
        )
        .unwrap();
        AppState::new(
            Arc::new(ArcSwap::from_pointee(Registry::from_config(&config))),
            policy,
        )
    }

    /// The caller an open deployment resolves for every request.
    fn anyone() -> Caller {
        Caller::new(Principal::anonymous(), Access::unrestricted())
    }

    #[test]
    fn a_cluster_that_is_not_configured_is_not_found() {
        let error = state(Policy::open())
            .cluster("secret", &anyone())
            .unwrap_err();
        assert_eq!(error.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn a_cluster_no_role_selects_is_not_found_rather_than_forbidden() {
        // The point of doing visibility inside the lookup: `kaas` *is*
        // configured, and this caller still gets a 404. A 403 would confirm
        // the id, and confirming ids is how a registry becomes enumerable.
        let policy = Policy::enforcing(vec![Role {
            name: "prod-only".to_owned(),
            subjects: vec!["someone".to_owned()],
            clusters: [("env".to_owned(), "prod".to_owned())]
                .into_iter()
                .collect(),
            grants: [Grant::Metadata].into_iter().collect(),
            ..Role::default()
        }]);
        let state = state(policy);
        let nobody = Caller::new(Principal::new("stranger", None, []), Access::none());

        let error = state.cluster("kaas", &nobody).unwrap_err();
        assert_eq!(error.status(), axum::http::StatusCode::NOT_FOUND);
        // And it is there for someone who can see it.
        assert!(state.cluster("kaas", &anyone()).is_ok());
    }

    #[test]
    fn an_unconnected_cluster_is_unavailable_not_a_bad_gateway() {
        // Nothing was asked of a broker, so this is not 502: the process
        // simply has not finished connecting, and the card says so.
        let error = state(Policy::open())
            .connected("kaas", &anyone())
            .unwrap_err();
        assert_eq!(error.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }
}
