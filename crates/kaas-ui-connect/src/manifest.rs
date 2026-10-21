//! What a Connect write would be in Git — or, failing Git, in a terminal.
//!
//! GitOps first: every write form shows its equivalent, whether or not the
//! caller may apply it, and a refusal because Git owns the connector shows
//! the change to make there instead. Pure functions; nothing here is sent.

use kaas_ui_auth::Operation;
use serde::Serialize;
use utoipa::ToSchema;

/// How to render a write's equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// The REST call, as `curl`.
    Plain,
    /// Strimzi: a `KafkaConnector` patch for declared state, the annotation
    /// for a restart.
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

/// The equivalent of `operation` on `connector`, at the Connect REST `url`.
#[must_use]
pub fn equivalent(operation: Operation, connector: &str, url: &str, format: Format) -> Equivalent {
    let base = url.trim_end_matches('/');
    match (format, operation) {
        (Format::Strimzi, Operation::ConnectorRestart) => Equivalent {
            language: "shell".into(),
            text: format!("kubectl annotate kafkaconnector {connector} strimzi.io/restart=true"),
            note: "A restart is not declared state: Strimzi models it as an annotation the \
                   operator acts on and removes."
                .into(),
        },
        (Format::Strimzi, state_change) => Equivalent {
            language: "yaml".into(),
            text: format!(
                "apiVersion: kafka.strimzi.io/v1\nkind: KafkaConnector\nmetadata:\n  name: \
                 {connector}\nspec:\n  state: {}\n",
                match state_change {
                    Operation::ConnectorPause => "paused",
                    Operation::ConnectorStop => "stopped",
                    _ => "running",
                }
            ),
            note: "Declared state: change `spec.state` in the KafkaConnector resource in Git, or \
                   the operator will put back whatever was changed here."
                .into(),
        },
        (Format::Plain, operation) => {
            let (method, path) = match operation {
                Operation::ConnectorRestart => {
                    ("POST", "restart?includeTasks=true&onlyFailed=true")
                }
                Operation::ConnectorPause => ("PUT", "pause"),
                Operation::ConnectorResume => ("PUT", "resume"),
                Operation::ConnectorStop => ("PUT", "stop"),
            };
            Equivalent {
                language: "shell".into(),
                text: format!("curl -X {method} '{base}/connectors/{connector}/{path}'"),
                note: "The Connect REST call this performs.".into(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
