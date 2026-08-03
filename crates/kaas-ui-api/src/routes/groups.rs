//! Consumer groups: four kinds, committed offsets, and lag.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use kaas_ui_core::dto::{GroupDetail, GroupOffset, GroupSummary, group_offset};
use kaas_ui_core::envelope::Envelope;
use kafka_admin::types::oks;
use kafka_admin::{Admin, OffsetSpec};

use kaas_ui_auth::{Action, Resource};

use crate::{ApiResult, AppState, Caller, call};

/// `GET /api/clusters/{id}/groups`
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/groups",
    params(("id" = String, Path, description = "Cluster id")),
    responses((status = 200, description = "Groups", body = Envelope<GroupSummary>)),
    tag = "groups",
)]
pub async fn list(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Envelope<GroupSummary>>> {
    let (handle, admin) = state.connected(&id, &caller)?;
    caller.require(&id, &handle.labels, Resource::Consumer, Action::View, None)?;
    let listings = call("list_groups", admin.list_groups()).await?;

    let mut groups: Vec<GroupSummary> = listings.iter().map(GroupSummary::from).collect();
    groups.sort_by(|a, b| a.group_id.cmp(&b.group_id));

    Ok(Json(Envelope::new(groups)))
}

/// `GET /api/clusters/{id}/groups/{group}`
///
/// Four kinds, not one struct with optional fields. `Unrecognized` is a
/// *successful* description of an undescribable group — it exists, it is
/// listed, and the UI can say what it is rather than showing a spinner that
/// never resolves.
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/groups/{group}",
    params(
        ("id" = String, Path, description = "Cluster id"),
        ("group" = String, Path, description = "Group id"),
    ),
    responses((status = 200, description = "Group detail", body = Envelope<GroupDetail>)),
    tag = "groups",
)]
pub async fn detail(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, group)): Path<(String, String)>,
) -> ApiResult<Json<Envelope<GroupDetail>>> {
    let (handle, admin) = state.connected(&id, &caller)?;
    caller.require(&id, &handle.labels, Resource::Consumer, Action::View, None)?;
    let described = call("describe_groups", admin.describe_groups([group])).await?;
    Ok(Json(Envelope::from_per_item(
        described,
        Clone::clone,
        |_, description| GroupDetail::from(&description),
    )))
}

/// `GET /api/clusters/{id}/groups/{group}/offsets`
///
/// Committed offsets joined to the log ends, so lag can be classified rather
/// than subtracted. "No commit yet", "empty partition", "caught up" and
/// "behind by n" are four different answers and rendering them all as `0` is
/// how a lag column becomes something nobody trusts.
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/groups/{group}/offsets",
    params(
        ("id" = String, Path, description = "Cluster id"),
        ("group" = String, Path, description = "Group id"),
    ),
    responses((status = 200, description = "Committed offsets and lag", body = Envelope<GroupOffset>)),
    tag = "groups",
)]
pub async fn offsets(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, group)): Path<(String, String)>,
) -> ApiResult<Json<Envelope<GroupOffset>>> {
    let (handle, admin) = state.connected(&id, &caller)?;
    caller.require(&id, &handle.labels, Resource::Consumer, Action::View, None)?;

    let committed = call("fetch_offsets", admin.fetch_offsets(&group, None)).await?;

    let partitions: Vec<(String, i32)> = committed
        .iter()
        .map(|((topic, partition), _)| (topic.clone(), *partition))
        .collect();

    let (latest, earliest) = ends(&admin, &partitions).await;

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for ((topic, partition), outcome) in &committed {
        let key = (topic.clone(), *partition);
        match outcome {
            Ok(offset) => rows.push(group_offset(
                topic.clone(),
                *partition,
                Some(offset),
                earliest.get(&key).copied().flatten(),
                latest.get(&key).copied().flatten(),
            )),
            Err(error) => errors.push(kaas_ui_core::ResourceError::new(
                format!("{topic}-{partition}"),
                error,
            )),
        }
    }
    rows.sort_by(|a, b| (&a.topic, a.partition).cmp(&(&b.topic, b.partition)));

    Ok(Json(Envelope::new(rows).with_errors(errors)))
}

/// The log ends for a set of partitions, as two maps.
///
/// Both ends, because "caught up" and "the partition is empty" are only
/// distinguishable with the earliest offset as well as the latest. A failure
/// here yields an absent end rather than a failed request: lag becomes
/// `unknown`, which is honest, instead of `0`, which is not.
async fn ends(
    admin: &Admin,
    partitions: &[(String, i32)],
) -> (
    BTreeMap<(String, i32), Option<i64>>,
    BTreeMap<(String, i32), Option<i64>>,
) {
    let mut latest = BTreeMap::new();
    let mut earliest = BTreeMap::new();
    if partitions.is_empty() {
        return (latest, earliest);
    }

    if let Ok(listed) = call(
        "list_offsets(latest)",
        admin.list_offsets(partitions.to_vec(), OffsetSpec::Latest),
    )
    .await
    {
        for (key, offset) in oks(&listed) {
            latest.insert(key.clone(), offset.offset);
        }
    }

    if let Ok(listed) = call(
        "list_offsets(earliest)",
        admin.list_offsets(partitions.to_vec(), OffsetSpec::Earliest),
    )
    .await
    {
        for (key, offset) in oks(&listed) {
            earliest.insert(key.clone(), offset.offset);
        }
    }

    (latest, earliest)
}
