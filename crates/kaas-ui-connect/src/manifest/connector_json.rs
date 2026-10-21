//! A connector as a file: the `{name, config}` document `POST /connectors`
//! takes, kept one per connector in Git and applied by a pipeline.
//!
//! The file holds the config, and its presence is the connector's existence.
//! Everything else — running state, offsets, a restart — is in no such file,
//! and renders as the REST call.

use kaas_ui_auth::Operation;

use super::{Detail, Equivalent, entries_except, fallback, json};

pub(super) fn render(
    operation: Operation,
    connector: &str,
    url: &str,
    detail: Detail<'_>,
) -> Equivalent {
    match operation {
        Operation::ConnectorEdit | Operation::ConnectorCreate => {
            // `name` is the document's own key; a copy inside `config` is
            // Connect's to fill in.
            let config = entries_except(detail, &["name"])
                .map(|entry| {
                    (
                        entry.key.clone(),
                        serde_json::Value::from(entry.value.clone()),
                    )
                })
                .collect::<serde_json::Map<_, _>>();
            let withheld = detail
                .config
                .unwrap_or_default()
                .iter()
                .any(|entry| entry.redacted);
            let document = serde_json::json!({ "name": connector, "config": config });
            let mut note = if operation == Operation::ConnectorCreate {
                "A new connector file, for wherever your pipeline applies connectors from."
                    .to_owned()
            } else {
                "The connector's file as this edit would leave it.".to_owned()
            };
            if withheld {
                note.push_str(
                    " Values shown as ****** are withheld: reference a config provider there \
                     (${file:…}, ${env:…}), never the secret.",
                );
            }
            json(
                serde_json::to_string_pretty(&document).unwrap_or_default(),
                &note,
            )
        }
        Operation::ConnectorDelete => fallback(
            operation,
            connector,
            url,
            detail,
            "Remove the connector's file from Git, or the pipeline puts the connector back. \
             If it does not delete what it no longer finds, this is the call that does.",
        ),
        Operation::ConnectorRestart
        | Operation::ConnectorRestartTask
        | Operation::ConnectorPause
        | Operation::ConnectorResume
        | Operation::ConnectorStop
        | Operation::ConnectorResetTopics
        | Operation::ConnectorResetOffsets
        | Operation::ConnectorAlterOffsets => fallback(
            operation,
            connector,
            url,
            detail,
            "No connector file says this; the Connect REST call kaas-ui would send.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::entry;
    use super::super::{Format, equivalent, equivalent_with};
    use super::*;

    #[test]
    fn an_edit_is_the_file_and_a_pause_is_the_call() {
        let config = [
            entry("connector.class", "io.x.MqttSource", false),
            entry("name", "mqtt", false),
            entry("tasks.max", "2", false),
            entry("connect.mqtt.password", "******", true),
        ];
        let edit = equivalent_with(
            Operation::ConnectorEdit,
            "mqtt",
            "http://c",
            Format::ConnectorJson,
            Detail {
                config: Some(&config),
                ..Detail::default()
            },
        );
        assert_eq!(edit.language, "json");
        let document: serde_json::Value = serde_json::from_str(&edit.text).unwrap();
        assert_eq!(document["name"], "mqtt");
        assert_eq!(document["config"]["tasks.max"], "2");
        assert_eq!(document["config"]["connect.mqtt.password"], "******");
        assert!(document["config"].get("name").is_none());
        assert!(edit.note.contains("config provider"), "{}", edit.note);

        let pause = equivalent(
            Operation::ConnectorPause,
            "mqtt",
            "http://c",
            Format::ConnectorJson,
        );
        assert_eq!(pause.text, "curl -X PUT 'http://c/connectors/mqtt/pause'");
        assert!(pause.note.contains("No connector file"), "{}", pause.note);
    }
}
