//! Liveness.

use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

/// The liveness answer.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    /// Always `"ok"` — reaching the handler is the whole check.
    pub status: &'static str,
    /// The running version, so a deployment can be identified without exec'ing
    /// into the pod.
    pub version: &'static str,
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
pub async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}
