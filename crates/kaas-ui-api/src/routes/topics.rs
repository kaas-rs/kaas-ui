//! Topic list and detail.

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::Json;
use axum::extract::{Path, Query, State};
use kaas_ui_core::dto::{Partition, TopicDetail, TopicSummary};
use kaas_ui_core::envelope::Envelope;
use kafka_admin::OffsetSpec;
use kafka_admin::types::oks;
use kafka_meta::MetadataSnapshot;
use serde::Deserialize;

use crate::routes::split_list;
use kaas_ui_auth::{Action, Resource};

use crate::{ApiError, ApiResult, AppState, Caller, call};

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
    /// `name`, `partitions`, `messages`, `size`, `underReplicated`.
    pub sort: Option<String>,
    /// `asc` or `desc`.
    pub order: Option<String>,
    /// Page size.
    pub limit: Option<usize>,
    /// Page offset.
    pub offset: Option<usize>,
    /// Fetch message counts and on-disk sizes.
    ///
    /// Opt-in because it is the only thing on this route that touches a
    /// broker: everything else is served from the metadata snapshot. The
    /// client asks twice — once without, to paint the table, once with, to
    /// fill the two columns — so a slow cluster delays two numbers rather
    /// than the page.
    #[serde(default)]
    pub metrics: bool,
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
    path = "/api/environments/{env}/clusters/{id}/topics",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
        ("search" = Option<String>, Query, description = "Substring match"),
        ("internal" = Option<bool>, Query, description = "Include internal topics"),
        ("sort" = Option<String>, Query, description = "name | partitions | messages | size | underReplicated"),
        ("order" = Option<String>, Query, description = "asc | desc"),
        ("limit" = Option<usize>, Query, description = "Page size"),
        ("offset" = Option<usize>, Query, description = "Page offset"),
        ("metrics" = Option<bool>, Query, description = "Fetch message counts and sizes"),
        ("name" = Option<String>, Query, description = "Describe these topics instead of listing"),
    ),
    responses((status = 200, description = "Topics", body = Envelope<TopicSummary>)),
    tag = "topics",
)]
pub async fn list(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
    Query(query): Query<TopicQuery>,
) -> ApiResult<Json<Envelope<TopicSummary>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(&id, &handle.labels, Resource::Topic, Action::View, None)?;

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

    // Ordering by a metric needs that metric for every topic, because the page
    // is what the ordering *produces* — enriching afterwards would sort five
    // thousand rows by a column that is null on all of them. Every other sort
    // comes out of the snapshot, so the fan-out can wait until after paging
    // and pay for fifty rows instead.
    let sort_needs_metrics = matches!(sort, "size" | "messages");
    if sort_needs_metrics {
        errors.extend(enrich(&admin, &snapshot, &mut topics).await);
    }

    match sort {
        "partitions" => topics.sort_by_key(|topic| topic.partition_count),
        "size" => topics.sort_by_key(|topic| topic.replicated_bytes.unwrap_or(0)),
        "messages" => topics.sort_by_key(|topic| topic.message_count.unwrap_or(0)),
        "underReplicated" => {
            topics.sort_by_key(|topic| topic.under_replicated_partition_count);
        }
        "name" => topics.sort_by(|a, b| a.name.cmp(&b.name)),
        other => {
            return Err(ApiError::bad_request(format!(
                "unknown sort {other:?}: expected name, partitions, messages, size or \
                 underReplicated"
            )));
        }
    }
    if query.order.as_deref() == Some("desc") {
        topics.reverse();
    }

    let total = topics.len();
    let offset = query.offset.unwrap_or(0).min(total);
    let limit = query.limit.unwrap_or(total);
    let mut page: Vec<TopicSummary> = topics.into_iter().skip(offset).take(limit).collect();

    if query.metrics && !sort_needs_metrics {
        errors.extend(enrich(&admin, &snapshot, &mut page).await);
    }

    Ok(Json(
        Envelope::new(page)
            .with_errors(errors)
            .with_total(total)
            .with_snapshot_age(snapshot.age()),
    ))
}

/// Attach message counts and on-disk sizes to `topics`, in place.
///
/// Both fan-outs are bounded by the **broker** count, not by how many topics
/// are handed in: a log directory is a property of one broker's disks, so
/// `DescribeLogDirs` goes to each of them, and `list_offsets` groups its
/// partitions by leader and sends one request per leader. What the topic count
/// changes is the *payload* — which is the whole reason the caller hands this
/// a page of fifty rather than a cluster of five thousand.
///
/// Errors are scoped to the rows asked about. `DescribeLogDirs` answers for
/// every topic on the broker, and a failure on a topic forty pages away is
/// noise on a chip under the table someone is reading.
async fn enrich(
    admin: &kafka_admin::Admin,
    snapshot: &MetadataSnapshot,
    topics: &mut [TopicSummary],
) -> Vec<kaas_ui_core::ResourceError> {
    let mut errors = Vec::new();
    if topics.is_empty() {
        return errors;
    }
    let wanted: HashSet<&str> = topics.iter().map(|topic| topic.name.as_str()).collect();

    // Sizes, joined through a map. The scan this replaces ran once per row,
    // which is quadratic in the topic count and did its worst work on exactly
    // the cluster that the paging above exists for.
    match call("topic_sizes", admin.topic_sizes()).await {
        Ok(sizes) => {
            let by_name: HashMap<_, _> = oks(&sizes)
                .map(|(name, size)| (name.as_str(), size))
                .filter(|(name, _)| wanted.contains(name))
                .collect();
            for (name, error) in kafka_admin::types::errs(&sizes) {
                if wanted.contains(name.as_str()) {
                    errors.push(kaas_ui_core::ResourceError::new(name, error));
                }
            }
            for topic in topics.iter_mut() {
                if let Some(size) = by_name.get(topic.name.as_str()) {
                    topic.set_size(size);
                }
            }
        }
        Err(error) => errors.push(error.into_resource_error("DescribeLogDirs")),
    }

    // Message counts. The partition list comes from the snapshot rather than
    // from `partition_count`, because the count is a length and `list_offsets`
    // needs the indices — which are not required to be `0..count`.
    let keys: Vec<(String, i32)> = topics
        .iter()
        .filter_map(|topic| snapshot.topic(&topic.name))
        .flat_map(|info| {
            info.partitions
                .iter()
                .map(|partition| (info.name.clone(), partition.partition))
        })
        .collect();

    let (latest, earliest, offset_errors) = offset_ends(admin, &keys).await;
    errors.extend(offset_errors);

    // One pass, accumulating per topic. A partition that answered neither end
    // marks its topic incomplete rather than contributing nothing: a sum with
    // a partition missing is a smaller number, not a marked one, and the
    // column has no way to say "this is short by one partition".
    let mut summed: HashMap<&str, (i64, bool)> = HashMap::new();
    for key in &keys {
        let entry = summed.entry(key.0.as_str()).or_insert((0, true));
        match (
            earliest.get(key).copied().flatten(),
            latest.get(key).copied().flatten(),
        ) {
            (Some(low), Some(high)) => {
                entry.0 = entry.0.saturating_add(high.saturating_sub(low));
            }
            _ => entry.1 = false,
        }
    }
    for topic in topics.iter_mut() {
        if let Some((records, complete)) = summed.get(topic.name.as_str())
            && *complete
        {
            topic.set_message_count(*records);
        }
    }

    errors
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
    path = "/api/environments/{env}/clusters/{id}/topics/{topic}",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
        ("offsets" = Option<bool>, Query, description = "Also fetch the offset range"),
    ),
    responses((status = 200, description = "Topic detail", body = Envelope<TopicDetail>)),
    tag = "topics",
)]
pub async fn detail(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, topic)): Path<(String, String, String)>,
    Query(query): Query<DetailQuery>,
) -> ApiResult<Json<Envelope<TopicDetail>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(&id, &handle.labels, Resource::Topic, Action::View, None)?;

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
        envelope = envelope.with_errors(errors);

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
    path = "/api/environments/{env}/clusters/{id}/topics/{topic}/offsets",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
    ),
    responses((status = 200, description = "Offset ranges", body = Envelope<Partition>)),
    tag = "topics",
)]
pub async fn offsets(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, topic)): Path<(String, String, String)>,
) -> ApiResult<Json<Envelope<PartitionOffsets>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(&id, &handle.labels, Resource::Topic, Action::View, None)?;

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
