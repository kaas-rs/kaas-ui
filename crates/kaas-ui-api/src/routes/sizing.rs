//! The sizing advisor, config-lint half.
//!
//! Two describes and some arithmetic: `DescribeConfigs` for what the topic
//! says about itself, `DescribeLogDirs` for what its partitions actually
//! hold. No payload is read, so this route is cheaper than the statistics tab
//! and needs none of its grants — which is also why the advisor is its own tab
//! rather than a card inside that one.
//!
//! The rules live in [`kaas_ui_core::sizing`], cluster-free and unit-tested.
//! This file is the plumbing: fetch, join, and turn each failure into an
//! envelope error rather than a dead page.

use axum::Json;
use axum::extract::{Path, Query, State};
use kaas_ui_core::envelope::Envelope;
use kaas_ui_core::sizing::SizingReport;
use kafka_admin::ConfigResource;
use kafka_admin::types::{errs, oks};
use serde::Deserialize;

use kaas_ui_auth::{Action, Resource};

use crate::{ApiError, ApiResult, AppState, Caller, call};

/// What the report may skip.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizingQuery {
    /// Whether to read partition sizes. Defaults to **true**, unlike the
    /// topic detail's `?size=`.
    ///
    /// There the page paints from the describe and the fan-out arrives into
    /// it; here the sizes are half the answer — without them every diagnostic
    /// is a suspicion rather than a finding. A caller who wants the cheap
    /// version asks for it.
    pub size: Option<bool>,
}

/// `GET /api/environments/{env}/clusters/{id}/topics/{topic}/sizing`
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/topics/{topic}/sizing",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
        ("size" = Option<bool>, Query, description = "Read partition sizes. Defaults to true"),
    ),
    responses((status = 200, description = "Sizing report", body = Envelope<SizingReport>)),
    tag = "topics",
)]
pub async fn sizing(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, topic)): Path<(String, String, String)>,
    Query(query): Query<SizingQuery>,
) -> ApiResult<Json<Envelope<SizingReport>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    // The same grant the configs tab needs, because this is that data read
    // differently. Deliberately *not* stricter: a report the configs tab can
    // already show the inputs of must not 403.
    caller.require(
        &id,
        &handle.labels,
        Resource::ClusterConfig,
        Action::View,
        None,
    )?;

    let snapshot = admin.cluster().snapshot();
    let info = snapshot
        .topic(&topic)
        .ok_or_else(|| ApiError::not_found(format!("no topic {topic:?} on cluster {id:?}")))?;
    let partitions = info.partitions.len();

    let mut errors = Vec::new();

    // The configuration. A failure here leaves the report with no settings and
    // no diagnostics rather than no report: the partition count and the error
    // are still worth rendering.
    let entries = match call(
        "describe_configs",
        admin.describe_configs_documented(vec![ConfigResource::topic(topic.clone())]),
    )
    .await
    {
        Ok(described) => {
            for (resource, error) in errs(&described) {
                errors.push(kaas_ui_core::ResourceError::new(&resource.name, error));
            }
            oks(&described)
                .flat_map(|(_, found)| found.iter().map(kaas_ui_core::dto::ConfigEntryDto::from))
                .collect()
        }
        Err(error) => {
            errors.push(error.into_resource_error("DescribeConfigs"));
            Vec::new()
        }
    };

    // The sizes, leader copies only: `segment.bytes` counts bytes in one log,
    // so a comparison against the replicated figure is wrong by the
    // replication factor. kaas-lib has already done that filtering.
    let leader_bytes = if query.size.unwrap_or(true) {
        match call("topic_sizes", admin.topic_sizes()).await {
            Ok(sizes) => {
                for (name, error) in errs(&sizes) {
                    if name == &topic {
                        errors.push(kaas_ui_core::ResourceError::new(name, error));
                    }
                }
                oks(&sizes)
                    .find(|(name, _)| *name == &topic)
                    .map(|(_, size)| {
                        size.partitions
                            .iter()
                            .map(|partition| (partition.partition, partition.logical_bytes))
                            .collect::<Vec<_>>()
                    })
            }
            Err(error) => {
                errors.push(error.into_resource_error("DescribeLogDirs"));
                None
            }
        }
    } else {
        None
    };

    let report = SizingReport::build(&topic, partitions, &entries, leader_bytes.as_deref());

    Ok(Json(
        Envelope::one(report)
            .with_errors(errors)
            .with_snapshot_age(snapshot.age()),
    ))
}
