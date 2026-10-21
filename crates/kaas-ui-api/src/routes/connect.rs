//! Kafka Connect.
//!
//! Rooted at `/api/environments/{env}/connect-clusters/{connect}`, a peer of
//! clusters and schema registries inside an environment. The lookup —
//! [`AppState::connect_cluster`] — answers only for a caller who may view
//! topics on the Kafka cluster the Connect cluster is attached to, so
//! connectors cannot be used to learn which topics flow where on a cluster
//! the caller cannot see.
//!
//! Reads, here, go through a [`kaas_ui_connect::ConnectClient`], which has no
//! method that sends anything but `GET`. Writes are in
//! [`super::connect_write`]; the pages here carry what that module says a
//! caller may do, so a button is drawn only where the gate would agree.
//!
//! What is **not** an error here: a worker without the `/offsets` or
//! `/topics` endpoint. That is absence — the field is empty and says why —
//! and the page renders around it.

use axum::Json;
use axum::extract::{Path, Query, State};
use futures::StreamExt;
use kaas_ui_auth::{Action, Resource};
use kaas_ui_connect::{
    ConnectFault, ConnectorDetail, ConnectorStatus, ConnectorSummary, ConnectorType, Plugin,
    StoredOffset,
};
use kaas_ui_core::dto::ConnectClusterCard;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::connect_write::{self, WriteOption};
use crate::error::connect_resource_error;
use crate::{ApiResult, AppState, Caller};

/// One Connect cluster's page header.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectClusterDetail {
    /// Id, name, url, health, and what `GET /` said.
    pub card: ConnectClusterCard,
    /// Whether the workers use the Kafka cluster the config says they do.
    pub kafka_cluster_check: KafkaClusterCheck,
    /// How many connectors are in each state, where the list could be read.
    pub counts: Option<ConnectorCounts>,
    /// How many connector plugins are installed, where that could be read.
    pub plugin_count: Option<usize>,
    /// What could not be read, by name. The header renders without it.
    pub errors: Vec<kaas_ui_core::ResourceError>,
    /// Creating a connector here, where the ceiling lists it: whether this
    /// caller may, and why not.
    pub create: Option<WriteOption>,
}

/// The `kafka_cluster:` reference, checked against the workers.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KafkaClusterCheck {
    /// The verdict.
    pub state: KafkaClusterCheckState,
    /// The Kafka cluster id kaas-ui sees for the referenced cluster.
    pub expected: Option<String>,
    /// The Kafka cluster id the workers report.
    pub actual: Option<String>,
}

/// The verdict on a `kafka_cluster:` reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KafkaClusterCheckState {
    /// Both ids are known and equal.
    Verified,
    /// Both ids are known and differ: the reference is wrong, or the workers
    /// are pointed somewhere unexpected. Shown as a banner.
    Mismatch,
    /// One of the two ids is not known yet — the Kafka cluster is not
    /// connected, or the workers were not reached.
    Unverified,
    /// The entry names no Kafka cluster, so there is nothing to check.
    NotConfigured,
}

/// Connectors by derived state.
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorCounts {
    /// Every connector.
    pub total: usize,
    /// Running, every task running.
    pub running: usize,
    /// Running with some failed tasks.
    pub degraded: usize,
    /// Failed outright.
    pub failed: usize,
    /// Paused on purpose.
    pub paused: usize,
    /// Stopped on purpose.
    pub stopped: usize,
    /// Anything else: unassigned, restarting, no tasks, unknown.
    pub other: usize,
}

impl ConnectorCounts {
    fn of(connectors: &[ConnectorSummary]) -> Self {
        let mut counts = Self {
            total: connectors.len(),
            ..Self::default()
        };
        for connector in connectors {
            match connector.status {
                ConnectorStatus::Running => counts.running += 1,
                ConnectorStatus::Degraded => counts.degraded += 1,
                ConnectorStatus::Failed => counts.failed += 1,
                ConnectorStatus::Paused => counts.paused += 1,
                ConnectorStatus::Stopped => counts.stopped += 1,
                _ => counts.other += 1,
            }
        }
        counts
    }
}

/// `GET /api/environments/{env}/connect-clusters/{connect}`
#[utoipa::path(
    get,
    path = "/api/environments/{env}/connect-clusters/{connect}",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("connect" = String, Path, description = "Connect cluster id"),
    ),
    responses(
        (status = 200, description = "The Connect cluster, its workers and its counts", body = ConnectClusterDetail),
        (status = 404, description = "No such Connect cluster, or not visible"),
    ),
    tag = "connect",
)]
pub async fn detail(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
) -> ApiResult<Json<ConnectClusterDetail>> {
    let handle = state.connect_cluster(&env, &id, &caller)?;
    let client = handle.client();

    // The header renders whatever happens: a worker that is down is a card
    // that says so, not a failed page.
    let mut errors = Vec::new();
    let info = match client.ready().await {
        Ok(info) => Some(info),
        Err(fault) => {
            errors.push(connect_resource_error("workers", &fault));
            None
        }
    };
    let counts = match info {
        Some(_) => match client.connectors().await {
            Ok(connectors) => Some(ConnectorCounts::of(&connectors)),
            Err(fault) => {
                errors.push(connect_resource_error("connectors", &fault));
                None
            }
        },
        None => None,
    };
    let plugin_count = match info {
        Some(_) => match client.plugins().await {
            Ok(plugins) => Some(plugins.len()),
            Err(fault) => {
                errors.push(connect_resource_error("plugins", &fault));
                None
            }
        },
        None => None,
    };

    let kafka_cluster_check = check_kafka_cluster(
        &state,
        &env,
        handle.kafka_cluster.as_deref(),
        info.as_ref()
            .and_then(|info| info.kafka_cluster_id.as_deref()),
        &caller,
    );

    Ok(Json(ConnectClusterDetail {
        card: ConnectClusterCard::of(&handle),
        kafka_cluster_check,
        counts,
        plugin_count,
        errors,
        create: connect_write::create_option(&state, &handle, &caller),
    }))
}

/// Compare the configured reference with what the workers report.
///
/// The expected id is read from the referenced cluster's metadata snapshot —
/// never dialled — so an unconnected Kafka cluster makes this `unverified`
/// rather than putting a broker round trip on a Connect page.
fn check_kafka_cluster(
    state: &AppState,
    env: &str,
    reference: Option<&str>,
    actual: Option<&str>,
    caller: &Caller,
) -> KafkaClusterCheck {
    let Some(reference) = reference else {
        return KafkaClusterCheck {
            state: KafkaClusterCheckState::NotConfigured,
            expected: None,
            actual: actual.map(str::to_owned),
        };
    };
    let registry = state.registry();
    let expected = registry
        .get(env, reference, caller.access())
        .and_then(|cluster| cluster.admin())
        .and_then(|admin| admin.cluster().snapshot().cluster_id().map(str::to_owned));
    let verdict = match (expected.as_deref(), actual) {
        (Some(expected), Some(actual)) if expected == actual => KafkaClusterCheckState::Verified,
        (Some(_), Some(_)) => KafkaClusterCheckState::Mismatch,
        _ => KafkaClusterCheckState::Unverified,
    };
    KafkaClusterCheck {
        state: verdict,
        expected,
        actual: actual.map(str::to_owned),
    }
}

/// The connector table.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorList {
    /// The Connect cluster answering.
    pub card: ConnectClusterCard,
    /// The matching connectors, by name.
    pub connectors: Vec<ConnectorSummary>,
    /// How many connectors there are before filtering.
    pub total: usize,
}

/// How to narrow the connector table.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorQuery {
    /// Case-insensitive substring of the name or the class.
    pub search: Option<String>,
    /// Comma-separated derived states to keep.
    pub status: Option<String>,
    /// `source` or `sink`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// `GET /api/environments/{env}/connect-clusters/{connect}/connectors`
#[utoipa::path(
    get,
    path = "/api/environments/{env}/connect-clusters/{connect}/connectors",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("connect" = String, Path, description = "Connect cluster id"),
        ("search" = Option<String>, Query, description = "Substring of the name or class"),
        ("status" = Option<String>, Query, description = "Comma-separated derived states"),
        ("type" = Option<String>, Query, description = "source | sink"),
    ),
    responses(
        (status = 200, description = "The connectors, with state and tasks", body = ConnectorList),
        (status = 404, description = "No such Connect cluster, or not visible"),
        (status = 502, description = "The workers could not be reached or refused kaas-ui"),
        (status = 503, description = "The workers are rebalancing"),
    ),
    tag = "connect",
)]
pub async fn connectors(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
    Query(query): Query<ConnectorQuery>,
) -> ApiResult<Json<ConnectorList>> {
    let handle = state.connect_cluster(&env, &id, &caller)?;
    let all = handle.client().connectors().await?;

    let needle = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|needle| !needle.is_empty())
        .map(str::to_lowercase);
    let statuses: Option<Vec<String>> = query.status.as_deref().map(super::split_list);
    let kind = match query.kind.as_deref() {
        Some("source") => Some(ConnectorType::Source),
        Some("sink") => Some(ConnectorType::Sink),
        Some(other) => {
            return Err(crate::ApiError::bad_request(format!(
                "type must be `source` or `sink`, not {other:?}"
            )));
        }
        None => None,
    };

    let connectors = all
        .iter()
        .filter(|connector| {
            needle.as_ref().is_none_or(|needle| {
                connector.name.to_lowercase().contains(needle)
                    || connector
                        .class
                        .as_ref()
                        .is_some_and(|class| class.to_lowercase().contains(needle))
            })
        })
        .filter(|connector| {
            statuses.as_ref().is_none_or(|wanted| {
                serde_json::to_value(connector.status)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .is_some_and(|status| wanted.contains(&status))
            })
        })
        .filter(|connector| kind.is_none_or(|kind| connector.kind == kind))
        .cloned()
        .collect();

    Ok(Json(ConnectorList {
        card: ConnectClusterCard::of(&handle),
        connectors,
        total: all.len(),
    }))
}

/// One connector's page.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorPage {
    /// The Connect cluster answering.
    pub card: ConnectClusterCard,
    /// State, tasks, redacted config, task configs.
    pub connector: ConnectorDetail,
    /// The topics the connector has used, where the worker tracks them.
    pub topics: Option<Vec<String>>,
    /// Why `topics` is absent, where it is.
    pub topics_absent: Option<String>,
    /// The connector's stored offsets, where the worker has the endpoint.
    pub offsets: Option<Vec<StoredOffset>>,
    /// Why `offsets` is absent, where it is.
    pub offsets_absent: Option<String>,
    /// What could not be read for any other reason, by name.
    pub errors: Vec<kaas_ui_core::ResourceError>,
    /// Every write this connector could take, and whether this caller may.
    /// Empty where the Connect cluster has no `writes:` ceiling — the
    /// read-only default renders no write UI at all.
    pub writes: Vec<WriteOption>,
}

/// `GET /api/environments/{env}/connect-clusters/{connect}/connectors/{name}`
#[utoipa::path(
    get,
    path = "/api/environments/{env}/connect-clusters/{connect}/connectors/{name}",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("connect" = String, Path, description = "Connect cluster id"),
        ("name" = String, Path, description = "Connector name"),
    ),
    responses(
        (status = 200, description = "One connector, its tasks, redacted config, topics and offsets", body = ConnectorPage),
        (status = 404, description = "No such Connect cluster or connector"),
        (status = 502, description = "The workers could not be reached or refused kaas-ui"),
        (status = 503, description = "The workers are rebalancing"),
    ),
    tag = "connect",
)]
pub async fn connector(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, name)): Path<(String, String, String)>,
) -> ApiResult<Json<ConnectorPage>> {
    let handle = state.connect_cluster(&env, &id, &caller)?;
    let client = handle.client();
    let detail = client.connector(&name).await?;

    let mut errors = Vec::new();
    let (topics, topics_absent) = split(client.topics(&name).await, "topics", &mut errors);
    let (offsets, offsets_absent) = split(client.offsets(&name).await, "offsets", &mut errors);
    let writes = connect_write::write_options(&state, &handle, &name, &caller);

    Ok(Json(ConnectorPage {
        card: ConnectClusterCard::of(&handle),
        connector: detail,
        topics,
        topics_absent,
        offsets,
        offsets_absent,
        errors,
        writes,
    }))
}

/// Sort an enriching read into its value, its absence, or a named error.
fn split<T>(
    outcome: Result<T, ConnectFault>,
    resource: &str,
    errors: &mut Vec<kaas_ui_core::ResourceError>,
) -> (Option<T>, Option<String>) {
    match outcome {
        Ok(value) => (Some(value), None),
        Err(ConnectFault::Absent(why)) => (None, Some(why)),
        Err(fault) => {
            errors.push(connect_resource_error(resource, &fault));
            (None, None)
        }
    }
}

/// The installed connector plugins.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PluginList {
    /// The Connect cluster answering.
    pub card: ConnectClusterCard,
    /// Every connector plugin, by class.
    pub plugins: Vec<Plugin>,
}

/// `GET /api/environments/{env}/connect-clusters/{connect}/plugins`
#[utoipa::path(
    get,
    path = "/api/environments/{env}/connect-clusters/{connect}/plugins",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("connect" = String, Path, description = "Connect cluster id"),
    ),
    responses(
        (status = 200, description = "The installed connector plugins", body = PluginList),
        (status = 404, description = "No such Connect cluster, or not visible"),
        (status = 502, description = "The workers could not be reached or refused kaas-ui"),
    ),
    tag = "connect",
)]
pub async fn plugins(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
) -> ApiResult<Json<PluginList>> {
    let handle = state.connect_cluster(&env, &id, &caller)?;
    let plugins = handle.client().plugins().await?;
    Ok(Json(PluginList {
        card: ConnectClusterCard::of(&handle),
        plugins,
    }))
}

/// The connectors that have used one topic.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopicConnectors {
    /// Every connector, on every Connect cluster attached to this Kafka
    /// cluster, whose active topics include this one.
    pub connectors: Vec<TopicConnector>,
    /// Connect clusters or connectors that could not be asked, by name. The
    /// list is then a lower bound, and says so by carrying these.
    pub errors: Vec<kaas_ui_core::ResourceError>,
}

/// One connector that has used a topic.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopicConnector {
    /// The Connect cluster running it.
    pub connect_cluster: String,
    /// That Connect cluster's name, to render.
    pub connect_name: String,
    /// The connector.
    pub name: String,
    /// Source writes this topic; sink reads it.
    #[serde(rename = "type")]
    pub kind: ConnectorType,
    /// Its derived state.
    pub status: ConnectorStatus,
}

/// How many connectors' topic lists to ask for at once — the registry's
/// reasoning: a worker is one service other things are talking to.
const TOPIC_CONCURRENCY: usize = 8;

/// `GET /api/environments/{env}/clusters/{id}/topics/{topic}/connectors`
///
/// The reverse of a connector's topic list. Built from each connector's
/// *active* topics — what the worker saw it do — rather than from its config,
/// because a `topics.regex` cannot be evaluated here and a source connector's
/// targets are often not in its config at all.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/topics/{topic}/connectors",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Kafka cluster id"),
        ("topic" = String, Path, description = "Topic name"),
    ),
    responses(
        (status = 200, description = "The connectors that write or read this topic", body = TopicConnectors),
        (status = 404, description = "No such cluster, or not visible"),
    ),
    tag = "connect",
)]
pub async fn topic_connectors(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, topic)): Path<(String, String, String)>,
) -> ApiResult<Json<TopicConnectors>> {
    let cluster = state.cluster(&env, &id, &caller)?;
    caller.require(
        &id,
        &cluster.labels,
        Resource::Topic,
        Action::View,
        Some(&topic),
    )?;

    let handles: Vec<_> = state
        .registry()
        .connect_clusters_of(&env, &id, caller.access())
        .map(std::sync::Arc::clone)
        .collect();

    let mut connectors = Vec::new();
    let mut errors = Vec::new();
    for handle in handles {
        let client = handle.client();
        let listed = match client.connectors().await {
            Ok(listed) => listed,
            Err(fault) => {
                errors.push(connect_resource_error(client.id(), &fault));
                continue;
            }
        };
        let answers: Vec<_> = futures::stream::iter(listed.iter().cloned())
            .map(|summary| async move {
                let topics = client.topics_cached(&summary.name).await;
                (summary, topics)
            })
            .buffered(TOPIC_CONCURRENCY)
            .collect()
            .await;
        for (summary, topics) in answers {
            match topics {
                Ok(topics) if topics.contains(&topic) => {
                    connectors.push(TopicConnector {
                        connect_cluster: client.id().to_owned(),
                        connect_name: client.name().to_owned(),
                        name: summary.name,
                        kind: summary.kind,
                        status: summary.status,
                    });
                }
                Ok(_) | Err(ConnectFault::Absent(_)) => {}
                Err(fault) => errors.push(connect_resource_error(
                    &format!("{}/{}", client.id(), summary.name),
                    &fault,
                )),
            }
        }
    }

    Ok(Json(TopicConnectors { connectors, errors }))
}
