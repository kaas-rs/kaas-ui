//! MirrorMaker 2 flows: Phase 9b (from the Connect API) and 9c (from the
//! clusters themselves).
//!
//! A flow is `source alias → target alias`. Two places can say one exists:
//!
//! * **a Connect cluster** — any of them in the environment, dedicated to
//!   MirrorMaker or not — running one of the three Mirror connectors, whose
//!   config names the aliases. A flow can be split across Connect clusters,
//!   and each piece is attributed to the one hosting it;
//! * **the clusters' own topics** — `<source>.checkpoints.internal` on a
//!   target, `mm2-offset-syncs.<target>.internal` on a source. That is how a
//!   dedicated-mode MirrorMaker, which has no public REST API at all, is seen
//!   at all; such a flow is labelled `discovered` and has no connector state.
//!
//! **MirrorMaker 2 only.** The legacy MirrorMaker — a consumer and a producer
//! in one process, since removed from Kafka — runs no connectors and writes
//! none of these topics, so there is nothing of it to find and kaas-ui does
//! not look. Every label says "MirrorMaker 2" for the same reason.
//!
//! Aliases resolve through the environment's `mirror_aliases`, or to the
//! cluster with the same id. A flow whose resolved end the caller cannot see
//! is not listed: a mirrored topic's name on a cluster you cannot see is a
//! fact about that cluster.
//!
//! The internal topics are read for the flow page — heartbeats, checkpoints,
//! offset syncs — and decoded in `kaas_ui_serde::mirror`. They carry
//! replication metadata, not payloads: reading them needs topic view on the
//! cluster that holds them (and consumer view, for checkpoints, which name
//! groups), and is not audited as a disclosure.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Path, State};
use kaas_ui_auth::{Action, Resource};
use kaas_ui_connect::{ConnectorStatus, MirrorKind, ReplicationPolicy};
use kaas_ui_serde::mirror::{self, Checkpoint, Heartbeat, OffsetSync};
use kafka_read::TailSpec;
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::connect_resource_error;
use crate::{ApiError, ApiResult, AppState, Caller};

/// One end of a flow, resolved to a Kafka cluster kaas-ui shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlowEnd {
    /// The environment holding it — not always this one.
    pub environment: String,
    /// The cluster id.
    pub id: String,
}

/// One Mirror connector in a flow, and where it runs.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlowConnector {
    /// The Connect cluster hosting it.
    pub connect_cluster: String,
    /// That Connect cluster's name.
    pub connect_name: String,
    /// The connector.
    pub name: String,
    /// Source, checkpoint or heartbeat.
    pub kind: MirrorKind,
    /// Its derived state.
    pub status: ConnectorStatus,
    /// Running tasks.
    pub tasks_running: usize,
    /// All tasks.
    pub tasks_total: usize,
}

/// One MirrorMaker 2 flow.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MirrorFlow {
    /// The source alias, as the connectors or topics name it.
    pub source_alias: String,
    /// The target alias.
    pub target_alias: String,
    /// The source cluster, where the alias resolves.
    pub source: Option<FlowEnd>,
    /// The target cluster, where the alias resolves.
    pub target: Option<FlowEnd>,
    /// The Mirror connectors, wherever they run.
    pub connectors: Vec<FlowConnector>,
    /// Which of the three connectors no Connect cluster runs. A flow with no
    /// checkpoint connector translates no offsets: a consumer failing over
    /// starts from `auto.offset.reset`. Empty for a discovered flow, where
    /// that is unknown rather than complete.
    pub missing: Vec<MirrorKind>,
    /// How the flow names topics on its target.
    pub policy: Option<ReplicationPolicy>,
    /// `topics`, verbatim — a regex MirrorMaker evaluates and kaas-ui does not.
    pub topics: Option<String>,
    /// `topics.exclude`, verbatim.
    pub topics_exclude: Option<String>,
    /// `groups`, verbatim.
    pub groups: Option<String>,
    /// `groups.exclude`, verbatim.
    pub groups_exclude: Option<String>,
    /// Found from the clusters' internal topics and from no Connect API: a
    /// dedicated-mode MirrorMaker, or one on a Connect cluster kaas-ui is not
    /// configured with. Task state is unknown.
    pub discovered: bool,
}

/// The flows of one environment.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MirrorList {
    /// Every flow this caller may see.
    pub flows: Vec<MirrorFlow>,
    /// Connect clusters that could not be asked. The list is a lower bound
    /// while there are any.
    pub errors: Vec<kaas_ui_core::ResourceError>,
}

/// Assemble the flows of `env` for `caller`.
async fn flows(
    state: &AppState,
    env: &str,
    caller: &Caller,
) -> ApiResult<(Vec<MirrorFlow>, Vec<kaas_ui_core::ResourceError>)> {
    let entry = state.environment(env, caller)?;
    let registry = state.registry();
    let mut errors = Vec::new();
    let mut by_key: BTreeMap<(String, String), MirrorFlow> = BTreeMap::new();

    // From the Connect API, every Connect cluster this caller may see here.
    let handles: Vec<_> = registry
        .connect_clusters_in(&entry, caller.access())
        .map(std::sync::Arc::clone)
        .collect();
    for handle in handles {
        let client = handle.client();
        let listed = match client.connectors().await {
            Ok(listed) => listed,
            Err(fault) => {
                errors.push(connect_resource_error(client.id(), &fault));
                continue;
            }
        };
        for summary in listed.iter() {
            let Some(role) = &summary.mirror else {
                continue;
            };
            let key = (
                role.source_alias.clone().unwrap_or_else(|| "?".to_owned()),
                role.target_alias.clone().unwrap_or_else(|| "?".to_owned()),
            );
            let flow = by_key
                .entry(key.clone())
                .or_insert_with(|| empty_flow(key.0, key.1));
            flow.connectors.push(FlowConnector {
                connect_cluster: client.id().to_owned(),
                connect_name: client.name().to_owned(),
                name: summary.name.clone(),
                kind: role.kind,
                status: summary.status,
                tasks_running: summary.tasks_running,
                tasks_total: summary.tasks.len(),
            });
            // The source connector speaks for the topics and the policy, the
            // checkpoint connector for the groups; either is better than none.
            if role.kind == MirrorKind::Source || flow.policy.is_none() {
                flow.policy = Some(role.policy.clone());
            }
            if role.kind == MirrorKind::Source {
                flow.topics.clone_from(&role.topics);
                flow.topics_exclude.clone_from(&role.topics_exclude);
            }
            if role.kind == MirrorKind::Checkpoint {
                flow.groups.clone_from(&role.groups);
                flow.groups_exclude.clone_from(&role.groups_exclude);
            }
        }
    }

    // From the clusters' own topics: what a MirrorMaker without a Connect API
    // leaves behind. Default-policy naming with `.`, the only shape a name
    // alone can be read in.
    for cluster in registry.visible(caller.access()) {
        if cluster.environment != env {
            continue;
        }
        let Some(admin) = cluster.admin() else {
            continue;
        };
        let here = (cluster.environment.clone(), cluster.id.clone());
        let Some(alias) = registry.alias_of(env, &here) else {
            continue;
        };
        for topic in admin.cluster().snapshot().topics() {
            // An internal topic's name is a topic name like any other: a
            // caller who may not view it does not learn the flow from it.
            if !caller.access().may(
                &cluster.id,
                &cluster.labels,
                Resource::Topic,
                Action::View,
                Some(&topic.name),
            ) {
                continue;
            }
            let found = if let Some(source) = topic.name.strip_suffix(".checkpoints.internal") {
                Some((source.to_owned(), alias.clone()))
            } else {
                topic
                    .name
                    .strip_prefix("mm2-offset-syncs.")
                    .and_then(|rest| rest.strip_suffix(".internal"))
                    .map(|target| (alias.clone(), target.to_owned()))
            };
            if let Some(key) = found {
                by_key.entry(key.clone()).or_insert_with(|| MirrorFlow {
                    discovered: true,
                    ..empty_flow(key.0, key.1)
                });
            }
        }
    }

    let mut listed = Vec::new();
    for (_, mut flow) in by_key {
        let resolve = |alias: &str| registry.resolve_alias(env, alias);
        let source = resolve(&flow.source_alias);
        let target = resolve(&flow.target_alias);
        // An end the caller cannot see hides the whole flow.
        let hidden = [&source, &target]
            .into_iter()
            .flatten()
            .any(|(e, id)| registry.get(e, id, caller.access()).is_none());
        if hidden {
            continue;
        }
        flow.source = source.map(|(environment, id)| FlowEnd { environment, id });
        flow.target = target.map(|(environment, id)| FlowEnd { environment, id });
        if !flow.discovered {
            let present: BTreeSet<MirrorKind> = flow
                .connectors
                .iter()
                .map(|connector| connector.kind)
                .collect();
            flow.missing = [
                MirrorKind::Source,
                MirrorKind::Checkpoint,
                MirrorKind::Heartbeat,
            ]
            .into_iter()
            .filter(|kind| !present.contains(kind))
            .collect();
        }
        flow.connectors.sort_by_key(|connector| connector.kind);
        listed.push(flow);
    }
    Ok((listed, errors))
}

fn empty_flow(source_alias: String, target_alias: String) -> MirrorFlow {
    MirrorFlow {
        source_alias,
        target_alias,
        source: None,
        target: None,
        connectors: Vec::new(),
        missing: Vec::new(),
        policy: None,
        topics: None,
        topics_exclude: None,
        groups: None,
        groups_exclude: None,
        discovered: false,
    }
}

/// `GET /api/environments/{env}/mirrors`
#[utoipa::path(
    get,
    path = "/api/environments/{env}/mirrors",
    params(("env" = String, Path, description = "Environment id")),
    responses(
        (status = 200, description = "The MirrorMaker 2 flows in this environment", body = MirrorList),
        (status = 404, description = "No such environment, or nothing in it is visible"),
    ),
    tag = "connect",
)]
pub async fn list(
    State(state): State<AppState>,
    caller: Caller,
    Path(env): Path<String>,
) -> ApiResult<Json<MirrorList>> {
    let (flows, errors) = flows(&state, &env, &caller).await?;
    Ok(Json(MirrorList { flows, errors }))
}

/// One mirrored topic, by both its names.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MirroredTopic {
    /// Its name on the source, where the policy says.
    pub source_topic: Option<String>,
    /// Its name on the target.
    pub target_topic: String,
    /// Partitions on the source, where that cluster is connected.
    pub source_partitions: Option<usize>,
    /// Partitions on the target, where that cluster is connected. A count that
    /// differs from the source's is a finding.
    pub target_partitions: Option<usize>,
}

/// The newest heartbeat on one topic.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatSeen {
    /// The topic it was read from.
    pub topic: String,
    /// What it says.
    pub heartbeat: Heartbeat,
    /// How long ago it was emitted.
    pub age_ms: i64,
}

/// A checkpoint, with the target name of its topic.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointRow {
    /// What MirrorMaker wrote.
    pub checkpoint: Checkpoint,
    /// The topic's name on the target, where the policy says.
    pub target_topic: Option<String>,
}

/// One flow in full.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MirrorFlowDetail {
    /// The flow.
    pub flow: MirrorFlow,
    /// The mirrored topics.
    pub topics: Vec<MirroredTopic>,
    /// The newest heartbeat on each heartbeat topic of the target that holds
    /// one for this flow.
    pub heartbeats: Vec<HeartbeatSeen>,
    /// The newest checkpoint per group, topic and partition.
    pub checkpoints: Vec<CheckpointRow>,
    /// The newest offset sync per topic and partition: replication has got at
    /// least this far. Sparse by design, so a lower bound.
    pub offset_syncs: Vec<OffsetSync>,
    /// What could not be read, by name.
    pub errors: Vec<kaas_ui_core::ResourceError>,
}

/// How many records of an internal topic to read: the newest heartbeats,
/// and enough of a checkpoint or offset-sync topic to hold the latest entry
/// per key on a small flow.
const HEARTBEAT_TAIL: usize = 20;
const STATE_TAIL: usize = 500;

/// `GET /api/environments/{env}/mirrors/{source}/{target}`
#[utoipa::path(
    get,
    path = "/api/environments/{env}/mirrors/{source}/{target}",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("source" = String, Path, description = "Source alias"),
        ("target" = String, Path, description = "Target alias"),
    ),
    responses(
        (status = 200, description = "One flow: connectors, mirrored topics, heartbeats, checkpoints, offset syncs", body = MirrorFlowDetail),
        (status = 404, description = "No such flow, or not visible"),
    ),
    tag = "connect",
)]
pub async fn detail(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, source, target)): Path<(String, String, String)>,
) -> ApiResult<Json<MirrorFlowDetail>> {
    let (flows, mut errors) = flows(&state, &env, &caller).await?;
    let flow = flows
        .into_iter()
        .find(|flow| flow.source_alias == source && flow.target_alias == target)
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "no MirrorMaker 2 flow {source} → {target} in {env:?}"
            ))
        })?;
    let policy = flow.policy.clone().unwrap_or(ReplicationPolicy::Default {
        separator: ".".to_owned(),
    });
    let separator = policy.separator().to_owned();
    let registry = state.registry();
    let handle_of = |end: &Option<FlowEnd>| {
        end.as_ref()
            .and_then(|end| registry.get(&end.environment, &end.id, caller.access()))
            .map(std::sync::Arc::clone)
    };
    let source_cluster = handle_of(&flow.source);
    let target_cluster = handle_of(&flow.target);
    let partitions_on = |cluster: &Option<std::sync::Arc<kaas_ui_core::ClusterHandle>>,
                         topic: &str| {
        cluster
            .as_ref()
            .and_then(|cluster| cluster.admin())
            .and_then(|admin| {
                admin
                    .cluster()
                    .snapshot()
                    .topic(topic)
                    .map(|info| info.partitions.len())
            })
    };

    // Mirrored topics: what the source connector was seen writing, or — for a
    // discovered flow — what the target holds under the source's prefix.
    let mut remote_names: BTreeSet<String> = BTreeSet::new();
    if let Some(connector) = flow
        .connectors
        .iter()
        .find(|connector| connector.kind == MirrorKind::Source)
        && let Some(handle) =
            registry.connect_cluster(&env, &connector.connect_cluster, caller.access())
    {
        match handle.client().topics_cached(&connector.name).await {
            Ok(topics) => remote_names.extend(topics.iter().cloned()),
            Err(kaas_ui_connect::ConnectFault::Absent(_)) => {}
            Err(fault) => errors.push(connect_resource_error(&connector.name, &fault)),
        }
    }
    if remote_names.is_empty()
        && let Some(admin) = target_cluster.as_ref().and_then(|cluster| cluster.admin())
    {
        let prefix = format!("{source}{separator}");
        remote_names.extend(
            admin
                .cluster()
                .snapshot()
                .topics()
                .iter()
                .filter(|topic| topic.name.starts_with(&prefix))
                .map(|topic| topic.name.clone()),
        );
    }
    let internal = |name: &str| name.ends_with(".internal") || name.ends_with("heartbeats");
    let may_view = |cluster: &Option<std::sync::Arc<kaas_ui_core::ClusterHandle>>, topic: &str| {
        cluster.as_ref().is_some_and(|cluster| {
            caller.access().may(
                &cluster.id,
                &cluster.labels,
                Resource::Topic,
                Action::View,
                Some(topic),
            )
        })
    };
    // Listed only where the caller may view both names: a mirrored topic is
    // two topics, and the row names each.
    let topics = remote_names
        .into_iter()
        .filter(|name| !internal(name))
        .filter_map(|target_topic| {
            let source_topic = policy.upstream(&source, &target_topic);
            let visible = may_view(&target_cluster, &target_topic)
                && source_topic
                    .as_deref()
                    .is_none_or(|topic| may_view(&source_cluster, topic));
            visible.then_some((target_topic, source_topic))
        })
        .map(|(target_topic, source_topic)| MirroredTopic {
            source_partitions: source_topic
                .as_deref()
                .and_then(|topic| partitions_on(&source_cluster, topic)),
            target_partitions: partitions_on(&target_cluster, &target_topic),
            source_topic,
            target_topic,
        })
        .collect();

    // Heartbeats on the target: those its own heartbeat connector emits for
    // this flow, and those replicated from the source.
    let mut heartbeats = Vec::new();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        });
    let remote_heartbeats = format!("{source}{separator}{}", mirror::HEARTBEATS);
    for topic in [mirror::HEARTBEATS.to_owned(), remote_heartbeats.clone()] {
        let records = read(
            &caller,
            &target_cluster,
            &topic,
            HEARTBEAT_TAIL,
            false,
            &mut errors,
        )
        .await;
        let newest = records
            .iter()
            .filter_map(|(key, value)| mirror::heartbeat(key.as_deref(), value.as_deref()).ok())
            .filter(|beat| {
                // The local topic carries every flow targeting this cluster;
                // only this flow's are its evidence. The replicated one is the
                // source's own topic, so whatever arrived is evidence.
                topic == remote_heartbeats
                    || (beat.source_alias == source && beat.target_alias == target)
            })
            .max_by_key(|beat| beat.timestamp_ms);
        if let Some(heartbeat) = newest {
            heartbeats.push(HeartbeatSeen {
                age_ms: now.saturating_sub(heartbeat.timestamp_ms),
                topic,
                heartbeat,
            });
        }
    }

    // Checkpoints on the target, newest per key.
    let checkpoint_topic = mirror::checkpoints_topic(&source, &separator);
    let mut latest: BTreeMap<(String, String, i32), Checkpoint> = BTreeMap::new();
    for (key, value) in read(
        &caller,
        &target_cluster,
        &checkpoint_topic,
        STATE_TAIL,
        true,
        &mut errors,
    )
    .await
    {
        if let Ok(checkpoint) = mirror::checkpoint(key.as_deref(), value.as_deref()) {
            latest.insert(
                (
                    checkpoint.group.clone(),
                    checkpoint.topic.clone(),
                    checkpoint.partition,
                ),
                checkpoint,
            );
        }
    }
    let checkpoints = latest
        .into_values()
        .map(|checkpoint| CheckpointRow {
            target_topic: policy.remote(&source, &checkpoint.topic),
            checkpoint,
        })
        .collect();

    // Offset syncs on the source, newest per partition.
    let syncs_topic = mirror::offset_syncs_topic(&target, &separator);
    let mut syncs: BTreeMap<(String, i32), OffsetSync> = BTreeMap::new();
    for (key, value) in read(
        &caller,
        &source_cluster,
        &syncs_topic,
        STATE_TAIL,
        false,
        &mut errors,
    )
    .await
    {
        if let Ok(sync) = mirror::offset_sync(key.as_deref(), value.as_deref()) {
            syncs.insert((sync.topic.clone(), sync.partition), sync);
        }
    }

    Ok(Json(MirrorFlowDetail {
        flow,
        topics,
        heartbeats,
        checkpoints,
        offset_syncs: syncs.into_values().collect(),
        errors,
    }))
}

/// The newest records of an internal topic, oldest first per partition, as
/// key and value bytes — or nothing, where the topic is absent, the cluster
/// is not connected, or the caller may not look. Only a failed read is an
/// error; the other three are the ordinary shapes of a flow.
async fn read(
    caller: &Caller,
    cluster: &Option<std::sync::Arc<kaas_ui_core::ClusterHandle>>,
    topic: &str,
    limit: usize,
    names_groups: bool,
    errors: &mut Vec<kaas_ui_core::ResourceError>,
) -> Vec<(Option<Vec<u8>>, Option<Vec<u8>>)> {
    let Some(cluster) = cluster else {
        return Vec::new();
    };
    let Some(admin) = cluster.admin() else {
        return Vec::new();
    };
    if admin.cluster().snapshot().topic(topic).is_none() {
        return Vec::new();
    }
    let may = |resource, name| {
        caller
            .access()
            .may(&cluster.id, &cluster.labels, resource, Action::View, name)
    };
    if !may(Resource::Topic, Some(topic)) || (names_groups && !may(Resource::Consumer, None)) {
        return Vec::new();
    }
    match crate::call(
        "tail",
        kafka_read::tail(admin.cluster(), &TailSpec::new(topic.to_owned(), limit)),
    )
    .await
    {
        Ok(tails) => tails
            .into_iter()
            .flat_map(|partition| partition.records)
            .map(|record| {
                (
                    record.key.map(|key| key.to_vec()),
                    record.value.map(|value| value.to_vec()),
                )
            })
            .collect(),
        Err(error) => {
            errors.push(error.into_resource_error(topic));
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use arc_swap::ArcSwap;
    use kaas_ui_auth::{Permission, Policy, Principal, Role};
    use kaas_ui_core::{Config, Registry};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    /// A Connect worker that answers `/` and the expanded connector list.
    async fn worker(connectors: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let connectors = connectors.clone();
                tokio::spawn(async move {
                    let mut buffer = vec![0_u8; 4096];
                    let read = socket.read(&mut buffer).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buffer[..read]).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("").to_owned();
                    let body = if path == "/" {
                        r#"{"version":"stub","kafka_cluster_id":"k"}"#.to_owned()
                    } else {
                        connectors
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: \
                         {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn mirror(name: &str, class: &str, source: &str, target: &str) -> String {
        format!(
            r#""{name}": {{
              "status": {{"connector": {{"state": "RUNNING"}},
                          "tasks": [{{"id": 0, "state": "RUNNING"}}], "type": "source"}},
              "info": {{"config": {{"connector.class": "org.apache.kafka.connect.mirror.{class}",
                        "source.cluster.alias": "{source}", "target.cluster.alias": "{target}",
                        "topics": "kaas-canary-v1", "groups": ".*"}}, "type": "source"}}
            }}"#
        )
    }

    async fn state(policy: Policy) -> AppState {
        // A: dedicated, strimzi → kaas, no heartbeat connector (Strimzi's v1
        // KafkaMirrorMaker2 has none). B: on the general Connect cluster,
        // kaas → strimzi, all three, beside an ordinary connector.
        let mm2 = worker(format!(
            "{{{},{}}}",
            mirror("mm2-source", "MirrorSourceConnector", "strimzi", "kaas"),
            mirror(
                "mm2-checkpoint",
                "MirrorCheckpointConnector",
                "strimzi",
                "kaas"
            ),
        ))
        .await;
        let connect = worker(format!(
            r#"{{{},{},{},"gtfs": {{"status": {{"connector": {{"state": "RUNNING"}}, "tasks": [], "type": "source"}},
               "info": {{"config": {{"connector.class": "x.Gtfs"}}, "type": "source"}}}}}}"#,
            mirror("mirror-kaas-source", "MirrorSourceConnector", "kaas", "strimzi"),
            mirror("mirror-kaas-checkpoint", "MirrorCheckpointConnector", "kaas", "strimzi"),
            mirror("mirror-kaas-heartbeat", "MirrorHeartbeatConnector", "kaas", "strimzi"),
        ))
        .await;
        let config = Config::from_yaml(&format!(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: kaas
        bootstrap: ["kaas:9092"]
      - id: strimzi
        bootstrap: ["strimzi:9092"]
    connect_clusters:
      - id: connect
        url: {connect}
        kafka_cluster: strimzi
      - id: mm2
        url: {mm2}
        kafka_cluster: kaas
"#
        ))
        .unwrap();
        AppState::new(
            Arc::new(ArcSwap::from_pointee(
                Registry::from_config(&config).unwrap(),
            )),
            policy,
        )
    }

    fn caller(state: &AppState, principal: Principal) -> Caller {
        let access = state.policy().access(&principal);
        Caller::new(principal, access)
    }

    #[tokio::test]
    async fn two_flows_across_two_connect_clusters_each_piece_where_it_runs() {
        let state = state(Policy::open()).await;
        let Json(list) = list(
            State(state.clone()),
            caller(&state, Principal::anonymous()),
            Path("dev".into()),
        )
        .await
        .unwrap();
        assert!(list.errors.is_empty(), "{:?}", list.errors);
        assert_eq!(list.flows.len(), 2);

        let a = list
            .flows
            .iter()
            .find(|flow| flow.source_alias == "strimzi")
            .unwrap();
        assert_eq!(a.target_alias, "kaas");
        assert!(a.connectors.iter().all(|c| c.connect_cluster == "mm2"));
        assert_eq!(a.missing, [MirrorKind::Heartbeat]);
        assert_eq!(a.source.as_ref().unwrap().id, "strimzi");
        assert_eq!(a.target.as_ref().unwrap().id, "kaas");
        assert!(!a.discovered);

        let b = list
            .flows
            .iter()
            .find(|flow| flow.source_alias == "kaas")
            .unwrap();
        assert!(b.missing.is_empty());
        assert_eq!(
            b.connectors.len(),
            3,
            "the ordinary connector is not a mirror"
        );
        assert!(b.connectors.iter().all(|c| c.connect_cluster == "connect"));
        assert_eq!(b.topics.as_deref(), Some("kaas-canary-v1"));
        assert_eq!(b.groups.as_deref(), Some(".*"));
        assert!(matches!(b.policy, Some(ReplicationPolicy::Default { .. })));
    }

    #[tokio::test]
    async fn a_flow_with_an_end_the_caller_cannot_see_is_not_listed() {
        // Someone who may view topics on kaas only: both flows touch strimzi.
        let policy = Policy::enforcing(vec![Role {
            name: "kaas-only".into(),
            subjects: vec!["alice".into()],
            clusters: vec!["kaas".into()],
            cluster_labels: BTreeMap::new(),
            permissions: vec![Permission {
                resource: Resource::Topic,
                value: None,
                actions: vec![Action::View],
            }],
        }]);
        let state = state(policy).await;
        let Json(list) = list(
            State(state.clone()),
            caller(&state, Principal::new("alice")),
            Path("dev".into()),
        )
        .await
        .unwrap();
        assert!(list.flows.is_empty(), "{:?}", list.flows);
    }

    #[tokio::test]
    async fn a_role_scoped_to_one_topic_sees_no_flow() {
        // The deployed canary-reader: one topic on every cluster, nothing
        // else. No `connect: view`, so no Connect-found flow; and no view of
        // the internal topics by name, so nothing discovered from them either.
        let policy = Policy::enforcing(vec![Role {
            name: "canary-reader".into(),
            subjects: vec!["bob".into()],
            clusters: vec!["*".into()],
            cluster_labels: BTreeMap::new(),
            permissions: vec![Permission {
                resource: Resource::Topic,
                value: Some("kaas-canary-v1".into()),
                actions: vec![Action::View, Action::MessagesRead],
            }],
        }]);
        let state = state(policy).await;
        let Json(list) = list(
            State(state.clone()),
            caller(&state, Principal::new("bob")),
            Path("dev".into()),
        )
        .await
        .unwrap();
        assert!(list.flows.is_empty(), "{:?}", list.flows);
    }
}
