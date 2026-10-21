//! The Connect REST call, as `curl`: what kaas-ui itself would send.

use kaas_ui_auth::Operation;

use super::{Detail, Equivalent, shell};

const NOTE: &str = "The Connect REST call kaas-ui would send. Withheld values are shown as ******.";

pub(super) fn render(
    operation: Operation,
    connector: &str,
    url: &str,
    detail: Detail<'_>,
) -> Equivalent {
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
    let text = match operation {
        Operation::ConnectorRestart => {
            let (tasks, failed) = detail.restart.unwrap_or((true, true));
            curl(
                "POST",
                &format!("/restart?includeTasks={tasks}&onlyFailed={failed}"),
            )
        }
        Operation::ConnectorRestartTask => curl(
            "POST",
            &format!("/tasks/{}/restart", detail.task.unwrap_or(0)),
        ),
        Operation::ConnectorPause => curl("PUT", "/pause"),
        Operation::ConnectorResume => curl("PUT", "/resume"),
        Operation::ConnectorStop => curl("PUT", "/stop"),
        Operation::ConnectorResetTopics => curl("PUT", "/topics/reset"),
        Operation::ConnectorEdit => with_body(
            "PUT",
            &format!("{base}/config"),
            &serde_json::Value::Object(config()),
        ),
        Operation::ConnectorCreate => with_body(
            "POST",
            &format!("{}/connectors", url.trim_end_matches('/')),
            &serde_json::json!({ "name": connector, "config": config() }),
        ),
        Operation::ConnectorDelete => curl("DELETE", ""),
        Operation::ConnectorResetOffsets => curl("DELETE", "/offsets"),
        Operation::ConnectorAlterOffsets => with_body(
            "PATCH",
            &format!("{base}/offsets"),
            &serde_json::json!({ "offsets": detail.offsets.unwrap_or_default() }),
        ),
    };
    shell(text, NOTE)
}

#[cfg(test)]
mod tests {
    use super::super::tests::entry;
    use super::super::{Format, equivalent, equivalent_with};
    use super::*;

    #[test]
    fn a_stop_is_the_put_and_an_edit_carries_the_redacted_config() {
        let stop = equivalent(
            Operation::ConnectorStop,
            "gtfs",
            "http://c:8083/",
            Format::Rest,
        );
        assert_eq!(
            stop.text,
            "curl -X PUT 'http://c:8083/connectors/gtfs/stop'"
        );

        let config = [
            entry("connector.class", "io.x.MqttSource", false),
            entry("connect.mqtt.password", "******", true),
        ];
        let edit = equivalent_with(
            Operation::ConnectorEdit,
            "mqtt",
            "http://c",
            Format::Rest,
            Detail {
                config: Some(&config),
                ..Detail::default()
            },
        );
        assert!(edit.text.starts_with("curl -X PUT"));
        assert!(edit.text.contains("******"));
    }
}
