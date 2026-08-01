//! Topic list and detail.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use kaas_ui_core::dto::{Partition, TopicDetail, TopicSummary};
use kaas_ui_core::envelope::Envelope;
use kafka_admin::OffsetSpec;
use kafka_admin::types::oks;
use serde::Deserialize;

use crate::routes::split_list;
use crate::{ApiError, ApiResult, AppState, call};

/// The list query. Filtering, sorting and paging all happen here rather than
/// in the browser: a five-thousand-topic cluster is a real number, and sending
/// five thousand rows so JavaScript can hide most of them is how a UI becomes
/// unusable on exactly the cluster that needed one.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicQuery {
    /// Case-insensitive substring match on the name.
    pub search: Option<String>,
    /// Include internal topics. Off by default — `__consumer_offsets` and its
    /// friends are never parsed by kaas-ui, only listed.
    #[serde(default)]
    pub internal: bool,
    /// `name`, `partitions`, `size`, `underReplicated`.
    pub sort: Option<String>,
    /// `asc` or `desc`.
    pub order: Option<String>,
    /// Page size.
    pub limit: Option<usize>,
    /// Page offset.
    pub offset: Option<usize>,
    /// Fetch per-topic sizes. A `DescribeLogDirs` fan-out, so it is opt-in
    /// rather than on the critical path for rendering a list.
    #[serde(default)]
    pub sizes: bool,
    /// Describe exactly these topics, comma-separated, instead of listing.
    ///
    /// This is the path where the envelope earns its keep: naming fifty topics
    /// of which two do not exist is `200 OK` with forty-eight items and two
    /// errors.
    pub name: Option<String>,
}

/// `GET /api/clusters/{id}/topics`
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/topics",
    params(
        ("id" = String, Path, description = "Cluster id"),
        ("search" = Option<String>, Query, description = "Substring match"),
        ("internal" = Option<bool>, Query, description = "Include internal topics"),
        ("sort" = Option<String>, Query, description = "name | partitions | size | underReplicated"),
        ("order" = Option<String>, Query, description = "asc | desc"),
        ("limit" = Option<usize>, Query, description = "Page size"),
        ("offset" = Option<usize>, Query, description = "Page offset"),
        ("sizes" = Option<bool>, Query, description = "Fetch per-topic sizes"),
        ("name" = Option<String>, Query, description = "Describe these topics instead of listing"),
    ),
    responses((status = 200, description = "Topics", body = Envelope<TopicSummary>)),
    tag = "topics",
)]
pub async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TopicQuery>,
) -> ApiResult<Json<Envelope<TopicSummary>>> {
    let (_, admin) = state.connected(&id)?;

    if let Some(names) = query.name.as_deref() {
        let names = split_list(names);
        if names.is_empty() {
            return Err(ApiError::bad_request("?name= was empty"));
        }
        let described = call("describe_topics", admin.describe_topics(names)).await?;
        let envelope =
            Envelope::from_per_item(described, Clone::clone, |_, info| TopicSummary::of(&info));
        return Ok(Json(envelope));
    }

    let snapshot = admin.cluster().snapshot();
    let mut topics: Vec<TopicSummary> = snapshot
        .topics()
        .iter()
        .filter(|topic| query.internal || !topic.internal)
        .filter(|topic| match &query.search {
            Some(needle) => topic
                .name
                .to_lowercase()
                .contains(&needle.trim().to_lowercase()),
            None => true,
        })
        .map(TopicSummary::of)
        .collect();

    let sort = query.sort.as_deref().unwrap_or("name");
    let mut errors = Vec::new();

    // Sorting by size needs the sizes, whether or not they were asked for.
    if query.sizes || sort == "size" {
        match call("topic_sizes", admin.topic_sizes()).await {
            Ok(sizes) => {
                for topic in &mut topics {
                    if let Some((_, size)) = oks(&sizes).find(|(name, _)| *name == &topic.name) {
                        *topic = topic.clone().with_size(size);
                    }
                }
                for (name, error) in kafka_admin::types::errs(&sizes) {
                    errors.push(kaas_ui_core::ResourceError::new(name, error));
                }
            }
            Err(error) => errors.push(error.into_resource_error("DescribeLogDirs")),
        }
    }

    match sort {
        "partitions" => topics.sort_by_key(|topic| topic.partition_count),
        "size" => topics.sort_by_key(|topic| topic.replicated_bytes.unwrap_or(0)),
        "underReplicated" => {
            topics.sort_by_key(|topic| topic.under_replicated_partition_count);
        }
        "name" => topics.sort_by(|a, b| a.name.cmp(&b.name)),
        other => {
            return Err(ApiError::bad_request(format!(
                "unknown sort {other:?}: expected name, partitions, size or underReplicated"
            )));
        }
    }
    if query.order.as_deref() == Some("desc") {
        topics.reverse();
    }

    let total = topics.len();
    let offset = query.offset.unwrap_or(0).min(total);
    let limit = query.limit.unwrap_or(total);
    let page: Vec<TopicSummary> = topics.into_iter().skip(offset).take(limit).collect();

    Ok(Json(
        Envelope::new(page)
            .with_errors(errors)
            .with_total(total)
            .with_snapshot_age(snapshot.age()),
    ))
}

/// `GET /api/clusters/{id}/topics/{topic}`
///
/// `describe_topics` prefers `DescribeTopicPartitions` and falls back to
/// `Metadata` where the newer call is unreachable. Both branches are live
/// against the two development clusters, and **there is no branch here** —
/// which is the claim that a partially-implemented broker is indistinguishable
/// from an old one, made good.
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/topics/{topic}",
    params(
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
        ("offsets" = Option<bool>, Query, description = "Also fetch the offset range"),
    ),
    responses((status = 200, description = "Topic detail", body = Envelope<TopicDetail>)),
    tag = "topics",
)]
pub async fn detail(
    State(state): State<AppState>,
    Path((id, topic)): Path<(String, String)>,
    Query(query): Query<DetailQuery>,
) -> ApiResult<Json<Envelope<TopicDetail>>> {
    let (_, admin) = state.connected(&id)?;

    let described = call("describe_topics", admin.describe_topics([topic.clone()])).await?;
    let mut envelope =
        Envelope::from_per_item(described, Clone::clone, |_, info| TopicDetail::of(&info));

    // Offsets are a separate call on purpose, and an explicit partition list
    // rather than `topic_offset_range`: that helper refreshes metadata first,
    // so calling it per row of a five-hundred-topic list would be five hundred
    // metadata refreshes.
    if query.offsets.unwrap_or(true) {
        let keys: Vec<(String, i32)> = envelope
            .items
            .iter()
            .flat_map(|detail| detail.partitions.iter())
            .map(|partition| (topic.clone(), partition.partition))
            .collect();

        let (latest, earliest, errors) = offset_ends(&admin, &keys).await;
        envelope.errors.extend(errors);

        for detail in &mut envelope.items {
            for partition in &mut detail.partitions {
                let key = (topic.clone(), partition.partition);
                partition.set_offsets(
                    earliest.get(&key).copied().flatten(),
                    latest.get(&key).copied().flatten(),
                );
            }
        }
    }

    Ok(Json(envelope))
}

/// Whether the detail page should also fetch offsets.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailQuery {
    /// Defaults to true.
    pub offsets: Option<bool>,
}

/// `GET /api/clusters/{id}/topics/{topic}/offsets`
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/topics/{topic}/offsets",
    params(
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
    ),
    responses((status = 200, description = "Offset ranges", body = Envelope<Partition>)),
    tag = "topics",
)]
pub async fn offsets(
    State(state): State<AppState>,
    Path((id, topic)): Path<(String, String)>,
) -> ApiResult<Json<Envelope<PartitionOffsets>>> {
    let (_, admin) = state.connected(&id)?;

    let snapshot = admin.cluster().snapshot();
    let info = snapshot
        .topic(&topic)
        .ok_or_else(|| ApiError::not_found(format!("no topic {topic:?} on cluster {id:?}")))?;
    let keys: Vec<(String, i32)> = info
        .partitions
        .iter()
        .map(|partition| (topic.clone(), partition.partition))
        .collect();

    let (latest, earliest, errors) = offset_ends(&admin, &keys).await;

    let items = keys
        .iter()
        .map(|key| {
            let earliest_offset = earliest.get(key).copied().flatten();
            let latest_offset = latest.get(key).copied().flatten();
            PartitionOffsets {
                partition: key.1,
                earliest_offset,
                latest_offset,
                records: match (earliest_offset, latest_offset) {
                    (Some(low), Some(high)) => high.checked_sub(low),
                    _ => None,
                },
            }
        })
        .collect();

    Ok(Json(
        Envelope::new(items)
            .with_errors(errors)
            .with_snapshot_age(snapshot.age()),
    ))
}

/// Both ends of a set of partitions, and whatever failed getting them.
///
/// Both, because "this partition holds offsets X through Y" needs two numbers,
/// and because an empty partition is only distinguishable from a caught-up one
/// when you have the low end as well.
async fn offset_ends(
    admin: &kafka_admin::Admin,
    keys: &[(String, i32)],
) -> (
    BTreeMap<(String, i32), Option<i64>>,
    BTreeMap<(String, i32), Option<i64>>,
    Vec<kaas_ui_core::ResourceError>,
) {
    let mut latest = BTreeMap::new();
    let mut earliest = BTreeMap::new();
    let mut errors = Vec::new();
    if keys.is_empty() {
        return (latest, earliest, errors);
    }

    for (spec, sink) in [
        (OffsetSpec::Latest, &mut latest),
        (OffsetSpec::Earliest, &mut earliest),
    ] {
        match call("list_offsets", admin.list_offsets(keys.to_vec(), spec)).await {
            Ok(listed) => {
                for (key, outcome) in listed {
                    match outcome {
                        Ok(offset) => {
                            sink.insert(key, offset.offset);
                        }
                        Err(error) => errors.push(kaas_ui_core::ResourceError::new(
                            format!("{}-{}", key.0, key.1),
                            &error,
                        )),
                    }
                }
            }
            Err(error) => errors.push(error.into_resource_error("ListOffsets")),
        }
    }

    (latest, earliest, errors)
}

/// One partition's offset range.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartitionOffsets {
    /// Partition index.
    pub partition: i32,
    /// The first offset still retained.
    pub earliest_offset: Option<i64>,
    /// The next offset to be written.
    pub latest_offset: Option<i64>,
    /// How many records are between them.
    pub records: Option<i64>,
}
