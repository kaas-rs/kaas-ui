//! Kafka Connect.
//!
//! Rooted at `/api/environments/{env}/connect-clusters/{connect}`, a peer of
//! clusters and schema registries inside an environment. The lookup —
//! [`AppState::connect_cluster`] — answers only for a caller who may view
//! topics on the Kafka cluster the Connect cluster is attached to, so
//! connectors cannot be used to learn which topics flow where on a cluster
//! the caller cannot see.
//!
//! Reads go through a [`kaas_ui_connect::ConnectClient`], which has no method
//! that sends anything but `GET`. Writes — Phase 10, restart, pause, resume,
//! stop — go through [`write`]: the gate mints a `Grant` or says which of the
//! three gates refused, the intent is audited before anything is sent, and the
//! `ConnectWriter` checks the grant once more.
//!
//! What is **not** an error here: a worker without the `/offsets` or
//! `/topics` endpoint. That is absence — the field is empty and says why —
//! and the page renders around it.

use axum::Json;
use axum::extract::{Path, Query, State};
use futures::StreamExt;
use kaas_ui_auth::{Action, Change, Denied, Operation, Resource, Tier};
use kaas_ui_connect::{
    ConnectFault, ConnectorDetail, ConnectorStatus, ConnectorSummary, ConnectorType, Plugin,
    StoredOffset,
};
use kaas_ui_connect::{Equivalent, Format};
use kaas_ui_core::config::ManifestFormat;
use kaas_ui_core::dto::ConnectClusterCard;
use kaas_ui_core::registry::ConnectHandle;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::connect_resource_error;
use crate::{ApiError, ApiResult, AppState, Caller};

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
    /// Every write this connector could take, whether this caller may, and
    /// what each would be in Git. Empty where the Connect cluster has no
    /// `writes:` ceiling — the read-only default renders no write UI at all.
    pub writes: Vec<WriteOption>,
    /// Set when Git owns this connector.
    pub managed_in_git: Option<kaas_ui_auth::Owned>,
}

/// One write a connector page could offer.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WriteOption {
    /// The catalogue operation.
    pub operation: Operation,
    /// Operate, edit or destroy.
    pub tier: Tier,
    /// Whether every gate agrees for this caller now.
    pub allowed: bool,
    /// Which gate refuses, where one does.
    pub refused_by: Option<Refusal>,
    /// The sentence a refusal renders.
    pub reason: Option<String>,
    /// What this write would be in Git, or as a REST call.
    pub equivalent: Equivalent,
}

/// Which gate refused a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Refusal {
    /// Nobody is signed in.
    Anonymous,
    /// The ceiling does not list it.
    NotEnabled,
    /// No role of the caller's permits it.
    Forbidden,
    /// Git owns the connector and this changes declared state.
    Owned,
}

impl Refusal {
    fn of(denied: &Denied) -> Self {
        match denied {
            Denied::Anonymous => Self::Anonymous,
            Denied::NotEnabled(_) => Self::NotEnabled,
            Denied::Forbidden(_) => Self::Forbidden,
            Denied::Owned(..) => Self::Owned,
        }
    }
}

/// The writes a connector page can offer: everything on the Connect cluster's
/// ceiling, checked against this caller, each with its Git equivalent.
fn write_options(
    state: &AppState,
    handle: &ConnectHandle,
    connector: &str,
    caller: &Caller,
) -> (Vec<WriteOption>, Option<kaas_ui_auth::Owned>) {
    let owned = handle.owned(connector);
    if handle.ceiling().is_empty() {
        return (Vec::new(), owned);
    }
    let target = state.registry().connect_target(handle, connector);
    let format = format_of(handle);
    let options = Operation::ALL
        .into_iter()
        .filter(|operation| handle.ceiling().contains(operation))
        .map(|operation| {
            let checked =
                caller
                    .access()
                    .check(operation, &target, handle.ceiling(), owned.as_ref());
            WriteOption {
                operation,
                tier: operation.tier(),
                allowed: checked.is_ok(),
                refused_by: checked.as_ref().err().map(Refusal::of),
                reason: checked.err().map(|denied| denied.message(&target)),
                equivalent: kaas_ui_connect::equivalent(
                    operation,
                    connector,
                    handle.client().url(),
                    format,
                ),
            }
        })
        .collect();
    (options, owned)
}

fn format_of(handle: &ConnectHandle) -> Format {
    match handle.manifest_format() {
        ManifestFormat::Strimzi => Format::Strimzi,
        ManifestFormat::Plain => Format::Plain,
    }
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
    let (writes, managed_in_git) = write_options(&state, &handle, &name, &caller);

    Ok(Json(ConnectorPage {
        card: ConnectClusterCard::of(&handle),
        connector: detail,
        topics,
        topics_absent,
        offsets,
        offsets_absent,
        errors,
        writes,
        managed_in_git,
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

/// The body of a restart. Pause, resume and stop take none.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteRequest {
    /// Restart the connector's tasks as well as the connector.
    #[serde(default)]
    pub include_tasks: bool,
    /// Restart only what has failed. On a healthy connector this restarts
    /// nothing, which makes it the safe default for a button.
    #[serde(default)]
    pub only_failed: bool,
}

/// What a write did.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    /// The id the intent and outcome audit lines share.
    pub audit_id: String,
    /// What was done.
    pub operation: Operation,
    /// To which connector.
    pub connector: String,
    /// What it would have been in Git.
    pub equivalent: Equivalent,
}

/// `POST /api/environments/{env}/connect-clusters/{connect}/connectors/{name}/{operation}`
///
/// `operation` is `restart`, `pause`, `resume` or `stop`. The three gates
/// decide; their refusals are `403` (no ceiling, no role, nobody signed in) or
/// `409` (Git owns the connector, with the change to make there in the body).
/// The intent is audited before the worker is asked anything, and a failure
/// to record it is a failure to write.
#[utoipa::path(
    post,
    path = "/api/environments/{env}/connect-clusters/{connect}/connectors/{name}/{operation}",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("connect" = String, Path, description = "Connect cluster id"),
        ("name" = String, Path, description = "Connector name"),
        ("operation" = String, Path, description = "restart | pause | resume | stop"),
        ("X-Kaas-Ui" = String, Header, description = "Must be `1`: proves the request came from kaas-ui's own page"),
    ),
    request_body(content = Option<WriteRequest>, description = "restart's options; the others take none"),
    responses(
        (status = 200, description = "Done, and audited", body = WriteResult),
        (status = 403, description = "Not enabled here, not permitted, or not signed in", body = crate::error::ApiErrorBody),
        (status = 404, description = "No such Connect cluster, connector or operation"),
        (status = 409, description = "Git owns the connector; the body carries the change to make there", body = crate::error::ApiErrorBody),
        (status = 502, description = "The workers refused or could not be reached"),
    ),
    tag = "connect",
)]
pub async fn write(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, name, operation)): Path<(String, String, String, String)>,
    body: Option<Json<WriteRequest>>,
) -> ApiResult<Json<WriteResult>> {
    let operation = match operation.as_str() {
        "restart" => Operation::ConnectorRestart,
        "pause" => Operation::ConnectorPause,
        "resume" => Operation::ConnectorResume,
        "stop" => Operation::ConnectorStop,
        other => {
            return Err(ApiError::not_found(format!(
                "no connector operation {other:?}: restart, pause, resume or stop"
            )));
        }
    };
    let request = body.map(|Json(request)| request).unwrap_or_default();

    let handle = state.connect_cluster(&env, &id, &caller)?;
    let target = state.registry().connect_target(&handle, &name);
    let owned = handle.owned(&name);
    let equivalent =
        kaas_ui_connect::equivalent(operation, &name, handle.client().url(), format_of(&handle));

    let grant = caller
        .access()
        .authorize(
            caller.principal(),
            operation,
            target.clone(),
            handle.ceiling(),
            owned.as_ref(),
        )
        .map_err(|denied| ApiError::denied(&denied, &target, Some(equivalent.clone())))?;

    let parameters = match operation {
        Operation::ConnectorRestart => serde_json::json!({
            "includeTasks": request.include_tasks,
            "onlyFailed": request.only_failed,
        }),
        _ => serde_json::Value::Null,
    };
    let intent = Change::intent(&grant, parameters);
    state
        .audit()
        .record(&intent)
        .map_err(|error| ApiError::intent_unrecorded(&error.to_string()))?;

    let writer = handle.client().writer(&grant)?;
    let outcome = match operation {
        Operation::ConnectorRestart => {
            writer
                .restart(request.include_tasks, request.only_failed)
                .await
        }
        Operation::ConnectorPause => writer.pause().await,
        Operation::ConnectorResume => writer.resume().await,
        Operation::ConnectorStop => writer.stop().await,
    };
    state
        .audit()
        .record(&intent.outcome(outcome.as_ref().map(|_| ()).map_err(ToString::to_string)))
        .map_err(|error| ApiError::outcome_unrecorded(&error.to_string()))?;
    outcome?;

    Ok(Json(WriteResult {
        audit_id: grant.id().to_owned(),
        operation,
        connector: name,
        equivalent,
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use arc_swap::ArcSwap;
    use axum::http::StatusCode;
    use kaas_ui_auth::{Audit, Permission, Policy, Principal, Role};
    use kaas_ui_core::{Config, Registry};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    /// A writer the test can read back.
    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Lines {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Lines {
        fn read(&self) -> Vec<serde_json::Value> {
            String::from_utf8(self.0.lock().unwrap().clone())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    /// A worker that accepts everything and writes down what it was sent.
    async fn worker() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&log);
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let seen = Arc::clone(&seen);
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut chunk = [0_u8; 1024];
                    while !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                        }
                    }
                    let head = String::from_utf8_lossy(&buffer).to_string();
                    let first = head.lines().next().unwrap_or("").to_owned();
                    seen.lock().unwrap().push(first);
                    let _ = socket
                        .write_all(
                            b"HTTP/1.1 202 Accepted\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}",
                        )
                        .await;
                });
            }
        });
        (format!("http://{addr}"), log)
    }

    fn state(url: &str, audit: Lines) -> AppState {
        let config = Config::from_yaml(&format!(
            r#"
auth:
  issuer: https://kaas.example/dex
  client_id: kaas-ui
  redirect_url: https://kaas.example/auth/callback
environments:
  - id: dev
    kafka_clusters:
      - id: strimzi
        bootstrap: ["strimzi:9092"]
    connect_clusters:
      - id: connect
        url: {url}
        kafka_cluster: strimzi
        writes:
          allow: [connector.restart, connector.pause]
          gitops:
            owns_connectors: ["owned-*"]
            source: k3s-cluster/apps/strimzi/kafka-connectors
            format: strimzi
"#
        ))
        .unwrap();
        let policy = Policy::enforcing(vec![
            Role {
                name: "oncall".into(),
                subjects: vec!["alice".into()],
                clusters: vec!["strimzi".into()],
                cluster_labels: BTreeMap::new(),
                permissions: vec![
                    Permission {
                        resource: kaas_ui_auth::Resource::Topic,
                        value: None,
                        actions: vec![kaas_ui_auth::Action::View],
                    },
                    Permission {
                        resource: kaas_ui_auth::Resource::Connector,
                        value: None,
                        actions: vec![kaas_ui_auth::Action::Operate],
                    },
                ],
            },
            Role {
                name: "viewer".into(),
                subjects: vec!["bob".into()],
                clusters: vec!["strimzi".into()],
                cluster_labels: BTreeMap::new(),
                permissions: vec![Permission {
                    resource: kaas_ui_auth::Resource::Topic,
                    value: None,
                    actions: vec![kaas_ui_auth::Action::View],
                }],
            },
        ]);
        AppState::new(
            Arc::new(ArcSwap::from_pointee(
                Registry::from_config(&config).unwrap(),
            )),
            policy,
        )
        .with_audit(Arc::new(Audit::to_writer(Box::new(audit))))
    }

    fn caller(state: &AppState, who: &str) -> Caller {
        let principal = Principal::new(who);
        let access = state.policy().access(&principal);
        Caller::new(principal, access)
    }

    async fn post(
        state: &AppState,
        who: &str,
        name: &str,
        op: &str,
    ) -> ApiResult<Json<WriteResult>> {
        write(
            State(state.clone()),
            caller(state, who),
            Path(("dev".into(), "connect".into(), name.into(), op.into())),
            Some(Json(WriteRequest {
                include_tasks: true,
                only_failed: true,
            })),
        )
        .await
    }

    #[tokio::test]
    async fn a_granted_restart_is_audited_before_and_after_and_reaches_the_worker() {
        let (url, sent) = worker().await;
        let audit = Lines::default();
        let state = state(&url, audit.clone());

        let Json(result) = post(&state, "alice", "gtfs", "restart").await.unwrap();
        assert_eq!(result.operation, Operation::ConnectorRestart);
        assert_eq!(
            sent.lock().unwrap().as_slice(),
            ["POST /connectors/gtfs/restart?includeTasks=true&onlyFailed=true HTTP/1.1"]
        );

        let lines = audit.read();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0]["phase"], "intent");
        assert_eq!(lines[1]["phase"], "outcome");
        assert_eq!(lines[1]["ok"], true);
        assert_eq!(lines[0]["id"], lines[1]["id"]);
        assert_eq!(lines[0]["id"], result.audit_id);
        assert_eq!(lines[0]["roles"], serde_json::json!(["oncall"]));
    }

    #[tokio::test]
    async fn each_gate_refuses_in_its_own_words_and_nothing_reaches_the_worker() {
        let (url, sent) = worker().await;
        let audit = Lines::default();
        let state = state(&url, audit.clone());

        // No role permits bob to operate.
        let forbidden = post(&state, "bob", "gtfs", "restart").await.unwrap_err();
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        // The ceiling does not list stop.
        let not_enabled = post(&state, "alice", "gtfs", "stop").await.unwrap_err();
        assert_eq!(not_enabled.status(), StatusCode::FORBIDDEN);

        // Git owns `owned-a`: a pause is declared state, so 409 with the
        // KafkaConnector change to make instead...
        let owned = post(&state, "alice", "owned-a", "pause").await.unwrap_err();
        assert_eq!(owned.status(), StatusCode::CONFLICT);

        // ...and nobody signed in may write at all.
        let anonymous = write(
            State(state.clone()),
            Caller::new(
                Principal::anonymous(),
                state.policy().access(&Principal::anonymous()),
            ),
            Path((
                "dev".into(),
                "connect".into(),
                "gtfs".into(),
                "restart".into(),
            )),
            None,
        )
        .await
        .unwrap_err();
        // Anonymous sees no Connect cluster in an enforcing deployment: 404,
        // before any gate is asked.
        assert_eq!(anonymous.status(), StatusCode::NOT_FOUND);

        assert!(
            sent.lock().unwrap().is_empty(),
            "{:?}",
            sent.lock().unwrap()
        );
        assert!(audit.read().is_empty(), "a refused write leaves no intent");

        // A restart of an owned connector is operate-tier and goes through.
        assert!(post(&state, "alice", "owned-a", "restart").await.is_ok());
    }

    #[tokio::test]
    async fn a_failed_intent_record_means_no_write() {
        struct Broken;
        impl std::io::Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (url, sent) = worker().await;
        let state =
            state(&url, Lines::default()).with_audit(Arc::new(Audit::to_writer(Box::new(Broken))));
        let error = post(&state, "alice", "gtfs", "restart").await.unwrap_err();
        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_page_offers_what_the_caller_may_do_and_says_what_git_owns() {
        let (url, _) = worker().await;
        let state = state(&url, Lines::default());
        let handle = state
            .connect_cluster("dev", "connect", &caller(&state, "alice"))
            .unwrap();

        let (alice, _) = write_options(&state, &handle, "owned-a", &caller(&state, "alice"));
        let restart = alice
            .iter()
            .find(|o| o.operation == Operation::ConnectorRestart)
            .unwrap();
        assert!(restart.allowed);
        let pause = alice
            .iter()
            .find(|o| o.operation == Operation::ConnectorPause)
            .unwrap();
        assert_eq!(pause.refused_by, Some(Refusal::Owned));
        assert!(pause.equivalent.text.contains("state: paused"));
        // Only what the ceiling lists is offered at all.
        assert!(
            !alice
                .iter()
                .any(|o| o.operation == Operation::ConnectorStop)
        );
    }
}
