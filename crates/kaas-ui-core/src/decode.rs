//! Turning a record's bytes into payloads, with the registry a cluster names.
//!
//! The decision about *how* to read a payload has three inputs, and they are
//! consulted in this order:
//!
//! 1. **The framing.** A payload carrying the Confluent magic byte is
//!    registry-backed, and the registry says whether its id is Avro, JSON
//!    Schema or Protobuf. Nothing is guessed.
//! 2. **The request.** The chip in the message list is a query parameter. It
//!    can always fall *back* — to hex or string, which need no schema — and
//!    cannot invent a schema id to move up.
//! 3. **The configuration.** A cluster's `codecs:` entries, matched against
//!    the topic name.
//!
//! Key and value are decoded independently, because a JSON key beside an Avro
//! value is ordinary and having to choose one for both would make that topic
//! unreadable.

use std::sync::Arc;

use kaas_ui_serde::{Codec, Decoded, Payload, Predicate, PredicateStats, RegistryHandle};
use kafka_read::Record;
use serde::Deserialize;

use crate::dto::Header;
use crate::registry::ClusterHandle;

/// What the reader asked for, per side, on one request.
///
/// Absent means "whatever the configuration says", which in turn defaults to
/// [`Codec::Auto`]. This is the chip in the message list travelling as a query
/// parameter, so that the message view's URL stays the shareable artifact it
/// was in Phase 3.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodecOverride {
    /// How to read keys.
    pub key: Option<Codec>,
    /// How to read values.
    pub value: Option<Codec>,
}

/// Everything one request needs to render payloads.
///
/// Holds an `Arc` to the registry rather than borrowing it, because a live
/// stream outlives the request that opened it: the pump runs on its own task
/// and has to keep the client it decodes with. The clone is per request, not
/// per record, so the handle stays the one shared client — the `Arc` is what
/// makes sharing observable rather than what copies it.
///
/// The user predicate lives here too, and that is the point: **decode then
/// filter is one operation**, so there is no arrangement of calls that runs
/// the expensive filter on a record the cheap ones would have dropped.
#[derive(Debug)]
pub struct PayloadDecoder {
    registry: Option<Arc<RegistryHandle>>,
    key: Codec,
    value: Codec,
    predicate: Option<Predicate>,
}

impl PayloadDecoder {
    /// The decoder for one topic on one cluster, as one request asked for it.
    #[must_use]
    pub fn new(handle: &ClusterHandle, topic: &str, request: CodecOverride) -> Self {
        let (key, value) = handle.configured_codecs(topic);
        Self {
            registry: handle.schema_registry().map(Arc::clone),
            key: request.key.unwrap_or(key),
            value: request.value.unwrap_or(value),
            predicate: None,
        }
    }

    /// Filter with a compiled user predicate as well as decoding.
    #[must_use]
    pub fn with_predicate(mut self, predicate: Option<Predicate>) -> Self {
        self.predicate = predicate;
        self
    }

    /// A decoder that resolves nothing. For the paths with no cluster in
    /// hand — a malformed batch's raw bytes, and the tests.
    #[must_use]
    pub fn plain() -> Self {
        Self {
            registry: None,
            key: Codec::Auto,
            value: Codec::Auto,
            predicate: None,
        }
    }

    /// Which registry, if any, is answering for this cluster.
    #[must_use]
    pub fn registry(&self) -> Option<&Arc<RegistryHandle>> {
        self.registry.as_ref()
    }

    /// Decode a record and apply the user predicate, in that order.
    ///
    /// `None` means the record is not to be shown. **The one place the
    /// ordering lives**: the cheap filters have already run — kaas-lib's are in
    /// the scan spec, the caller's offset floor runs before this is called —
    /// and the predicate runs last, on the decoded value, exactly once.
    pub async fn accept(&self, record: &Record, ceiling: usize) -> Option<DecodedRecord> {
        let decoded = self.record(record, ceiling).await;
        let Some(predicate) = &self.predicate else {
            return Some(decoded);
        };

        // A structured decode is complete whatever the ceiling — truncation
        // only shortens its *rendering*. The string fallback is not: its text
        // is the very value the predicate sees, and the preview ceiling must
        // not decide filter results — a filter that silently depends on it
        // excludes exactly the records someone is searching for. That one
        // case is re-decoded unbounded, for the predicate only; the sandbox's
        // memory cap is what bounds it, and a value too big for the sandbox
        // is a counted failure rather than a silent mismatch.
        let matched = match (&decoded.value, &record.value) {
            (Some(side), Some(bytes)) if side.value.is_none() && side.payload.truncated => {
                let whole =
                    kaas_ui_serde::decode(self.registry.as_deref(), bytes, self.value, usize::MAX)
                        .await;
                let value = match whole.value {
                    Some(value) => value,
                    None => serde_json::Value::String(whole.payload.text),
                };
                predicate.matches(&value)
            }
            _ => predicate.matches(&decoded.predicate_value()),
        };
        matched.then_some(decoded)
    }

    /// What the user predicate has done, where there is one.
    #[must_use]
    pub fn predicate_stats(&self) -> Option<PredicateStats> {
        self.predicate.as_ref().map(Predicate::stats)
    }

    /// Decode both sides of a record, and render its headers.
    pub async fn record(&self, record: &Record, ceiling: usize) -> DecodedRecord {
        let registry = self.registry.as_deref();
        let key = match &record.key {
            Some(bytes) => Some(kaas_ui_serde::decode(registry, bytes, self.key, ceiling).await),
            None => None,
        };
        let value = match &record.value {
            Some(bytes) => Some(kaas_ui_serde::decode(registry, bytes, self.value, ceiling).await),
            None => None,
        };

        DecodedRecord {
            key,
            value,
            headers: record
                .headers
                .iter()
                .map(|(name, bytes)| Header {
                    name: name.clone(),
                    // Headers are never resolved against the registry. They
                    // could in principle carry framing, but a per-header schema
                    // lookup on every row would spend the registry's budget on
                    // the part of a record nobody opened it for.
                    value: bytes
                        .as_ref()
                        .map(|bytes| Payload::plain(bytes, Codec::Auto, ceiling)),
                })
                .collect(),
        }
    }
}

/// One record, decoded: what reaches the wire and what a predicate sees.
#[derive(Debug, Clone)]
pub struct DecodedRecord {
    /// The key. `None` is a keyless record.
    pub key: Option<Decoded>,
    /// The value. `None` is a **tombstone**, which is not the same as an empty
    /// value — compaction turns on the difference.
    pub value: Option<Decoded>,
    /// The headers, always plain.
    pub headers: Vec<Header>,
}

impl DecodedRecord {
    /// The value a JS predicate is evaluated against.
    ///
    /// Always a JSON value, so a predicate never has to test for absence
    /// before it can test for anything else:
    ///
    /// * a structured decode — Avro, Protobuf, JSON — is the value itself;
    /// * a text or hex rendering is that **string**, so `v.includes("boom")`
    ///   works on a topic with no schema at all;
    /// * a tombstone is `null`, which is distinguishable from an empty value
    ///   and is the thing a predicate would actually want to ask about.
    #[must_use]
    pub fn predicate_value(&self) -> std::borrow::Cow<'_, serde_json::Value> {
        use std::borrow::Cow;
        match &self.value {
            None => Cow::Owned(serde_json::Value::Null),
            Some(decoded) => match &decoded.value {
                Some(value) => Cow::Borrowed(value),
                None => Cow::Owned(serde_json::Value::String(decoded.payload.text.clone())),
            },
        }
    }

    /// The key payload, ready for the wire.
    #[must_use]
    pub fn key_payload(&self) -> Option<Payload> {
        self.key.as_ref().map(|decoded| decoded.payload.clone())
    }

    /// The value payload, ready for the wire.
    #[must_use]
    pub fn value_payload(&self) -> Option<Payload> {
        self.value.as_ref().map(|decoded| decoded.payload.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::registry::Registry;
    use kaas_ui_auth::Access;

    fn cluster(yaml: &str) -> Registry {
        Registry::from_config(&Config::from_yaml(yaml).unwrap()).unwrap()
    }

    const NO_REGISTRY: &str = r#"
clusters:
  - id: kaas
    bootstrap: ["a:9092"]
    codecs:
      - topic: "raw-*"
        value: hex
"#;

    #[test]
    fn the_request_overrides_the_configuration_and_the_configuration_overrides_auto() {
        let registry = cluster(NO_REGISTRY);
        let handle = registry.get("kaas", &Access::admin()).unwrap();

        let configured = PayloadDecoder::new(handle, "raw-bytes", CodecOverride::default());
        assert_eq!(configured.value, Codec::Hex);
        assert_eq!(configured.key, Codec::Auto);

        let asked = PayloadDecoder::new(
            handle,
            "raw-bytes",
            CodecOverride {
                key: None,
                value: Some(Codec::String),
            },
        );
        assert_eq!(asked.value, Codec::String);

        let unmatched = PayloadDecoder::new(handle, "orders", CodecOverride::default());
        assert_eq!(unmatched.value, Codec::Auto);
        // A cluster with no `schema_registry:` has none, and that is a normal
        // path rather than a degraded one.
        assert!(unmatched.registry().is_none());
    }

    #[tokio::test]
    async fn the_predicate_sees_the_whole_value_not_the_preview() {
        let needle_past_the_ceiling =
            format!("{}order-123", "x".repeat(kaas_ui_serde::PREVIEW_CHARS * 2));
        let predicate = kaas_ui_serde::Predicate::compile("v => v.includes('order-123')").unwrap();
        let decoder = PayloadDecoder::plain().with_predicate(Some(predicate));
        let record = Record {
            topic: "orders".to_owned(),
            partition: 0,
            offset: 0,
            timestamp: 0,
            timestamp_type: kafka_read::TimestampType::Creation,
            key: None,
            value: Some(bytes::Bytes::from(needle_past_the_ceiling)),
            headers: Vec::new(),
            producer_id: None,
            transactional: false,
            leader_epoch: None,
        };
        let accepted = decoder
            .accept(&record, kaas_ui_serde::PREVIEW_CHARS)
            .await
            .expect("the match sits past the preview ceiling and must still be found");
        // The wire payload stays at the preview budget; only the predicate
        // reads the value whole.
        assert!(
            accepted
                .value
                .as_ref()
                .is_some_and(|side| side.payload.truncated)
        );
    }

    #[tokio::test]
    async fn a_tombstone_is_not_an_empty_value() {
        let decoder = PayloadDecoder::plain();
        let record = Record {
            topic: "orders".to_owned(),
            partition: 0,
            offset: 0,
            timestamp: 0,
            timestamp_type: kafka_read::TimestampType::Creation,
            key: Some(bytes::Bytes::from_static(b"k")),
            value: None,
            headers: Vec::new(),
            producer_id: None,
            transactional: false,
            leader_epoch: None,
        };
        let decoded = decoder
            .record(&record, kaas_ui_serde::MAX_PAYLOAD_CHARS)
            .await;
        assert!(decoded.key.is_some());
        // `None` rather than an empty payload: compaction turns on the
        // difference, and so does a predicate.
        assert!(decoded.value.is_none());
        // A predicate sees `null`, which is what it would ask about.
        assert_eq!(*decoded.predicate_value(), serde_json::Value::Null);
    }
}
