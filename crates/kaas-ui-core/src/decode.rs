//! Turning a record's bytes into payloads, with the registry a cluster names.
//!
//! The decision about *how* to read a payload has three inputs, and they are
//! consulted in this order:
//!
//! 1. **The framing.** A payload carrying the Confluent magic byte is
//!    registry-backed, and the registry says whether its id is Avro, JSON
//!    Schema or Protobuf. Nothing is guessed.
//! 2. **The request.** `?keyCodec=` and `?valueCodec=`, which a link may carry
//!    even though no control in the app sets them any more. It can always fall
//!    *back* — to hex or string, which need no schema — and cannot invent a
//!    schema id to move up.
//! 3. **The configuration.** A cluster's `codecs:` entries, matched against
//!    the topic name.
//!
//! Key and value are decoded independently, because a JSON key beside an Avro
//! value is ordinary and having to choose one for both would make that topic
//! unreadable.

use std::borrow::Cow;
use std::sync::Arc;

use kaas_ui_serde::{Codec, Decoded, Payload, RegistryHandle};
use kafka_read::Record;
use serde::Deserialize;

use crate::dto::Header;
use crate::registry::ClusterHandle;

/// The longest needle a payload filter may carry.
///
/// Not a guess about what anyone wants to type. The needle is compared against
/// every record in a window, so its length is a cost the *caller* chooses and
/// the server pays — a megabyte of query string would be a megabyte of
/// comparison per record. Refusing one is cheaper than serving it, and 256
/// characters is far past any substring somebody is looking for by hand.
pub const MAX_FILTER_CHARS: usize = 256;

/// A needle too long to be a search.
#[derive(Debug, thiserror::Error)]
#[error(
    "a payload filter may be at most {MAX_FILTER_CHARS} characters and this one is {0}; it is a \
     literal substring of the decoded value, not a pattern"
)]
pub struct FilterTooLong(pub usize);

/// A literal substring match over a record's **decoded** value.
///
/// Literal is the whole security story, and it is a property of the type
/// rather than of the call sites: there is no expression to compile, no
/// pattern to backtrack over, and no interpreter to escape from. The needle
/// reaches exactly one operation — [`str::contains`] — so the only thing a
/// reader can express is "these characters, in this order". Regex
/// metacharacters, quotes, backslashes, `${…}` and newlines are all just
/// characters to match, which is why this replaced a JavaScript sandbox
/// instead of being layered under one.
///
/// It is matched against the value **as the reader sees it**: the JSON
/// rendering of a decoded record, or the text of one that decoded to no
/// structure. Searching for a field name works on an Avro topic, which is the
/// point of running after the decode rather than before it — the field names
/// are not in the bytes at all.
#[derive(Debug, Clone)]
pub struct PayloadFilter {
    needle: String,
}

impl PayloadFilter {
    /// The filter one request asked for, or `None` where it asked for nothing.
    ///
    /// Whitespace is not a filter: otherwise clearing the box leaves one that
    /// matches every record and costs a comparison per record to say so.
    pub fn parse(raw: Option<&str>) -> Result<Option<Self>, FilterTooLong> {
        let Some(needle) = raw.map(str::trim).filter(|needle| !needle.is_empty()) else {
            return Ok(None);
        };
        let length = needle.chars().count();
        if length > MAX_FILTER_CHARS {
            return Err(FilterTooLong(length));
        }
        Ok(Some(Self {
            needle: needle.to_owned(),
        }))
    }

    /// Whether this text contains the needle.
    #[must_use]
    pub fn matches(&self, haystack: &str) -> bool {
        haystack.contains(&self.needle)
    }
}

/// What the reader asked for, per side, on one request.
///
/// Absent means "whatever the configuration says", which in turn defaults to
/// [`Codec::Auto`]. It arrives as a query parameter and nothing else, so a URL
/// stays the shareable artifact it was in Phase 3 — the toolbar's two codec
/// selects are gone, and a link written while they existed still opens on the
/// view it named.
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
/// The payload filter lives here too, and that is the point: **decode then
/// filter is one operation**, so there is no arrangement of calls that renders
/// a row the filter rejected, and none that filters on anything but the
/// decoded value.
#[derive(Debug)]
pub struct PayloadDecoder {
    registry: Option<Arc<RegistryHandle>>,
    key: Codec,
    value: Codec,
    filter: Option<PayloadFilter>,
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
            filter: None,
        }
    }

    /// Drop records whose decoded value does not contain this needle.
    #[must_use]
    pub fn with_filter(mut self, filter: Option<PayloadFilter>) -> Self {
        self.filter = filter;
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
            filter: None,
        }
    }

    /// Which registry, if any, is answering for this cluster.
    #[must_use]
    pub fn registry(&self) -> Option<&Arc<RegistryHandle>> {
        self.registry.as_ref()
    }

    /// Decode a record and apply the payload filter, in that order.
    ///
    /// `None` means the record is not to be shown. **The one place that
    /// ordering lives**: the filter reads the decoded value, so it cannot run
    /// any earlier, and the cheap selections that *can* — the partitions and
    /// the window in the scan spec, the caller's offset floor — have already
    /// happened by the time a record arrives here.
    pub async fn accept(&self, record: &Record, ceiling: usize) -> Option<DecodedRecord> {
        let decoded = self.record(record, ceiling).await;
        let Some(filter) = &self.filter else {
            return Some(decoded);
        };
        let matched = filter.matches(&self.haystack(&decoded, record).await);
        matched.then_some(decoded)
    }

    /// The value a filter is matched against: what the reader sees, **whole**.
    ///
    /// The preview ceiling must not decide filter results. A filter that
    /// silently depends on it excludes exactly the records someone is
    /// searching for — the long ones — and there is nothing on screen to say
    /// so. Where the rendering was cut, it is rebuilt at full length here:
    ///
    /// * a **structured** decode is complete whatever the ceiling, because
    ///   truncation only shortened its rendering, so re-rendering the value
    ///   costs no registry call;
    /// * a **text or hex** rendering is the value, so it is decoded again with
    ///   no ceiling at all.
    ///
    /// A tombstone has nothing to search and matches no needle, which is the
    /// same answer the byte filter this replaced gave.
    async fn haystack<'a>(&self, decoded: &'a DecodedRecord, record: &Record) -> Cow<'a, str> {
        let Some(side) = &decoded.value else {
            return Cow::Borrowed("");
        };
        if !side.payload.truncated {
            return Cow::Borrowed(&side.payload.text);
        }
        if let Some(value) = &side.value {
            return Cow::Owned(kaas_ui_serde::render_json(value));
        }
        let Some(bytes) = &record.value else {
            return Cow::Borrowed("");
        };
        let whole =
            kaas_ui_serde::decode(self.registry.as_deref(), bytes, self.value, usize::MAX).await;
        Cow::Owned(match &whole.value {
            Some(value) => kaas_ui_serde::render_json(value),
            None => whole.payload.text,
        })
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

/// One record, decoded: what reaches the wire and what a filter searches.
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
environments:
  - id: dev
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
        codecs:
          - topic: "raw-*"
            value: hex
"#;

    #[test]
    fn the_request_overrides_the_configuration_and_the_configuration_overrides_auto() {
        let registry = cluster(NO_REGISTRY);
        let handle = registry.get("dev", "kaas", &Access::admin()).unwrap();

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

    fn record_of(value: &str) -> Record {
        Record {
            topic: "orders".to_owned(),
            partition: 0,
            offset: 0,
            timestamp: 0,
            timestamp_type: kafka_read::TimestampType::Creation,
            key: None,
            value: Some(bytes::Bytes::copy_from_slice(value.as_bytes())),
            headers: Vec::new(),
            producer_id: None,
            transactional: false,
            leader_epoch: None,
        }
    }

    fn filtered(needle: &str) -> PayloadDecoder {
        PayloadDecoder::plain()
            .with_filter(PayloadFilter::parse(Some(needle)).expect("the needle is short enough"))
    }

    #[tokio::test]
    async fn the_filter_sees_the_whole_value_not_the_preview() {
        let needle_past_the_ceiling =
            format!("{}order-123", "x".repeat(kaas_ui_serde::PREVIEW_CHARS * 2));
        let record = record_of(&needle_past_the_ceiling);
        let accepted = filtered("order-123")
            .accept(&record, kaas_ui_serde::PREVIEW_CHARS)
            .await
            .expect("the match sits past the preview ceiling and must still be found");
        // The wire payload stays at the preview budget; only the filter reads
        // the value whole.
        assert!(
            accepted
                .value
                .as_ref()
                .is_some_and(|side| side.payload.truncated)
        );
    }

    #[tokio::test]
    async fn a_needle_that_is_not_there_drops_the_record() {
        let record = record_of(r#"{"order":"abc"}"#);
        assert!(
            filtered("order")
                .accept(&record, kaas_ui_serde::PREVIEW_CHARS)
                .await
                .is_some()
        );
        assert!(
            filtered("shipment")
                .accept(&record, kaas_ui_serde::PREVIEW_CHARS)
                .await
                .is_none()
        );
    }

    /// The needle is data, and every character in it is only itself.
    ///
    /// This is the property the JS predicate could not have: there is no
    /// expression to compile, so a needle that looks like code, like a
    /// pattern, or like a template is matched character for character. A
    /// regression here would not be a wrong row count — it would be an
    /// evaluator somebody reintroduced.
    #[tokio::test]
    async fn a_needle_is_matched_literally_and_never_evaluated() {
        let record = record_of(r#"{"note":"nothing to see"}"#);
        for injection in [
            ".*",
            "^.*$",
            "'; DROP TABLE topics; --",
            "${jndi:ldap://x/y}",
            "{{7*7}}",
            "\" || true || \"",
            "</script><script>alert(1)</script>",
            "\n\rSet-Cookie: x=1",
        ] {
            assert!(
                filtered(injection)
                    .accept(&record, kaas_ui_serde::PREVIEW_CHARS)
                    .await
                    .is_none(),
                "{injection:?} matched a record that does not contain it"
            );
        }
        // And the same characters *do* match when the value really holds them.
        let literal = record_of(r#"{"note":"a .* b"}"#);
        assert!(
            filtered(".*")
                .accept(&literal, kaas_ui_serde::PREVIEW_CHARS)
                .await
                .is_some()
        );
    }

    #[test]
    fn a_filter_of_whitespace_is_no_filter_and_an_enormous_one_is_refused() {
        assert!(PayloadFilter::parse(None).unwrap().is_none());
        assert!(PayloadFilter::parse(Some("   ")).unwrap().is_none());
        assert!(PayloadFilter::parse(Some("order")).unwrap().is_some());

        let at_the_ceiling = "é".repeat(MAX_FILTER_CHARS);
        assert!(
            PayloadFilter::parse(Some(&at_the_ceiling)).is_ok(),
            "the ceiling counts characters, not the bytes a multi-byte one costs"
        );
        let over = "x".repeat(MAX_FILTER_CHARS + 1);
        assert!(PayloadFilter::parse(Some(&over)).is_err());
    }

    #[tokio::test]
    async fn a_tombstone_matches_no_needle() {
        // It has no value to search. Matching one would put a row with nothing
        // in it into the results of a search for something.
        let mut record = record_of("");
        record.value = None;
        assert!(
            filtered("anything")
                .accept(&record, kaas_ui_serde::PREVIEW_CHARS)
                .await
                .is_none()
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
        // difference, and so does the filter.
        assert!(decoded.value.is_none());
    }
}
