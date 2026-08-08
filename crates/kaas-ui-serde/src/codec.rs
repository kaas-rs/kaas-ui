//! What a payload was decoded with, and the codecs that need no registry.
//!
//! The four codecs here are Phase 3's, given a name: `auto` renders text as
//! text and everything else as hex, `string` and `hex` force one of those, and
//! `json` is `string` that checked. None of them resolve anything, none of
//! them can fail to produce output, and all of them are reachable with no
//! registry configured — which is the common case on `kaas`.
//!
//! The three that are missing — Avro, Protobuf, JSON Schema — live in
//! [`crate::registry`], because all three are the same Confluent wire format
//! and all three resolve a schema id.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Above this, a payload is cut short: one oversized record must not be able
/// to blow up a browser tab that asked for five hundred of them.
pub const MAX_PAYLOAD_CHARS: usize = 8192;

/// The ceiling on a payload that rides in a *stream*.
///
/// Much smaller than [`MAX_PAYLOAD_CHARS`], and not a tuning knob. A topic
/// carrying 1 KB values at ten thousand records a second is 10 MB/s the
/// browser would parse, hold in a ring buffer and never draw — the list shows
/// one truncated line per row whatever arrives. The rest is fetched for the
/// one record someone actually selected.
pub const PREVIEW_CHARS: usize = 256;

/// The ceiling on the one payload someone actually opened.
///
/// A whole megabyte, because this is the answer to "show me this record" and
/// cutting it at the list's budget would make the detail panel useless for the
/// large records that are the reason anyone opens it. Still a ceiling: a
/// response is not allowed to be as large as a producer felt like being.
pub const DETAIL_PAYLOAD_CHARS: usize = 1024 * 1024;

/// How a payload was read.
///
/// The chip in the message list is this value, and the chip is the **override
/// control**, not a label: auto-detection that cannot be corrected is worse
/// than none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Codec {
    /// Text where the bytes are text, hex where they are not.
    #[default]
    Auto,
    /// Forced UTF-8, with the invalid sequences replaced rather than hidden.
    String,
    /// Forced hex.
    Hex,
    /// UTF-8 that was checked to parse as JSON.
    Json,
    /// Avro, resolved by schema id.
    Avro,
    /// Protobuf, resolved by schema id.
    Protobuf,
    /// JSON Schema, resolved by schema id.
    JsonSchema,
}

impl Codec {
    /// Whether choosing this codec means resolving a schema id.
    ///
    /// The whole reason the override is only free in one direction: these
    /// three cannot be *chosen*, only discovered from the framing, because
    /// nothing can invent a schema id for a payload that does not carry one.
    #[must_use]
    pub fn is_registry_backed(self) -> bool {
        matches!(self, Self::Avro | Self::Protobuf | Self::JsonSchema)
    }

    /// The name the UI and the error messages use.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::String => "string",
            Self::Hex => "hex",
            Self::Json => "json",
            Self::Avro => "avro",
            Self::Protobuf => "protobuf",
            Self::JsonSchema => "jsonSchema",
        }
    }
}

/// Which of the three registry formats a schema is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SchemaFormat {
    /// Avro.
    Avro,
    /// Protobuf.
    Protobuf,
    /// JSON Schema.
    Json,
}

impl SchemaFormat {
    /// The codec that decodes this format.
    #[must_use]
    pub fn codec(self) -> Codec {
        match self {
            Self::Avro => Codec::Avro,
            Self::Protobuf => Codec::Protobuf,
            Self::Json => Codec::JsonSchema,
        }
    }
}

/// Which schema decoded a payload, and which registry answered.
///
/// The registry id is here rather than implied by the cluster because a
/// registry serves an *environment*: two clusters can show the same schema id
/// and mean the same schema, and a reader has to be able to see that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaRef {
    /// The id the framing carried.
    pub id: u32,
    /// What the registry said the id is.
    pub format: SchemaFormat,
    /// The registry that answered, by its configured id.
    pub registry: String,
    /// The subject the registry named, where it named one. A schema id can be
    /// registered against more than one subject, so this is "what the registry
    /// returned" rather than an authoritative single answer.
    pub subject: Option<String>,
    /// The version within that subject, where the registry gave one.
    pub version: Option<u32>,
    /// The record or message name inside the schema.
    pub name: Option<String>,
}

/// Why a payload is not what the reader asked for.
///
/// Five distinct causes, kept apart because they want five different things
/// done about them — and because the alternative is a topic that silently
/// renders as hex with no way to tell a broken registry from a broken URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum NoteKind {
    /// The payload was framed, the schema resolved, and the bytes still did
    /// not decode. An application-level failure on an otherwise fine record —
    /// never the same thing as a batch that would not decode at the protocol
    /// level.
    DecodeError,
    /// The registry could not be reached. Records are still here; they are
    /// hex until it comes back.
    RegistryUnavailable,
    /// The payload is registry-backed and this cluster references no registry.
    /// Not an error — a registry is genuinely absent on some clusters — but
    /// the reason this record is hex has to be visible all the same.
    RegistryAbsent,
    /// The registry answered and is not speaking the Confluent API. A
    /// **configuration** fault: one missing path segment turns every Avro
    /// topic into hex, and this is what stops that being silent.
    RegistryMisconfigured,
    /// The requested codec could not be honoured. Falling back to hex or
    /// string is always possible; overriding *up* to Avro is not, because
    /// nothing can invent a schema id.
    OverrideRefused,
    /// The value parsed but does not satisfy its own subject's schema.
    NonConforming,
}

/// A note attached to a payload, with the sentence to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PayloadNote {
    /// What kind of note this is.
    pub kind: NoteKind,
    /// What to tell the reader.
    pub message: String,
}

impl PayloadNote {
    /// Build a note.
    pub fn new(kind: NoteKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// The bytes as they arrived, so a codec can be changed without a refetch.
///
/// Present exactly when [`Payload::text`] is a *decoded* rendering — Avro,
/// Protobuf or JSON Schema — because those are the only renderings the
/// original bytes cannot be recovered from. A hex rendering is already the
/// bytes, and a UTF-8 one is losslessly convertible to hex in the browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RawBytes {
    /// Lowercase hex of the record as it arrived, framing included.
    pub hex: String,
    /// Whether the hex was cut short.
    pub truncated: bool,
}

/// A key or value, rendered with the codec that was used said out loud.
///
/// Auto-detection that cannot be seen is worse than none: the reader has to
/// know whether they are looking at text the producer wrote, at kaas-ui's
/// guess, or at a schema the registry resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
    /// What produced [`Self::text`].
    pub codec: Codec,
    /// How to read it: `utf8`, `hex` or `json`.
    pub encoding: String,
    /// The rendering.
    pub text: String,
    /// Length in bytes of the original.
    pub bytes: usize,
    /// Whether `text` was cut short.
    pub truncated: bool,
    /// The original bytes, for a codec change with no refetch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<RawBytes>,
    /// Which schema decoded it, when a registry was involved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<SchemaRef>,
    /// Why this is not what was asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<PayloadNote>,
}

impl Payload {
    /// Render bytes with a codec that needs no registry.
    ///
    /// A registry-backed codec asked for here is a payload that was **not
    /// framed** — nothing carries a schema id — so it is refused with a
    /// reason rather than guessed at, and the bytes are still rendered.
    #[must_use]
    pub fn plain(bytes: &[u8], want: Codec, ceiling: usize) -> Self {
        match want {
            Codec::Auto => Self::auto(bytes, ceiling),
            Codec::Hex => Self::hex(bytes, ceiling),
            Codec::String => Self::string(bytes, ceiling),
            Codec::Json => Self::json(bytes, ceiling),
            registry_backed => Self::auto(bytes, ceiling).with_note(PayloadNote::new(
                NoteKind::OverrideRefused,
                format!(
                    "this payload carries no schema id, so it cannot be decoded as {}: the \
                     Confluent framing — a zero byte and a four-byte id — is what says a payload \
                     is registry-backed, and these bytes do not have it",
                    registry_backed.label()
                ),
            )),
        }
    }

    /// Text where the bytes are text, hex where they are not.
    #[must_use]
    pub fn auto(bytes: &[u8], ceiling: usize) -> Self {
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                let (text, truncated) = truncate(text, ceiling);
                Self::base(Codec::Auto, "utf8", text, bytes.len(), truncated)
            }
            Err(_) => {
                let mut payload = Self::hex(bytes, ceiling);
                payload.codec = Codec::Auto;
                payload
            }
        }
    }

    /// Forced hex.
    #[must_use]
    pub fn hex(bytes: &[u8], ceiling: usize) -> Self {
        let (text, truncated) = hex_of(bytes, ceiling);
        Self::base(Codec::Hex, "hex", text, bytes.len(), truncated)
    }

    /// Forced UTF-8, replacing what is not.
    ///
    /// Lossy rather than refused: this codec is what someone picks when they
    /// know the topic is text and kaas-ui guessed hex because of one stray
    /// byte, and refusing it would leave them with no way to look.
    #[must_use]
    pub fn string(bytes: &[u8], ceiling: usize) -> Self {
        let text = String::from_utf8_lossy(bytes);
        let (text, truncated) = truncate(&text, ceiling);
        Self::base(Codec::String, "utf8", text, bytes.len(), truncated)
    }

    /// UTF-8 that was checked to parse as JSON.
    ///
    /// The text is the producer's, byte for byte — not a re-serialisation —
    /// because a reformatted rendering is no longer the bytes that arrived,
    /// and formatting is the browser's job anyway.
    #[must_use]
    pub fn json(bytes: &[u8], ceiling: usize) -> Self {
        match std::str::from_utf8(bytes) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(text) {
                Ok(_) => {
                    let (text, truncated) = truncate(text, ceiling);
                    Self::base(Codec::Json, "json", text, bytes.len(), truncated)
                }
                Err(error) => Self::auto(bytes, ceiling).with_note(PayloadNote::new(
                    NoteKind::DecodeError,
                    format!("not valid JSON: {error}"),
                )),
            },
            Err(_) => Self::auto(bytes, ceiling).with_note(PayloadNote::new(
                NoteKind::DecodeError,
                "not valid JSON: the bytes are not UTF-8".to_owned(),
            )),
        }
    }

    /// A decoded value, rendered as JSON text beside the bytes it came from.
    #[must_use]
    pub fn decoded(
        value: &serde_json::Value,
        original: &[u8],
        codec: Codec,
        schema: SchemaRef,
        ceiling: usize,
    ) -> Self {
        let rendered = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_owned());
        let (text, truncated) = truncate(&rendered, ceiling);
        let (hex, hex_truncated) = hex_of(original, ceiling);
        Self {
            codec,
            encoding: "json".to_owned(),
            text,
            bytes: original.len(),
            truncated,
            raw: Some(RawBytes {
                hex,
                truncated: hex_truncated,
            }),
            schema: Some(schema),
            note: None,
        }
    }

    /// Attach a note, replacing any that was there.
    #[must_use]
    pub fn with_note(mut self, note: PayloadNote) -> Self {
        self.note = Some(note);
        self
    }

    /// Attach the schema the framing named, even where decoding did not get
    /// as far as using it.
    #[must_use]
    pub fn with_schema(mut self, schema: SchemaRef) -> Self {
        self.schema = Some(schema);
        self
    }

    fn base(codec: Codec, encoding: &str, text: String, bytes: usize, truncated: bool) -> Self {
        Self {
            codec,
            encoding: encoding.to_owned(),
            text,
            bytes,
            truncated,
            raw: None,
            schema: None,
            note: None,
        }
    }
}

/// The Confluent framing: a zero byte, a four-byte big-endian schema id, body.
///
/// This is the whole of the sniff, and it decides one thing only — whether the
/// payload is registry-backed at all. It does **not** guess a format: the
/// registry says whether 42 is Avro, JSON Schema or Protobuf.
///
/// A payload that is not framed is therefore not a decode failure. It is a
/// payload that was never registry-backed, and the two paths never compete for
/// the same bytes.
#[must_use]
pub fn framed(bytes: &[u8]) -> Option<u32> {
    // `> 4` rather than `>= 4`: a payload of exactly five bytes has an id and
    // an empty body, and a four-byte one has no body at all to have been
    // framed around.
    if bytes.len() > 4 && bytes.first() == Some(&0) {
        let id = bytes.get(1..5)?;
        let id: [u8; 4] = id.try_into().ok()?;
        Some(u32::from_be_bytes(id))
    } else {
        None
    }
}

fn hex_of(bytes: &[u8], ceiling: usize) -> (String, bool) {
    let mut hex = String::new();
    for byte in bytes {
        if hex.len() >= ceiling {
            return (hex, true);
        }
        // Writing into a String cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    (hex, false)
}

fn truncate(text: &str, ceiling: usize) -> (String, bool) {
    if text.len() <= ceiling {
        return (text.to_owned(), false);
    }
    let mut end = ceiling;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    match text.get(..end) {
        Some(head) => (head.to_owned(), true),
        None => (String::new(), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_text_and_bytes_are_hex() {
        let text = Payload::auto(b"hello", MAX_PAYLOAD_CHARS);
        assert_eq!(text.encoding, "utf8");
        assert_eq!(text.text, "hello");
        assert_eq!(text.codec, Codec::Auto);

        let binary = Payload::auto(&[0xff, 0x00, 0x10], MAX_PAYLOAD_CHARS);
        assert_eq!(binary.encoding, "hex");
        assert_eq!(binary.text, "ff0010");
        // Still `auto`: the reader has to be able to tell "kaas-ui chose hex"
        // from "someone asked for hex".
        assert_eq!(binary.codec, Codec::Auto);
    }

    #[test]
    fn a_forced_codec_says_it_was_forced() {
        let forced = Payload::hex(b"hello", MAX_PAYLOAD_CHARS);
        assert_eq!(forced.codec, Codec::Hex);
        assert_eq!(forced.text, "68656c6c6f");
    }

    #[test]
    fn json_keeps_the_producers_bytes_rather_than_reserialising() {
        let payload = Payload::json(br#"{ "a"  :  1 }"#, MAX_PAYLOAD_CHARS);
        assert_eq!(payload.encoding, "json");
        assert_eq!(payload.text, r#"{ "a"  :  1 }"#);
        assert!(payload.note.is_none());
    }

    #[test]
    fn json_that_does_not_parse_is_a_payload_error_not_an_empty_render() {
        let payload = Payload::json(b"{oops", MAX_PAYLOAD_CHARS);
        // The bytes are still shown. A note is what says they are not JSON.
        assert_eq!(payload.text, "{oops");
        assert_eq!(
            payload.note.as_ref().map(|n| n.kind),
            Some(NoteKind::DecodeError)
        );
    }

    #[test]
    fn the_framing_decides_only_whether_a_payload_is_registry_backed() {
        // Magic byte, big-endian id 1, body.
        assert_eq!(framed(&[0x00, 0x00, 0x00, 0x00, 0x01, 0x06]), Some(1));
        // No magic byte: never registry-backed, whatever it looks like.
        assert_eq!(framed(b"{\"a\":1}"), None);
        // A leading zero with nothing after it is not a frame.
        assert_eq!(framed(&[0x00, 0x00, 0x00, 0x00]), None);
        assert_eq!(framed(&[]), None);
        // The id is big-endian, and large ids are exactly where a
        // little-endian read would look plausible and be wrong.
        assert_eq!(framed(&[0x00, 0x00, 0x01, 0x00, 0x00, 0x2a]), Some(65536));
    }

    #[test]
    fn an_unframed_payload_cannot_be_forced_up_to_avro() {
        // The override is free downward and refused upward, because nothing
        // can invent a schema id.
        let payload = Payload::plain(b"hello", Codec::Avro, MAX_PAYLOAD_CHARS);
        assert_eq!(
            payload.note.as_ref().map(|n| n.kind),
            Some(NoteKind::OverrideRefused)
        );
        assert!(
            payload
                .note
                .as_ref()
                .is_some_and(|n| n.message.contains("avro")),
            "the refusal has to name the codec that was refused"
        );
        // And the bytes are still rendered: a refusal is not a blank panel.
        assert_eq!(payload.text, "hello");
    }

    #[test]
    fn a_long_payload_is_cut_and_says_so() {
        let long = "x".repeat(MAX_PAYLOAD_CHARS * 2);
        let payload = Payload::auto(long.as_bytes(), MAX_PAYLOAD_CHARS);
        assert!(payload.truncated);
        assert_eq!(payload.text.len(), MAX_PAYLOAD_CHARS);
        // The *original* length, not the rendered one: it is what the reader
        // needs to know how much was left out.
        assert_eq!(payload.bytes, MAX_PAYLOAD_CHARS * 2);
    }

    #[test]
    fn truncation_never_splits_a_character() {
        // Three-byte characters against a ceiling that lands mid-character.
        let text = "€".repeat(10);
        let payload = Payload::auto(text.as_bytes(), 8);
        assert!(payload.truncated);
        assert_eq!(payload.text, "€€");
    }

    #[test]
    fn a_decoded_value_carries_the_bytes_it_came_from() {
        // The property the "override down needs no refetch" claim rests on.
        let raw = [0x00, 0x00, 0x00, 0x00, 0x01, 0x06];
        let payload = Payload::decoded(
            &serde_json::json!({ "beat": 3 }),
            &raw,
            Codec::Avro,
            SchemaRef {
                id: 1,
                format: SchemaFormat::Avro,
                registry: "dev".to_owned(),
                subject: Some("heartbeat-value".to_owned()),
                version: Some(1),
                name: Some("Heartbeat".to_owned()),
            },
            MAX_PAYLOAD_CHARS,
        );
        assert_eq!(
            payload.raw.as_ref().map(|r| r.hex.as_str()),
            Some("000000000106")
        );
        assert_eq!(payload.bytes, 6);
        assert_eq!(payload.schema.as_ref().map(|s| s.id), Some(1));
    }
}
