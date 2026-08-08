//! The fleet view, cluster detail, brokers and log dirs.

use axum::Json;
use axum::extract::{Path, State};
use kaas_ui_core::dto::{
    Broker, ClusterCard, ClusterDescriptionDto, ClusterDetail, EnvironmentSection, LogDirDto,
};
use kaas_ui_core::envelope::Envelope;
use kaas_ui_core::health::ClusterStatus;
use kaas_ui_core::registry::Registry;

use kaas_ui_auth::{Access, Action, Resource};

use crate::{ApiError, ApiResult, AppState, Caller, call};

/// `GET /api/clusters`
///
/// One card per configured cluster, reachable or not.
///
/// **Reads `Cluster::snapshot()` and nothing else**, so it never awaits: the
/// snapshot sits behind an `ArcSwap` and carries brokers, controller id,
/// cluster id and every partition's replicas, ISR and offline set. An
/// unreachable cluster costs a timeout on its own background task, not on this
/// request — which is the property the dead-cluster fixture exists to prove.
#[utoipa::path(
    get,
    path = "/api/clusters",
    responses((status = 200, description = "Every configured cluster", body = Envelope<ClusterCard>)),
    tag = "clusters",
)]
pub async fn list(State(state): State<AppState>, caller: Caller) -> Json<Envelope<ClusterCard>> {
    let registry = state.registry();
    Json(Envelope::new(cards(&registry, caller.access())))
}

/// One card per visible cluster, nudging the unreachable ones to retry.
///
/// `visible`, not `all`. The fleet is the caller's fleet: someone in no
/// matching role gets an empty list, which is a true answer about what they
/// may see rather than an error about who they are.
fn cards(registry: &Registry, who: &Access) -> Vec<ClusterCard> {
    registry
        .visible(who)
        .map(|handle| {
            let card = ClusterCard::of(handle, who);
            if card.status != ClusterStatus::Ready {
                // Someone is looking at this cluster, so try again now rather
                // than at the end of the backoff. A side effect of a GET, not
                // a route that mutates anything.
                handle.request_retry();
            }
            card
        })
        .collect()
}

/// `GET /api/environments`
///
/// The fleet: one section per environment, each holding its clusters, the
/// schema registries beside them and whatever inventory was configured there.
///
/// Sectioned here rather than in the browser because the order is
/// configuration. Environments come in declared order, and no client can
/// recover "dev, staging, prod" from three strings that sort the other way. An
/// environment nothing visible lives in is absent from the response, so a
/// heading never reports the existence of a cluster the caller may not see —
/// which is also what makes every URL beneath it unprobeable.
#[utoipa::path(
    get,
    path = "/api/environments",
    responses((status = 200, description = "The fleet, by environment", body = Envelope<EnvironmentSection>)),
    tag = "environments",
)]
pub async fn fleet(
    State(state): State<AppState>,
    caller: Caller,
) -> Json<Envelope<EnvironmentSection>> {
    let registry = state.registry();
    let sections = EnvironmentSection::arrange(
        cards(&registry, caller.access()),
        &registry,
        caller.access(),
    );
    Json(Envelope::new(sections))
}

/// `GET /api/environments/{env}`
///
/// One section, for a page that landed on an environment directly rather than
/// through the fleet. Same shape as one element of the list above, so a client
/// that has either can render the same thing.
#[utoipa::path(
    get,
    path = "/api/environments/{env}",
    params(("env" = String, Path, description = "Environment id")),
    responses(
        (status = 200, description = "One environment", body = Envelope<EnvironmentSection>),
        (status = 404, description = "No such environment, or nothing in it is visible"),
    ),
    tag = "environments",
)]
pub async fn environment(
    State(state): State<AppState>,
    caller: Caller,
    Path(env): Path<String>,
) -> ApiResult<Json<Envelope<EnvironmentSection>>> {
    let entry = state.environment(&env, &caller)?;
    let registry = state.registry();
    let members: Vec<ClusterCard> = cards(&registry, caller.access())
        .into_iter()
        .filter(|card| card.environment == env)
        .collect();
    // `environment` above already refused an environment holding nothing this
    // caller can see, so the `None` arm here is unreachable rather than a
    // second policy decision — and it is written as a 404 anyway, because two
    // places that can disagree about visibility is one too many.
    let section = EnvironmentSection::of(&entry, members, &registry, caller.access())
        .ok_or_else(|| ApiError::not_found(format!("no environment {env:?}")))?;
    Ok(Json(Envelope::new(vec![section])))
}

/// `GET /api/clusters/{id}`
///
/// The card, the broker list, and `DescribeCluster` where the cluster answers
/// it. Where it does not — which is one of the two development clusters today
/// — the description is absent and the reason travels in `errors`. The page
/// renders, with a note.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}",
    params(("id" = String, Path, description = "Cluster id")),
    responses(
        (status = 200, description = "Cluster detail", body = Envelope<ClusterDetail>),
        (status = 404, description = "No such cluster"),
        (status = 503, description = "Configured but not connected"),
    ),
    tag = "clusters",
)]
pub async fn detail(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
) -> ApiResult<Json<Envelope<ClusterDetail>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(
        &id,
        &handle.labels,
        Resource::ClusterConfig,
        Action::View,
        None,
    )?;
    let snapshot = admin.cluster().snapshot();

    let mut brokers = Broker::list(&snapshot);
    let mut errors = Vec::new();
    let mut description = None;

    match call("describe_cluster", admin.describe_cluster()).await {
        Ok(described) => {
            Broker::enrich(&mut brokers, &described);
            description = Some(ClusterDescriptionDto::from(&described));
        }
        Err(error) => errors.push(error.into_resource_error("DescribeCluster")),
    }

    let detail = ClusterDetail {
        cluster: ClusterCard::of(&handle, caller.access()),
        brokers,
        description,
    };

    Ok(Json(
        Envelope::one(detail)
            .with_errors(errors)
            .with_snapshot_age(snapshot.age()),
    ))
}

/// `GET /api/clusters/{id}/brokers`
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/brokers",
    params(("id" = String, Path, description = "Cluster id")),
    responses((status = 200, description = "Brokers", body = Envelope<Broker>)),
    tag = "clusters",
)]
pub async fn brokers(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
) -> ApiResult<Json<Envelope<Broker>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(
        &id,
        &handle.labels,
        Resource::ClusterConfig,
        Action::View,
        None,
    )?;
    let snapshot = admin.cluster().snapshot();
    let mut brokers = Broker::list(&snapshot);

    let mut errors = Vec::new();
    match call("describe_cluster", admin.describe_cluster()).await {
        Ok(described) => Broker::enrich(&mut brokers, &described),
        Err(error) => errors.push(error.into_resource_error("DescribeCluster")),
    }

    Ok(Json(
        Envelope::new(brokers)
            .with_errors(errors)
            .with_snapshot_age(snapshot.age()),
    ))
}

/// `GET /api/clusters/{id}/brokers/{node}/log-dirs`
///
/// One of the four RPCs that go to a *specific* broker. A broker that is down
/// yields an error for that node, not for the call — which is why this is
/// per-node rather than a fan-out that one dead broker can blank.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/brokers/{node}/log-dirs",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
        ("node" = i32, Path, description = "Broker node id"),
    ),
    responses((status = 200, description = "Log directories", body = Envelope<LogDirDto>)),
    tag = "clusters",
)]
pub async fn log_dirs(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, node)): Path<(String, String, i32)>,
) -> ApiResult<Json<Envelope<LogDirDto>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(
        &id,
        &handle.labels,
        Resource::ClusterConfig,
        Action::View,
        None,
    )?;
    let dirs = call("describe_log_dirs", admin.describe_log_dirs(node)).await?;
    Ok(Json(Envelope::new(
        dirs.iter().map(LogDirDto::from).collect(),
    )))
}
