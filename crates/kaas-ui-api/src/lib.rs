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
    /// The path prefix a reverse proxy mounts kaas-ui under. Empty at the root.
    ///
    /// The router never needs it — a stripping proxy removed the prefix before
    /// the request was routed — but a `Location` header is consumed by the
    /// *browser*, which never saw it stripped. Everything that redirects into
    /// the application builds its target from [`Self::app_root`].
    base_prefix: String,
    /// Which clusters have an analysis running, keyed `(environment, id)`.
    ///
    /// One per cluster, and the ceiling is about **everyone else's latency**
    /// rather than memory: kaas-lib keeps one connection per broker, Kafka
    /// answers a connection in order, and an analysis fetches continuously
    /// for minutes — so a second one would sit behind the first in the same
    /// queue and every `ListOffsets` behind both. See upstream ask 11; until
    /// it lands, this is the honest bound.
    analyses: Arc<std::sync::Mutex<std::collections::HashSet<(String, String)>>>,
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
            base_prefix: String::new(),
            analyses: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        }
    }

    /// Claim the one analysis slot a cluster has, or say who is in it.
    ///
    /// The slot is released by dropping the permit — the same shape as the
    /// stream governor, and for the same reason: an analysis that ends by the
    /// client vanishing runs no teardown of its own.
    pub fn begin_analysis(&self, env: &str, id: &str) -> Result<AnalysisPermit, ApiError> {
        let key = (env.to_owned(), id.to_owned());
        let mut running = self
            .analyses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !running.insert(key.clone()) {
            // 429 and not 409: nothing about the request conflicts, the
            // resource is busy and retrying later is the right response.
            return Err(ApiError::too_many_requests(format!(
                "an analysis is already running on cluster {id:?}; a full-topic read \
                 occupies the shared broker connections, so one runs at a time"
            )));
        }
        Ok(AnalysisPermit {
            analyses: Arc::clone(&self.analyses),
            key,
        })
    }

    /// Serve under a path prefix.
    ///
    /// Takes the [`ServerConfig::base_prefix`] shape — leading slash, no
    /// trailing slash, empty for the root — so there is exactly one place that
    /// normalises what an operator typed.
    ///
    /// [`ServerConfig::base_prefix`]: kaas_ui_core::config::ServerConfig::base_prefix
    #[must_use]
    pub fn with_base_prefix(mut self, prefix: String) -> Self {
        self.base_prefix = prefix;
        self
    }

    /// Where the application lives, as a redirect target.
    ///
    /// `/` on a deployment at the root; `{prefix}/` under one — the trailing
    /// slash matters, because `Location: /proxy/8099` asks code-server for a
    /// directory listing where `Location: /proxy/8099/` asks it for the app.
    pub fn app_root(&self) -> String {
        if self.base_prefix.is_empty() {
            "/".to_owned()
        } else {
            format!("{}/", self.base_prefix)
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
    pub fn cluster(&self, env: &str, id: &str, who: &Caller) -> ApiResult<Arc<ClusterHandle>> {
        self.registry()
            .get(env, id, who.access())
            .map(Arc::clone)
            // One message for "no such environment", "no such cluster in it"
            // and "not yours", because telling them apart is exactly what a
            // prober wants and the 404-not-403 rule exists to refuse.
            .ok_or_else(|| ApiError::not_found(format!("no cluster {id:?} in environment {env:?}")))
    }

    /// An environment, or `404` if this caller sees nothing in it.
    pub fn environment(
        &self,
        env: &str,
        who: &Caller,
    ) -> ApiResult<kaas_ui_core::config::EnvironmentEntry> {
        self.registry()
            .environment(env, who.access())
            .cloned()
            .ok_or_else(|| ApiError::not_found(format!("no environment {env:?}")))
    }

    /// A schema registry, or `404`.
    ///
    /// Guarded by the same lookup rule as a cluster and for the same reason —
    /// a registry is addressable now, so it needs to be unenumerable on its
    /// own rather than by being unreachable.
    pub fn schema_registry(
        &self,
        env: &str,
        id: &str,
        who: &Caller,
    ) -> ApiResult<Arc<kaas_ui_serde::RegistryHandle>> {
        self.registry()
            .schema_registry(env, id, who.access())
            .map(Arc::clone)
            .ok_or_else(|| {
                ApiError::not_found(format!("no schema registry {id:?} in environment {env:?}"))
            })
    }

    /// A connected cluster, or `503` with what the connector last saw.
    ///
    /// Never waits for a connection. A cluster that is still connecting, or
    /// that failed, answers immediately — and gets nudged to retry now rather
    /// than at the end of its backoff, which is what the card's retry button
    /// is wired to.
    pub fn connected(
        &self,
        env: &str,
        id: &str,
        who: &Caller,
    ) -> ApiResult<(Arc<ClusterHandle>, Arc<Admin>)> {
        let handle = self.cluster(env, id, who)?;
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

/// One cluster's held analysis slot. Dropping it releases the cluster.
#[derive(Debug)]
pub struct AnalysisPermit {
    analyses: Arc<std::sync::Mutex<std::collections::HashSet<(String, String)>>>,
    key: (String, String),
}

impl Drop for AnalysisPermit {
    fn drop(&mut self) {
        self.analyses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.key);
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
    use routes::{
        analysis, capabilities, clusters, configs, groups, me, messages, schemas, spec, topics,
    };

    Router::new()
        // The document that describes everything below it, including itself.
        .route("/openapi.json", get(spec::spec))
        // Who is asking. Above the clusters because it is the answer that
        // decides which of them exist for this caller.
        .route("/me", get(me::me))
        // The fleet: every environment this caller can see, and everything in
        // one. The entry point, and the only route that does not name an
        // environment — because it is what tells you which ones there are.
        .route("/environments", get(clusters::fleet))
        .route("/environments/{env}", get(clusters::environment))
        // Everything below is addressed environment-first. A cluster id alone
        // addresses nothing: two environments may each hold a `kafka`, and the
        // lookup that decides whether you may see either takes both halves.
        .route("/environments/{env}/clusters", get(clusters::list))
        .route("/environments/{env}/clusters/{id}", get(clusters::detail))
        .route(
            "/environments/{env}/clusters/{id}/capabilities",
            get(capabilities::capabilities),
        )
        .route(
            "/environments/{env}/clusters/{id}/brokers",
            get(clusters::brokers),
        )
        .route(
            "/environments/{env}/clusters/{id}/brokers/{node}/log-dirs",
            get(clusters::log_dirs),
        )
        .route(
            "/environments/{env}/clusters/{id}/configs",
            get(configs::cluster_configs),
        )
        .route(
            "/environments/{env}/clusters/{id}/topics",
            get(topics::list),
        )
        .route(
            "/environments/{env}/clusters/{id}/topics/{topic}",
            get(topics::detail),
        )
        .route(
            "/environments/{env}/clusters/{id}/topics/{topic}/configs",
            get(configs::topic_configs),
        )
        .route(
            "/environments/{env}/clusters/{id}/topics/{topic}/offsets",
            get(topics::offsets),
        )
        .route(
            "/environments/{env}/clusters/{id}/topics/{topic}/messages",
            get(messages::page),
        )
        .route(
            "/environments/{env}/clusters/{id}/topics/{topic}/messages/tail",
            get(messages::tail),
        )
        .route(
            "/environments/{env}/clusters/{id}/topics/{topic}/messages/stream",
            get(messages::stream),
        )
        // Two path parameters rather than a query, because a record's identity
        // *is* `{partition}-{offset}` — the same string the list keys on, the
        // SSE `id:` carries and the query cache is keyed by.
        .route(
            "/environments/{env}/clusters/{id}/topics/{topic}/messages/{partition}/{offset}",
            get(messages::one),
        )
        // The statistics tab: a full-topic scan folded into an aggregate,
        // over SSE. One GET — cancellation is closing the response, so the
        // no-mutating-route invariant needs no exception for it.
        .route(
            "/environments/{env}/clusters/{id}/topics/{topic}/analysis",
            get(analysis::analysis),
        )
        .route(
            "/environments/{env}/clusters/{id}/groups",
            get(groups::list),
        )
        .route(
            "/environments/{env}/clusters/{id}/groups/{group}",
            get(groups::detail),
        )
        .route(
            "/environments/{env}/clusters/{id}/groups/{group}/offsets",
            get(groups::offsets),
        )
        // A registry is a peer of a cluster inside an environment, not a
        // feature of one. It got this URL when environments did: the id is
        // scoped, and the lookup still refuses a caller who cannot see a
        // cluster here that references it.
        .route(
            "/environments/{env}/schema-registries/{registry}/subjects",
            get(schemas::list),
        )
        .route(
            "/environments/{env}/schema-registries/{registry}/subjects/{subject}/versions",
            get(schemas::versions),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaas_ui_auth::{Access, Principal, Role};
    use kaas_ui_core::Config;

    fn state(policy: Policy) -> AppState {
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: kaas
        bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
"#,
        )
        .unwrap();
        AppState::new(
            Arc::new(ArcSwap::from_pointee(
                Registry::from_config(&config).unwrap(),
            )),
            policy,
        )
    }

    /// The caller an open deployment resolves for every request.
    fn anyone() -> Caller {
        Caller::new(Principal::anonymous(), Access::admin())
    }

    #[test]
    fn a_cluster_that_is_not_configured_is_not_found() {
        let error = state(Policy::open())
            .cluster("dev", "secret", &anyone())
            .unwrap_err();
        assert_eq!(error.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn a_cluster_no_role_selects_is_not_found_rather_than_forbidden() {
        // The point of doing visibility inside the lookup: `kaas` *is*
        // configured, and this caller still gets a 404. A 403 would confirm
        // the id, and confirming ids is how a registry becomes enumerable.
        let policy = Policy::enforcing(vec![Role {
            clusters: vec!["prod-*".to_owned()],
            ..Role::admin("prod-only", vec!["someone".to_owned()])
        }]);
        let state = state(policy);
        let nobody = Caller::new(Principal::new("stranger"), Access::none());

        let error = state.cluster("dev", "kaas", &nobody).unwrap_err();
        assert_eq!(error.status(), axum::http::StatusCode::NOT_FOUND);
        // And it is there for someone who can see it.
        assert!(state.cluster("dev", "kaas", &anyone()).is_ok());
    }

    #[test]
    fn an_unconnected_cluster_is_unavailable_not_a_bad_gateway() {
        // Nothing was asked of a broker, so this is not 502: the process
        // simply has not finished connecting, and the card says so.
        let error = state(Policy::open())
            .connected("dev", "kaas", &anyone())
            .unwrap_err();
        assert_eq!(error.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }
}
