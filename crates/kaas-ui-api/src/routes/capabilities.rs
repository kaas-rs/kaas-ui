//! The capability projection, and the broker it came from.

use axum::Json;
use axum::extract::{Path, Query, State};
use kaas_ui_core::capabilities::{Capabilities, CapabilitySource, project};
use serde::Deserialize;

use kaas_ui_auth::{Action, Resource};

use crate::{ApiError, ApiResult, AppState, Caller, call};

/// Which broker to read the version table from.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityQuery {
    /// Node id. Defaults to the lowest in the snapshot.
    pub broker: Option<i32>,
}

/// `GET /api/clusters/{id}/capabilities`
///
/// kaas-lib's version table is **per connection**, deliberately: brokers
/// mid-rolling-upgrade genuinely disagree, and a cluster-wide table would be
/// wrong during exactly the window when being right matters. There is no
/// `cluster.capabilities()` to project from, and fabricating one by using
/// whichever connection answered gives a UI whose tabs flicker.
///
/// So the table is read from an **explicitly named** broker — the lowest node
/// id unless `?broker=` says otherwise — and `source` says which. The UI
/// renders that as "as reported by broker 1".
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/capabilities",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
        ("broker" = Option<i32>, Query, description = "Read the table from this broker"),
    ),
    responses(
        (status = 200, description = "What this cluster can be asked", body = Capabilities),
        (status = 404, description = "No such cluster"),
        (status = 503, description = "Configured but not connected"),
    ),
    tag = "capabilities",
)]
pub async fn capabilities(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
    Query(query): Query<CapabilityQuery>,
) -> ApiResult<Json<Capabilities>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(
        &id,
        &handle.labels,
        Resource::ClusterConfig,
        Action::View,
        None,
    )?;
    let snapshot = admin.cluster().snapshot();

    let node_id = match query.broker {
        Some(requested) => {
            if snapshot.broker(requested).is_none() {
                return Err(ApiError::not_found(format!(
                    "cluster {id:?} has no broker {requested}"
                )));
            }
            Some(requested)
        }
        // Lowest node id: deterministic, so two loads of the page agree, and
        // named in the answer, so a surprising tab set can be traced.
        None => snapshot.brokers().iter().map(|b| b.node_id).min(),
    };

    let pool = admin.cluster().pool();
    let connection = match node_id {
        Some(node) => call("connect to broker", pool.get(node)).await?,
        // No snapshot yet: fall back to any connection and report what
        // answered rather than claiming a broker we did not choose.
        None => call("connect to any broker", pool.any()).await?,
    };

    let source = CapabilitySource::Broker {
        node_id: connection.node_id(),
        peer: connection.peer().to_owned(),
    };

    Ok(Json(project(connection.versions(), source)))
}
