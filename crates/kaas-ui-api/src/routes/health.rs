//! Liveness.

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;

use crate::AppState;

/// The liveness answer.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    /// Always `"ok"` — reaching the handler is the whole check.
    pub status: &'static str,
    /// The running version, so a deployment can be identified without exec'ing
    /// into the pod.
    pub version: &'static str,
    /// `"open"` when no roles are configured, `"enforcing"` when they are.
    ///
    /// Here for the same reason the version is: a deployment's security
    /// posture should be answerable from outside the pod. It says nothing a
    /// visitor cannot already tell by loading the page and not being asked to
    /// sign in.
    pub auth: &'static str,
}

/// `GET /health`
///
/// **Must not consult a cluster.** A liveness probe that fails because
/// someone's broker is down is a liveness probe that restarts a healthy
/// process — and with a dozen clusters configured, it restarts it constantly.
/// This handler touches nothing but the constant below, which is the point.
#[utoipa::path(
    get,
    path = "/health",
    responses((status = 200, description = "The process is alive", body = Health)),
    tag = "health",
)]
pub async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        // The policy, which is in memory. Still no cluster.
        auth: if state.policy().is_enforcing() {
            "enforcing"
        } else {
            "open"
        },
    })
}
