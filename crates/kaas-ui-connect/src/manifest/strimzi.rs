//! Strimzi's `KafkaConnector` resource, for a `KafkaConnect` with
//! `strimzi.io/use-connector-resources: "true"`.
//!
//! Declared state — config, `spec.state`, offsets through `alterOffsets` —
//! is the resource; the acts Strimzi models as an annotation the operator
//! acts on and removes are the annotation. Not for connectors a
//! `KafkaMirrorMaker2` generates: they have no `KafkaConnector`, and a
//! Connect cluster running them is `rest`.

use kaas_ui_auth::Operation;

use super::{Detail, Equivalent, entries_except, fallback, shell, yaml};

pub(super) fn render(
    operation: Operation,
    connector: &str,
    url: &str,
    detail: Detail<'_>,
) -> Equivalent {
    let header = format!(
        "apiVersion: kafka.strimzi.io/v1\nkind: KafkaConnector\nmetadata:\n  name: {connector}\n"
    );
    match operation {
        Operation::ConnectorRestart => shell(
            format!("kubectl annotate kafkaconnector {connector} strimzi.io/restart=true"),
            "Strimzi models a restart as an annotation the operator acts on and removes.",
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
            "`spec.state` in the KafkaConnector resource.",
        ),
        Operation::ConnectorResetTopics => fallback(
            operation,
            connector,
            url,
            detail,
            "Strimzi has no model for this; the Connect REST call kaas-ui would send.",
        ),
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
            for entry in entries_except(detail, &["connector.class", "tasks.max", "name"]) {
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

#[cfg(test)]
mod tests {
    use super::super::tests::entry;
    use super::super::{Format, equivalent, equivalent_with};
    use super::*;

    #[test]
    fn a_pause_is_spec_state_and_a_restart_is_the_annotation() {
        let pause = equivalent(
            Operation::ConnectorPause,
            "gtfs",
            "http://c:8083",
            Format::StrimziKafkaConnector,
        );
        assert_eq!(pause.language, "yaml");
        assert!(
            pause.text.contains("kind: KafkaConnector") && pause.text.contains("state: paused")
        );

        let restart = equivalent(
            Operation::ConnectorRestart,
            "gtfs",
            "http://c:8083",
            Format::StrimziKafkaConnector,
        );
        assert!(restart.text.contains("strimzi.io/restart=true"));
    }

    #[test]
    fn an_edit_renders_the_redacted_config_and_never_a_secret() {
        let config = [
            entry("connector.class", "io.x.MqttSource", false),
            entry("tasks.max", "2", false),
            entry("connect.mqtt.password", "******", true),
            entry("connect.mqtt.hosts", "tcp://h:1883", false),
        ];
        let yaml = equivalent_with(
            Operation::ConnectorEdit,
            "mqtt",
            "u",
            Format::StrimziKafkaConnector,
            Detail {
                config: Some(&config),
                ..Detail::default()
            },
        );
        assert!(yaml.text.contains("class: io.x.MqttSource"));
        assert!(yaml.text.contains("tasksMax: 2"));
        assert!(
            yaml.text
                .contains("connect.mqtt.password: \"******\"  # withheld")
        );
        assert!(yaml.text.contains("connect.mqtt.hosts: \"tcp://h:1883\""));
    }
}
