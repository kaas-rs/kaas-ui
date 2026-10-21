//! What a Connect write would be in Git — or, failing Git, in a terminal.
//!
//! GitOps first: every write form shows its equivalent, whether or not the
//! caller may apply it, and a refusal because Git owns the connector shows
//! the change to make there instead. Pure functions; nothing here is sent.
//!
//! A config rendered here is always the **redacted** one. A withheld value is
//! written as `******` with a comment saying where it belongs — a config
//! provider reference — because a manifest in Git is the last place a secret
//! should be pasted.

use kaas_ui_auth::Operation;
use serde::Serialize;
use utoipa::ToSchema;

use crate::model::StoredOffset;
use crate::redact::ConnectorConfigEntry;

/// How to render a write's equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// The REST call, as `curl`.
    Plain,
    /// Strimzi: a `KafkaConnector` resource or patch for declared state, an
    /// annotation for the acts the operator models as one.
    Strimzi,
}

/// One write's equivalent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Equivalent {
    /// What kind of text this is: `yaml` for a manifest, `shell` for a
    /// command.
    pub language: String,
    /// The text.
    pub text: String,
    /// One line saying where it goes.
    pub note: String,
}

/// What some operations need to say more than their name.
#[derive(Debug, Clone, Copy, Default)]
pub struct Detail<'a> {
    /// The task, for a task restart.
    pub task: Option<i32>,
    /// `includeTasks` and `onlyFailed`, for a restart.
    pub restart: Option<(bool, bool)>,
    /// The config, redacted, for an edit or a create.
    pub config: Option<&'a [ConnectorConfigEntry]>,
    /// The offsets, for an alter.
    pub offsets: Option<&'a [StoredOffset]>,
}

/// The equivalent of `operation` on `connector`, with nothing more said.
#[must_use]
pub fn equivalent(operation: Operation, connector: &str, url: &str, format: Format) -> Equivalent {
    equivalent_with(operation, connector, url, format, Detail::default())
}

/// The equivalent of `operation` on `connector`, at the Connect REST `url`.
#[must_use]
pub fn equivalent_with(
    operation: Operation,
    connector: &str,
    url: &str,
    format: Format,
    detail: Detail<'_>,
) -> Equivalent {
    match format {
        Format::Strimzi => strimzi(operation, connector, url, detail),
        Format::Plain => plain(operation, connector, url, detail),
    }
}

fn shell(text: String, note: &str) -> Equivalent {
    Equivalent {
        language: "shell".into(),
        text,
        note: note.into(),
    }
}

fn yaml(text: String, note: &str) -> Equivalent {
    Equivalent {
        language: "yaml".into(),
        text,
        note: note.into(),
    }
}

fn strimzi(operation: Operation, connector: &str, url: &str, detail: Detail<'_>) -> Equivalent {
    let header = format!(
        "apiVersion: kafka.strimzi.io/v1\nkind: KafkaConnector\nmetadata:\n  name: {connector}\n"
    );
    match operation {
        Operation::ConnectorRestart => shell(
            format!("kubectl annotate kafkaconnector {connector} strimzi.io/restart=true"),
            "A restart is not declared state: Strimzi models it as an annotation the operator \
             acts on and removes.",
        ),
        Operation::ConnectorRestartTask => shell(
            format!(
                "kubectl annotate kafkaconnector {connector} strimzi.io/restart-task={}",
                detail.task.unwrap_or(0)
            ),
            "Strimzi's annotation for restarting one task; the operator acts on it and removes it.",
        ),
        Operation::ConnectorPause | Operation::ConnectorResume | Operation::ConnectorStop => yaml(
            format!(
                "{header}spec:\n  state: {}\n",
                match operation {
                    Operation::ConnectorPause => "paused",
                    Operation::ConnectorStop => "stopped",
                    _ => "running",
                }
            ),
            "Declared state: change `spec.state` in the KafkaConnector resource in Git, or the \
             operator will put back whatever was changed here.",
        ),
        Operation::ConnectorResetTopics => plain(operation, connector, url, detail),
        Operation::ConnectorEdit | Operation::ConnectorCreate => {
            let entries = detail.config.unwrap_or_default();
            let class = entries
                .iter()
                .find(|entry| entry.key == "connector.class")
                .map_or("<connector.class>", |entry| entry.value.as_str());
            let tasks = entries
                .iter()
                .find(|entry| entry.key == "tasks.max")
                .map_or("1", |entry| entry.value.as_str());
            let mut text = header;
            if operation == Operation::ConnectorCreate {
                text.push_str(
                    "  labels:\n    strimzi.io/cluster: <the KafkaConnect resource's name>\n",
                );
            }
            text.push_str(&format!(
                "spec:\n  class: {class}\n  tasksMax: {tasks}\n  config:\n"
            ));
            for entry in entries {
                if matches!(entry.key.as_str(), "connector.class" | "tasks.max" | "name") {
                    continue;
                }
                let value = serde_json::to_string(&entry.value).unwrap_or_default();
                if entry.redacted {
                    text.push_str(&format!(
                        "    {}: {value}  # withheld — reference a config provider, never paste \
                         the secret\n",
                        entry.key
                    ));
                } else {
                    text.push_str(&format!("    {}: {value}\n", entry.key));
                }
            }
            yaml(
                text,
                if operation == Operation::ConnectorCreate {
                    "A new KafkaConnector resource, for the Git directory the operator watches."
                } else {
                    "The KafkaConnector resource's `spec` as this edit would leave it."
                },
            )
        }
        Operation::ConnectorDelete => yaml(
            header,
            "Delete this KafkaConnector's manifest from Git; the operator deletes the connector.",
        ),
        Operation::ConnectorResetOffsets => shell(
            format!(
                "kubectl annotate kafkaconnector {connector} strimzi.io/connector-offsets=reset"
            ),
            "Strimzi resets offsets on this annotation, and only for a connector whose \
             `spec.state` is `stopped` in Git.",
        ),
        Operation::ConnectorAlterOffsets => {
            let offsets = detail
                .offsets
                .map(|offsets| serde_json::json!({ "offsets": offsets }))
                .unwrap_or_default();
            let json = serde_json::to_string_pretty(&offsets).unwrap_or_default();
            let indented = json
                .lines()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            yaml(
                format!(
                    "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: {connector}-offsets\n\
                     data:\n  offsets.json: |\n{indented}\n---\n{header}spec:\n  state: stopped\n  \
                     alterOffsets:\n    fromConfigMap:\n      name: {connector}-offsets\n"
                ),
                "Commit both, then annotate the KafkaConnector with \
                 `strimzi.io/connector-offsets=alter`. Stopped connectors only.",
            )
        }
    }
}

fn plain(operation: Operation, connector: &str, url: &str, detail: Detail<'_>) -> Equivalent {
    let base = format!("{}/connectors/{connector}", url.trim_end_matches('/'));
    let curl = |method: &str, path: &str| format!("curl -X {method} '{base}{path}'");
    let with_body = |method: &str, target: &str, body: &serde_json::Value| {
        format!(
            "curl -X {method} -H 'content-type: application/json' '{target}' \\\n  -d '{}'",
            serde_json::to_string(body).unwrap_or_default()
        )
    };
    let config = || {
        detail
            .config
            .unwrap_or_default()
            .iter()
            .map(|entry| {
                (
                    entry.key.clone(),
                    serde_json::Value::from(entry.value.clone()),
                )
            })
            .collect::<serde_json::Map<String, serde_json::Value>>()
    };
    let note = "The Connect REST call this performs. Withheld values are shown as ******.";
    match operation {
        Operation::ConnectorRestart => {
            let (tasks, failed) = detail.restart.unwrap_or((true, true));
            shell(
                curl(
                    "POST",
                    &format!("/restart?includeTasks={tasks}&onlyFailed={failed}"),
                ),
                note,
            )
        }
        Operation::ConnectorRestartTask => shell(
            curl(
                "POST",
                &format!("/tasks/{}/restart", detail.task.unwrap_or(0)),
            ),
            note,
        ),
        Operation::ConnectorPause => shell(curl("PUT", "/pause"), note),
        Operation::ConnectorResume => shell(curl("PUT", "/resume"), note),
        Operation::ConnectorStop => shell(curl("PUT", "/stop"), note),
        Operation::ConnectorResetTopics => shell(
            curl("PUT", "/topics/reset"),
            "The Connect REST call this performs; no operator models it as a resource.",
        ),
        Operation::ConnectorEdit => shell(
            with_body(
                "PUT",
                &format!("{base}/config"),
                &serde_json::Value::Object(config()),
            ),
            note,
        ),
        Operation::ConnectorCreate => shell(
            with_body(
                "POST",
                &format!("{}/connectors", url.trim_end_matches('/')),
                &serde_json::json!({ "name": connector, "config": config() }),
            ),
            note,
        ),
        Operation::ConnectorDelete => shell(curl("DELETE", ""), note),
        Operation::ConnectorResetOffsets => shell(curl("DELETE", "/offsets"), note),
        Operation::ConnectorAlterOffsets => shell(
            with_body(
                "PATCH",
                &format!("{base}/offsets"),
                &serde_json::json!({ "offsets": detail.offsets.unwrap_or_default() }),
            ),
            note,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, value: &str, redacted: bool) -> ConnectorConfigEntry {
        ConnectorConfigEntry {
            key: key.into(),
            value: value.into(),
            redacted,
            provider: false,
            group: None,
            documentation: None,
        }
    }

    #[test]
    fn a_pause_under_strimzi_is_spec_state_and_a_restart_is_the_annotation() {
        let pause = equivalent(
            Operation::ConnectorPause,
            "gtfs",
            "http://c:8083",
            Format::Strimzi,
        );
        assert_eq!(pause.language, "yaml");
        assert!(
            pause.text.contains("kind: KafkaConnector") && pause.text.contains("state: paused")
        );

        let restart = equivalent(
            Operation::ConnectorRestart,
            "gtfs",
            "http://c:8083",
            Format::Strimzi,
        );
        assert!(restart.text.contains("strimzi.io/restart=true"));

        let plain = equivalent(
            Operation::ConnectorStop,
            "gtfs",
            "http://c:8083/",
            Format::Plain,
        );
        assert_eq!(
            plain.text,
            "curl -X PUT 'http://c:8083/connectors/gtfs/stop'"
        );
    }

    #[test]
    fn an_edit_renders_the_redacted_config_and_never_a_secret() {
        let config = [
            entry("connector.class", "io.x.MqttSource", false),
            entry("tasks.max", "2", false),
            entry("connect.mqtt.password", "******", true),
            entry("connect.mqtt.hosts", "tcp://h:1883", false),
        ];
        let detail = Detail {
            config: Some(&config),
            ..Detail::default()
        };
        let yaml = equivalent_with(
            Operation::ConnectorEdit,
            "mqtt",
            "u",
            Format::Strimzi,
            detail,
        );
        assert!(yaml.text.contains("class: io.x.MqttSource"));
        assert!(yaml.text.contains("tasksMax: 2"));
        assert!(
            yaml.text
                .contains("connect.mqtt.password: \"******\"  # withheld")
        );
        assert!(yaml.text.contains("connect.mqtt.hosts: \"tcp://h:1883\""));

        let curl = equivalent_with(
            Operation::ConnectorEdit,
            "mqtt",
            "http://c",
            Format::Plain,
            detail,
        );
        assert!(curl.text.starts_with("curl -X PUT"));
        assert!(curl.text.contains("******"));
    }

    #[test]
    fn every_operation_renders_in_both_formats() {
        for operation in Operation::ALL {
            for format in [Format::Plain, Format::Strimzi] {
                let rendered = equivalent(operation, "c", "http://w:8083", format);
                assert!(!rendered.text.is_empty(), "{operation:?} {format:?}");
                assert!(!rendered.note.is_empty(), "{operation:?} {format:?}");
            }
        }
    }
}
