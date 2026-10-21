//! What a Connect worker sends, as it sends it.
//!
//! Private to the crate. These are the shapes of Apache Kafka's Connect REST
//! API (`kafka.apache.org/42/generated/connect_rest.yaml`) and nothing outside
//! this crate sees them: the public types in [`crate::model`] are kaas-ui's
//! own, converted at the boundary, so a worker that grows a field changes this
//! file and not the generated TypeScript client.
//!
//! Every field a worker might leave out is `Option` or defaulted. Connect has
//! no version negotiation, so an older worker simply omits what it does not
//! have — `version` on a status, `trace` everywhere — and a deserializer that
//! insisted would turn that absence into a failed page.

use std::collections::BTreeMap;

use serde::Deserialize;

/// `GET /`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ServerInfo {
    pub version: Option<String>,
    pub commit: Option<String>,
    pub kafka_cluster_id: Option<String>,
}

/// One entry of `GET /connectors?expand=status&expand=info`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Expanded {
    pub status: Option<StateInfo>,
    pub info: Option<ConnectorInfo>,
}

/// `GET /connectors/{c}/status`, and the `status` half of the expanded list.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct StateInfo {
    pub connector: Option<State>,
    #[serde(default)]
    pub tasks: Vec<TaskState>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// A connector's own state.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct State {
    pub state: Option<String>,
    pub worker_id: Option<String>,
    pub trace: Option<String>,
    pub version: Option<String>,
}

/// One task's state.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TaskState {
    pub id: i32,
    pub state: Option<String>,
    pub worker_id: Option<String>,
    pub trace: Option<String>,
    pub version: Option<String>,
}

/// `GET /connectors/{c}`, and the `info` half of the expanded list.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConnectorInfo {
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// One element of `GET /connectors/{c}/tasks`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TaskInfo {
    pub id: TaskId,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
}

/// A task's address.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TaskId {
    pub task: i32,
}

/// `GET /connectors/{c}/topics` is `{name: {topics: [...]}}`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ActiveTopics {
    #[serde(default)]
    pub topics: Vec<String>,
}

/// `GET /connectors/{c}/offsets`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Offsets {
    #[serde(default)]
    pub offsets: Vec<Offset>,
}

/// One stored offset. Both halves are connector-defined maps: a sink's is
/// `{kafka_topic, kafka_partition}` → `{kafka_offset}`, a source's is whatever
/// the plugin chose.
///
/// `offset` can be an explicit `null` — a source partition whose position was
/// cleared, which the Strimzi worker's GTFS connector reports for every file
/// it has finished. `#[serde(default)]` does not cover a present null, so it
/// is an `Option`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Offset {
    #[serde(default)]
    pub partition: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub offset: Option<BTreeMap<String, serde_json::Value>>,
}

/// One element of `GET /connector-plugins`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PluginInfo {
    #[serde(rename = "class")]
    pub class: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub version: Option<String>,
}

/// One element of `GET /connector-plugins/{p}/config`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConfigKeyInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub group: Option<String>,
    pub documentation: Option<String>,
}

/// The body of every Connect error.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ErrorBody {
    pub message: Option<String>,
}
