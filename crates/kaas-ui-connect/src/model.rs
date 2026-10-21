//! kaas-ui's own view of a Connect cluster. What the API serialises.

use std::collections::BTreeMap;

use serde::Serialize;
use utoipa::ToSchema;

use crate::redact::{ConnectorConfigEntry, Redaction};
use crate::wire;

/// What `GET /` says about the workers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    /// The Connect release, as the worker reports it. Displayed, never
    /// compared: no Kafka version number decides anything in kaas-ui.
    pub version: Option<String>,
    /// The build commit.
    pub commit: Option<String>,
    /// The Kafka cluster the workers keep their state in.
    ///
    /// What a `connect_clusters` entry's `kafka_cluster:` reference is checked
    /// against.
    pub kafka_cluster_id: Option<String>,
}

impl From<wire::ServerInfo> for ServerInfo {
    fn from(info: wire::ServerInfo) -> Self {
        Self {
            version: info.version,
            commit: info.commit,
            kafka_cluster_id: info.kafka_cluster_id,
        }
    }
}

/// Which way data flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ConnectorType {
    /// Reads from somewhere else and writes to Kafka.
    Source,
    /// Reads from Kafka and writes somewhere else.
    Sink,
    /// The worker did not say, or said something else.
    Unknown,
}

impl ConnectorType {
    fn of(raw: Option<&str>) -> Self {
        match raw {
            Some("source") => Self::Source,
            Some("sink") => Self::Sink,
            _ => Self::Unknown,
        }
    }
}

/// A connector's state, derived from its own and its tasks'.
///
/// Connect reports one state for the connector and one per task, and neither
/// alone says whether data is moving: a `RUNNING` connector whose every task
/// has failed is moving nothing. So the two are folded into one, and the fold
/// keeps apart the states that a green/red rendering would merge — above all
/// **paused is not failed**, because somebody chose it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ConnectorStatus {
    /// Running, and every task is running.
    Running,
    /// Running, and some tasks have failed.
    Degraded,
    /// The connector failed, or every task did.
    Failed,
    /// Paused on purpose. Tasks hold their resources and process nothing.
    Paused,
    /// Stopped on purpose. Tasks are shut down.
    Stopped,
    /// Not yet assigned to a worker — usually a rebalance in progress.
    Unassigned,
    /// Restarting.
    Restarting,
    /// Running with zero tasks: configured, and doing nothing. Its own state
    /// because it is a misconfiguration rather than a health.
    NoTasks,
    /// The worker reported something this fold does not know.
    Unknown,
}

impl ConnectorStatus {
    /// Fold a connector's state and its tasks' into one.
    #[must_use]
    pub fn derive(connector: Option<&str>, tasks: &[&str]) -> Self {
        let failed = tasks.iter().filter(|state| **state == "FAILED").count();
        let all_failed = !tasks.is_empty() && failed == tasks.len();
        let any = |wanted: &str| tasks.contains(&wanted);
        match connector {
            Some("FAILED") => Self::Failed,
            Some("PAUSED") => Self::Paused,
            Some("STOPPED") => Self::Stopped,
            _ if all_failed => Self::Failed,
            Some("RESTARTING") => Self::Restarting,
            _ if any("RESTARTING") => Self::Restarting,
            Some("UNASSIGNED") => Self::Unassigned,
            _ if any("UNASSIGNED") => Self::Unassigned,
            Some("RUNNING") if tasks.is_empty() => Self::NoTasks,
            Some("RUNNING") if failed > 0 => Self::Degraded,
            Some("RUNNING") if tasks.iter().all(|state| *state == "RUNNING") => Self::Running,
            _ => Self::Unknown,
        }
    }
}

/// One task's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    /// The task number.
    pub id: i32,
    /// `RUNNING`, `FAILED`, `PAUSED`, `UNASSIGNED`, `RESTARTING`, verbatim.
    pub state: Option<String>,
    /// The worker running it.
    pub worker_id: Option<String>,
    /// The stack trace of a failed task. Shown unredacted: masking a stack
    /// trace hides the error, and that residual is documented, not regexed.
    pub trace: Option<String>,
    /// The plugin version this task runs. Displayed, never compared.
    pub version: Option<String>,
}

impl From<wire::TaskState> for TaskStatus {
    fn from(task: wire::TaskState) -> Self {
        Self {
            id: task.id,
            state: task.state,
            worker_id: task.worker_id,
            trace: task.trace,
            version: task.version,
        }
    }
}

/// One row of the connector table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorSummary {
    /// The connector name.
    pub name: String,
    /// `connector.class`, where the config names one.
    pub class: Option<String>,
    /// Source or sink.
    #[serde(rename = "type")]
    pub kind: ConnectorType,
    /// The derived state.
    pub status: ConnectorStatus,
    /// The connector's own state, verbatim.
    pub state: Option<String>,
    /// The worker running the connector.
    pub worker_id: Option<String>,
    /// The connector's stack trace, where it failed.
    pub trace: Option<String>,
    /// The plugin version. Displayed, never compared.
    pub version: Option<String>,
    /// Every task, in id order.
    pub tasks: Vec<TaskStatus>,
    /// How many tasks are running.
    pub tasks_running: usize,
    /// How many tasks have failed.
    pub tasks_failed: usize,
    /// The consumer group a sink commits to.
    ///
    /// `connect-<name>` unless the config overrides it with
    /// `consumer.override.group.id`. `None` for a source, which has none.
    pub consumer_group: Option<String>,
}

impl ConnectorSummary {
    /// Build from a status and the config beside it.
    pub(crate) fn of(
        name: &str,
        status: Option<wire::StateInfo>,
        config: Option<&BTreeMap<String, String>>,
        info_kind: Option<&str>,
    ) -> Self {
        let (connector, tasks, status_kind) = match status {
            Some(status) => (status.connector, status.tasks, status.kind),
            None => (None, Vec::new(), None),
        };
        let mut tasks: Vec<TaskStatus> = tasks.into_iter().map(TaskStatus::from).collect();
        tasks.sort_by_key(|task| task.id);
        let states: Vec<&str> = tasks
            .iter()
            .map(|task| task.state.as_deref().unwrap_or(""))
            .collect();
        let state = connector.as_ref().and_then(|c| c.state.clone());
        let kind = ConnectorType::of(status_kind.as_deref().or(info_kind));
        let consumer_group = (kind == ConnectorType::Sink).then(|| {
            config
                .and_then(|config| config.get("consumer.override.group.id"))
                .cloned()
                .unwrap_or_else(|| format!("connect-{name}"))
        });
        Self {
            name: name.to_owned(),
            class: config
                .and_then(|config| config.get("connector.class"))
                .cloned(),
            kind,
            status: ConnectorStatus::derive(state.as_deref(), &states),
            tasks_running: states.iter().filter(|s| **s == "RUNNING").count(),
            tasks_failed: states.iter().filter(|s| **s == "FAILED").count(),
            state,
            worker_id: connector.as_ref().and_then(|c| c.worker_id.clone()),
            trace: connector.as_ref().and_then(|c| c.trace.clone()),
            version: connector.and_then(|c| c.version),
            tasks,
            consumer_group,
        }
    }
}

/// A connector config, redacted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RedactedConfig {
    /// Every key, in key order.
    pub entries: Vec<ConnectorConfigEntry>,
    /// Which redaction applied.
    pub redaction: Redaction,
}

/// One task's config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskConfig {
    /// The task number.
    pub id: i32,
    /// Its config, redacted by the same rules as the connector's.
    pub config: RedactedConfig,
}

/// One connector, in full.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorDetail {
    /// State, tasks, class, consumer group.
    pub summary: ConnectorSummary,
    /// The connector config.
    pub config: RedactedConfig,
    /// Each task's generated config.
    pub task_configs: Vec<TaskConfig>,
}

/// One stored offset.
///
/// Both halves are maps the connector defines: a sink's partition is
/// `{kafka_topic, kafka_partition}` and its offset `{kafka_offset}`; a
/// source's are whatever its plugin chose, which is why they are passed
/// through rather than typed.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StoredOffset {
    /// Which source partition this is.
    #[schema(value_type = Object)]
    pub partition: BTreeMap<String, serde_json::Value>,
    /// The position within it. `None` where the worker reports `null`: a
    /// partition whose stored position was cleared, which is not the same as
    /// a position of zero.
    #[schema(value_type = Option<Object>)]
    pub offset: Option<BTreeMap<String, serde_json::Value>>,
}

impl From<wire::Offset> for StoredOffset {
    fn from(offset: wire::Offset) -> Self {
        Self {
            partition: offset.partition.unwrap_or_default(),
            offset: offset.offset,
        }
    }
}

/// What the plugin made of a config, without applying it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Validation {
    /// How many keys the plugin found fault with.
    pub error_count: i32,
    /// Each key with something wrong, and what.
    pub errors: Vec<ValidationError>,
}

/// One key the plugin refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationError {
    /// The key.
    pub key: String,
    /// What the plugin said about it, verbatim.
    pub messages: Vec<String>,
}

impl From<wire::ConfigInfos> for Validation {
    fn from(infos: wire::ConfigInfos) -> Self {
        Self {
            error_count: infos.error_count,
            errors: infos
                .configs
                .into_iter()
                .filter_map(|info| info.value)
                .filter(|value| !value.errors.is_empty())
                .map(|value| ValidationError {
                    key: value.name,
                    messages: value.errors,
                })
                .collect(),
        }
    }
}

/// One installed connector plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    /// The class.
    pub class: String,
    /// Source or sink.
    #[serde(rename = "type")]
    pub kind: ConnectorType,
    /// The plugin's version. Displayed, never compared.
    pub version: Option<String>,
}

impl From<wire::PluginInfo> for Plugin {
    fn from(plugin: wire::PluginInfo) -> Self {
        Self {
            kind: ConnectorType::of(plugin.kind.as_deref()),
            class: plugin.class,
            version: plugin.version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ConnectorStatus as S;

    #[test]
    fn paused_is_not_failed_and_running_is_not_healthy_on_its_own() {
        assert_eq!(S::derive(Some("PAUSED"), &["FAILED"]), S::Paused);
        assert_eq!(S::derive(Some("STOPPED"), &[]), S::Stopped);
        assert_eq!(S::derive(Some("RUNNING"), &["FAILED", "FAILED"]), S::Failed);
        assert_eq!(
            S::derive(Some("RUNNING"), &["RUNNING", "FAILED"]),
            S::Degraded
        );
        assert_eq!(
            S::derive(Some("RUNNING"), &["RUNNING", "RUNNING"]),
            S::Running
        );
        assert_eq!(S::derive(Some("RUNNING"), &[]), S::NoTasks);
        assert_eq!(S::derive(Some("FAILED"), &["RUNNING"]), S::Failed);
        assert_eq!(
            S::derive(Some("RUNNING"), &["RUNNING", "UNASSIGNED"]),
            S::Unassigned
        );
        assert_eq!(S::derive(Some("RUNNING"), &["RESTARTING"]), S::Restarting);
        assert_eq!(S::derive(Some("SOMETHING_NEW"), &["RUNNING"]), S::Unknown);
        assert_eq!(S::derive(None, &[]), S::Unknown);
    }
}
