//! What a Connect write would be in Git — or, failing Git, in a terminal.
//!
//! GitOps first: every write form shows its equivalent, whether or not the
//! caller may apply it, and a refusal because Git owns the connector shows
//! the change to make there instead. Pure functions; nothing here is sent.
//!
//! **One renderer per way of keeping connectors in Git**, chosen per Connect
//! cluster by `writes.gitops.format` and never guessed from a URL or a name:
//!
//! * [`Format::Rest`] — the call kaas-ui makes. True of every Connect
//!   cluster, and the default;
//! * [`Format::ConnectorJson`] — the `{name, config}` document Connect's own
//!   `POST /connectors` takes, which is what most config-as-code pipelines
//!   keep a file of per connector and apply;
//! * [`Format::StrimziKafkaConnector`] — Strimzi's `KafkaConnector` resource.
//!
//! What a format has no representation for — no connector file says "restart
//! task 3" — renders as the REST call, with a note saying so. A format also
//! says which [`Aspect`]s Git declares when the config does not say
//! otherwise, because it knows what its files can hold.
//!
//! A config rendered here is always the **redacted** one. A withheld value is
//! written as `******` with a note saying where it belongs — a config
//! provider reference — because a file in Git is the last place a secret
//! should be pasted.

use kaas_ui_auth::{Aspect, Operation};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::model::StoredOffset;
use crate::redact::ConnectorConfigEntry;

mod connector_json;
mod rest;
mod strimzi;

/// How Git holds a Connect cluster's connectors, and so how a write's
/// equivalent is rendered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", try_from = "String")]
pub enum Format {
    /// Not as files kaas-ui can render: show the Connect REST call.
    #[default]
    Rest,
    /// One `{name, config}` JSON document per connector, applied by a
    /// pipeline.
    ConnectorJson,
    /// Strimzi `KafkaConnector` resources, reconciled by its operator.
    StrimziKafkaConnector,
}

impl Format {
    /// What Git declares of a connector held this way, unless
    /// `writes.gitops.declares` says otherwise.
    ///
    /// A connector file holds the config, and its presence is the
    /// connector's existence; nothing in it says paused, or where the
    /// offsets are. A `KafkaConnector` declares all four: `spec.state`, and
    /// offsets through `spec.alterOffsets` and its annotation. `rest` says
    /// nothing about Git, so it assumes everything — refusing a write Git did
    /// not mind is the cheap mistake.
    #[must_use]
    pub fn declares(self) -> &'static [Aspect] {
        match self {
            Self::ConnectorJson => &[Aspect::Config, Aspect::Existence],
            Self::Rest | Self::StrimziKafkaConnector => &Aspect::ALL,
        }
    }
}

impl TryFrom<String> for Format {
    type Error = String;

    /// The two names this had before it was split by tool are refused by
    /// name, saying what they became.
    fn try_from(name: String) -> Result<Self, String> {
        match name.as_str() {
            "rest" => Ok(Self::Rest),
            "connector_json" => Ok(Self::ConnectorJson),
            "strimzi_kafka_connector" => Ok(Self::StrimziKafkaConnector),
            "plain" => Err("`format: plain` is now `format: rest`".to_owned()),
            "strimzi" => Err(
                "`format: strimzi` is now `format: strimzi_kafka_connector` — \
                              where Git holds `KafkaConnector` resources; connectors a \
                              `KafkaMirrorMaker2` generates are not among them, and are `rest`"
                    .to_owned(),
            ),
            other => Err(format!(
                "unknown format {other:?}: rest, connector_json or strimzi_kafka_connector"
            )),
        }
    }
}

/// One write's equivalent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Equivalent {
    /// What kind of text this is: `yaml` or `json` for a file, `shell` for a
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
        Format::Rest => rest::render(operation, connector, url, detail),
        Format::ConnectorJson => connector_json::render(operation, connector, url, detail),
        Format::StrimziKafkaConnector => strimzi::render(operation, connector, url, detail),
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

fn json(text: String, note: &str) -> Equivalent {
    Equivalent {
        language: "json".into(),
        text,
        note: note.into(),
    }
}

/// The REST call, for what a format cannot hold, with `why` as its note.
fn fallback(
    operation: Operation,
    connector: &str,
    url: &str,
    detail: Detail<'_>,
    why: &str,
) -> Equivalent {
    Equivalent {
        note: why.into(),
        ..rest::render(operation, connector, url, detail)
    }
}

/// A redacted config's entries without the ones a format holds elsewhere.
fn entries_except<'a>(
    detail: Detail<'a>,
    held_elsewhere: &'a [&'a str],
) -> impl Iterator<Item = &'a ConnectorConfigEntry> {
    detail
        .config
        .unwrap_or_default()
        .iter()
        .filter(move |entry| !held_elsewhere.contains(&entry.key.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn entry(key: &str, value: &str, redacted: bool) -> ConnectorConfigEntry {
        ConnectorConfigEntry {
            key: key.into(),
            value: value.into(),
            redacted,
            provider: false,
            group: None,
            documentation: None,
        }
    }

    const FORMATS: [Format; 3] = [
        Format::Rest,
        Format::ConnectorJson,
        Format::StrimziKafkaConnector,
    ];

    #[test]
    fn every_operation_renders_in_every_format() {
        for operation in Operation::ALL {
            for format in FORMATS {
                let rendered = equivalent(operation, "c", "http://w:8083", format);
                assert!(!rendered.text.is_empty(), "{operation:?} {format:?}");
                assert!(!rendered.note.is_empty(), "{operation:?} {format:?}");
            }
        }
    }

    #[test]
    fn no_format_renders_a_secret() {
        let config = [
            entry("connector.class", "io.x.MqttSource", false),
            entry("connect.mqtt.password", "******", true),
        ];
        let detail = Detail {
            config: Some(&config),
            ..Detail::default()
        };
        for format in FORMATS {
            for operation in [Operation::ConnectorEdit, Operation::ConnectorCreate] {
                let rendered = equivalent_with(operation, "mqtt", "http://c", format, detail);
                assert!(rendered.text.contains("******"), "{format:?}");
            }
        }
    }

    #[test]
    fn the_old_names_are_refused_saying_what_they_became() {
        let err = Format::try_from("strimzi".to_owned()).unwrap_err();
        assert!(err.contains("strimzi_kafka_connector"), "{err}");
        assert!(err.contains("KafkaMirrorMaker2"), "{err}");
        let err = Format::try_from("plain".to_owned()).unwrap_err();
        assert!(err.contains("format: rest"), "{err}");
        assert_eq!(
            Format::try_from("connector_json".to_owned()),
            Ok(Format::ConnectorJson)
        );
    }

    #[test]
    fn a_file_format_declares_what_its_files_hold() {
        assert_eq!(
            Format::ConnectorJson.declares(),
            [Aspect::Config, Aspect::Existence]
        );
        assert_eq!(Format::Rest.declares(), Aspect::ALL);
        assert_eq!(Format::StrimziKafkaConnector.declares(), Aspect::ALL);
    }
}
