//! Connector configs, with the secrets taken out.
//!
//! A worker hands back every config value it was given, verbatim: an inline
//! `connect.mqtt.password` comes back as the password. Redaction happens
//! here, before a public type exists, so the raw value never reaches the API
//! crate, the audit log or the browser.
//!
//! Two layers, and the second is not a formality:
//!
//! 1. **The plugin's declared type.** `GET /connector-plugins/{class}/config`
//!    types every key, and `PASSWORD` is reliable where it is used. A key is
//!    secret when it — or any suffix of it on a `.` boundary — is declared
//!    `PASSWORD`. The suffix rule is what catches MirrorMaker 2, whose real
//!    keys are `source.cluster.sasl.jaas.config` while its definition lists
//!    `sasl.jaas.config`, and every `producer.override.` / `consumer.override.`
//!    a connector is given.
//! 2. **The name.** Plugins declare secrets as `STRING` often enough —
//!    `connect.mqtt.ssl.key`, an HTTP source's `headers` — that the
//!    definition alone would leak them. kafbat-ui's list, matched on the last
//!    segments so `key.converter` is not mistaken for a key.
//!
//! A config-provider reference — `${file:/opt/secrets.properties:password}` —
//! stays visible. It is a pointer, not a secret, and seeing it is how a reader
//! knows the deployment is doing this right.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use utoipa::ToSchema;

/// What a key's value renders as once it is withheld.
pub const REDACTED: &str = "******";

/// Name endings that mean "this is a secret" whatever the plugin declared.
///
/// Matched against the last one or two `.`-separated segments, lowercased:
/// `connect.mqtt.ssl.key` ends in `key`, `key.converter` ends in `converter`.
const SECRET_ENDINGS: [&str; 10] = [
    "password",
    "passphrase",
    "secret",
    "token",
    "key",
    "credentials",
    "jaas.config",
    "user.info",
    "connection.uri",
    "headers",
];

/// Name prefixes for cloud credentials, which name the secret in the middle.
const SECRET_PREFIXES: [&str; 3] = ["aws.access.", "aws.secret.", "aws.session."];

/// One config key, as it may be shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorConfigEntry {
    /// The key, verbatim.
    pub key: String,
    /// The value, or [`REDACTED`] where it was withheld.
    pub value: String,
    /// Whether the value was withheld.
    pub redacted: bool,
    /// Whether the value is a config-provider reference rather than a value.
    pub provider: bool,
    /// The plugin definition's group for this key, where it declares one.
    pub group: Option<String>,
    /// The plugin definition's documentation for this key.
    pub documentation: Option<String>,
}

/// How a config was redacted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Redaction {
    /// By the plugin's declared `PASSWORD` keys and by name.
    Declared,
    /// By name only: the plugin's definition could not be fetched. Fails
    /// closed — a key the name rule misses is shown, and the panel says this
    /// is the weaker of the two.
    PatternsOnly,
}

/// What a plugin's definition says about its keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginKeys {
    /// Keys declared `PASSWORD`.
    pub(crate) secrets: BTreeSet<String>,
    /// Group and documentation, by key.
    pub(crate) described: BTreeMap<String, (Option<String>, Option<String>)>,
}

impl PluginKeys {
    /// Whether `key`, or any `.`-boundary suffix of it, is declared secret.
    fn declares_secret(&self, key: &str) -> bool {
        if self.secrets.contains(key) {
            return true;
        }
        key.match_indices('.').any(|(index, _)| {
            key.get(index + 1..)
                .is_some_and(|suffix| self.secrets.contains(suffix))
        })
    }

    /// The definition's group and documentation for `key`, suffix-aware.
    fn describe(&self, key: &str) -> (Option<String>, Option<String>) {
        if let Some(found) = self.described.get(key) {
            return found.clone();
        }
        key.match_indices('.')
            .find_map(|(index, _)| {
                key.get(index + 1..)
                    .and_then(|suffix| self.described.get(suffix))
            })
            .cloned()
            .unwrap_or((None, None))
    }
}

/// Whether the name alone says `key` is a secret.
fn named_secret(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    if SECRET_PREFIXES.iter().any(|prefix| key.starts_with(prefix)) {
        return true;
    }
    let segments: Vec<&str> = key.rsplitn(3, '.').collect();
    let last = segments.first().copied().unwrap_or("");
    let last_two = match segments.get(1) {
        Some(previous) => format!("{previous}.{last}"),
        None => last.to_owned(),
    };
    SECRET_ENDINGS
        .iter()
        .any(|ending| last == *ending || last_two == *ending || last.ends_with(ending))
}

/// Whether a value is a config-provider reference: `${provider:[path:]key}`.
fn is_provider_reference(value: &str) -> bool {
    let Some(inner) = value
        .trim()
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
    else {
        return false;
    };
    !inner.contains('}') && inner.contains(':') && !inner.starts_with(':')
}

/// Redact one config map.
///
/// `keys` is `None` when the plugin's definition could not be fetched; the
/// name rule then stands alone and the result says so.
#[must_use]
pub fn redact(
    config: &BTreeMap<String, String>,
    keys: Option<&PluginKeys>,
) -> (Vec<ConnectorConfigEntry>, Redaction) {
    let redaction = if keys.is_some() {
        Redaction::Declared
    } else {
        Redaction::PatternsOnly
    };
    let entries = config
        .iter()
        .map(|(key, value)| {
            let provider = is_provider_reference(value);
            let secret = !provider
                && (keys.is_some_and(|keys| keys.declares_secret(key)) || named_secret(key));
            let (group, documentation) = keys.map(|keys| keys.describe(key)).unwrap_or_default();
            ConnectorConfigEntry {
                key: key.clone(),
                value: if secret {
                    REDACTED.to_owned()
                } else {
                    value.clone()
                },
                redacted: secret,
                provider,
                group,
                documentation,
            }
        })
        .collect();
    (entries, redaction)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(secrets: &[&str]) -> PluginKeys {
        PluginKeys {
            secrets: secrets.iter().map(|s| (*s).to_owned()).collect(),
            described: BTreeMap::new(),
        }
    }

    fn config(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn shown(entries: &[ConnectorConfigEntry], key: &str) -> String {
        entries
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.value.clone())
            .unwrap()
    }

    #[test]
    fn a_declared_password_is_withheld_with_or_without_a_prefix() {
        // MirrorSourceConnector's definition, measured: it declares the bare
        // `sasl.jaas.config` and the connector is configured with prefixed
        // copies of it.
        let declared = keys(&["sasl.jaas.config", "ssl.truststore.password"]);
        let (entries, redaction) = redact(
            &config(&[
                (
                    "source.cluster.sasl.jaas.config",
                    "org.apache...password=\"s3cret\";",
                ),
                ("target.cluster.ssl.truststore.password", "changeit"),
                ("sasl.jaas.config", "plain"),
                ("source.cluster.alias", "upstream"),
            ]),
            Some(&declared),
        );
        assert_eq!(redaction, Redaction::Declared);
        assert_eq!(shown(&entries, "source.cluster.sasl.jaas.config"), REDACTED);
        assert_eq!(
            shown(&entries, "target.cluster.ssl.truststore.password"),
            REDACTED
        );
        assert_eq!(shown(&entries, "sasl.jaas.config"), REDACTED);
        assert_eq!(shown(&entries, "source.cluster.alias"), "upstream");
    }

    #[test]
    fn a_secret_declared_as_a_string_is_still_caught_by_its_name() {
        // Both measured: the MQTT source declares its key as STRING, the GTFS
        // source its headers.
        let (entries, _) = redact(
            &config(&[
                ("connect.mqtt.ssl.key", "-----BEGIN PRIVATE KEY-----"),
                ("connect.gtfsrt.http.headers", "Authorization:Bearer abc"),
                (
                    "key.converter",
                    "org.apache.kafka.connect.storage.StringConverter",
                ),
                ("aws.secret.access.key.id", "AKIA"),
                ("tasks.max", "1"),
            ]),
            Some(&keys(&[])),
        );
        assert_eq!(shown(&entries, "connect.mqtt.ssl.key"), REDACTED);
        assert_eq!(shown(&entries, "connect.gtfsrt.http.headers"), REDACTED);
        assert_eq!(shown(&entries, "aws.secret.access.key.id"), REDACTED);
        assert_eq!(
            shown(&entries, "key.converter"),
            "org.apache.kafka.connect.storage.StringConverter",
            "a converter is not a key"
        );
        assert_eq!(shown(&entries, "tasks.max"), "1");
    }

    #[test]
    fn a_provider_reference_is_a_pointer_and_stays_visible() {
        let (entries, _) = redact(
            &config(&[
                (
                    "connect.mqtt.password",
                    "${file:/opt/secrets.properties:mqtt}",
                ),
                ("database.password", "${vault:secret/db:password}"),
            ]),
            Some(&keys(&["connect.mqtt.password"])),
        );
        for entry in &entries {
            assert!(entry.provider && !entry.redacted, "{entry:?}");
        }
    }

    #[test]
    fn without_a_definition_the_names_stand_alone_and_say_so() {
        let (entries, redaction) = redact(
            &config(&[("connect.mqtt.password", "hunter2"), ("tasks.max", "1")]),
            None,
        );
        assert_eq!(redaction, Redaction::PatternsOnly);
        assert_eq!(shown(&entries, "connect.mqtt.password"), REDACTED);
        assert!(!entries.iter().any(|entry| entry.value == "hunter2"));
    }

    #[test]
    fn not_every_brace_is_a_provider() {
        assert!(is_provider_reference("${file:/x:y}"));
        assert!(is_provider_reference("${env:PASSWORD}"));
        assert!(!is_provider_reference("${nothing}"));
        assert!(!is_provider_reference("prefix ${file:/x:y}"));
        assert!(!is_provider_reference("${:x}"));
    }
}
