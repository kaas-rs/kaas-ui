//! Payload decoding: the Confluent framing, the registry client behind it,
//! and the codecs that need neither.
//!
//! kaas-lib hands over bytes; everything here is ours. The crate knows nothing
//! about HTTP routing and nothing about clusters — it is below
//! `kaas-ui-core`, not beside it — which is what lets one
//! [`RegistryHandle`](registry::RegistryHandle) be shared by every cluster
//! that references the same registry id.
//!
//! # The magic byte routes, it does not guess
//!
//! [`codec::framed`] is the whole sniff, and with one library owning the three
//! registry formats it no longer means "detect the codec". It decides whether
//! the payload is registry-backed at all:
//!
//! * **framed** — the schema id is resolved, and the *registry* says whether
//!   42 is Avro, JSON Schema or Protobuf. Nothing is left to guess.
//! * **not framed** — one of the codecs in [`codec`], from per-topic
//!   configuration, defaulting to [`Codec::Auto`].
//!
//! An unframed payload is therefore **not a decode error**. It is a payload
//! that was never registry-backed, and the two paths never compete for the
//! same bytes.
//!
//! # The override is free in one direction only
//!
//! Falling back to hex or string needs no schema and no refetch: the raw bytes
//! travel beside the decoded value. Overriding *up* to Avro cannot invent a
//! schema id, and is refused with a reason rather than guessed at.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod codec;
pub mod naming;
pub mod proto;
pub mod registry;
pub mod subjects;

pub use codec::{
    Codec, DETAIL_PAYLOAD_CHARS, MAX_PAYLOAD_CHARS, NoteKind, PREVIEW_CHARS, Payload, PayloadNote,
    RawBytes, SchemaFormat, SchemaRef, framed, render_json,
};
pub use naming::{NamingStrategy, SubjectNaming, declared_name};
pub use registry::{
    Decoded, RegistryAuth, RegistryError, RegistryFault, RegistryHandle, RegistryHealth,
    RegistrySettings, RegistryStatus,
};
pub use subjects::{SchemaReference, SubjectSchema};

/// Decode one key or value.
///
/// The only entry point anything above this crate needs. `want` is what the
/// reader asked for — from the per-topic configuration, or from the chip in
/// the message list — and `registry` is `None` on a cluster that references
/// none, which is a normal path rather than a degraded one.
pub async fn decode(
    registry: Option<&RegistryHandle>,
    bytes: &[u8],
    want: Codec,
    ceiling: usize,
) -> Decoded {
    let Some(id) = framed(bytes) else {
        return Decoded::plain(Payload::plain(bytes, want, ceiling));
    };

    // Downward is free: hex and string need no schema, so a reader who chose
    // one gets it without the registry being consulted at all. This is the
    // half of the override that must work while the registry is down.
    if matches!(want, Codec::Hex | Codec::String | Codec::Json) {
        return Decoded::plain(Payload::plain(bytes, want, ceiling));
    }

    let Some(registry) = registry else {
        return Decoded::degraded(
            bytes,
            ceiling,
            PayloadNote::new(
                NoteKind::RegistryAbsent,
                format!(
                    "this payload carries schema id {id} and this cluster references no schema \
                     registry, so there is nothing to resolve it against: declare one under \
                     `schema_registries` and name it with `schema_registry: <id>`"
                ),
            ),
        );
    };

    let decoded = registry.decode(id, bytes, ceiling).await;

    // The registry is authoritative about what a schema id is. Asking for Avro
    // and getting Protobuf is not a failure — it is the registry answering a
    // question the reader guessed at — but it must not look like the guess was
    // honoured.
    if want.is_registry_backed()
        && decoded.payload.codec.is_registry_backed()
        && decoded.payload.codec != want
    {
        let actual = decoded.payload.codec.label();
        return decoded.with_note(PayloadNote::new(
            NoteKind::OverrideRefused,
            format!(
                "schema id {id} is {actual} according to the registry, not {}: the registry \
                 decides what a schema id is",
                want.label()
            ),
        ));
    }

    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property the whole routing rests on: absence of framing is not a
    /// failure, and it does not reach the registry.
    #[tokio::test]
    async fn an_unframed_payload_renders_without_a_registry_and_without_an_error() {
        let decoded = decode(None, b"just some text", Codec::Auto, MAX_PAYLOAD_CHARS).await;
        assert_eq!(decoded.payload.text, "just some text");
        assert_eq!(decoded.payload.encoding, "utf8");
        assert!(
            decoded.payload.note.is_none(),
            "an unframed payload is not a decode error"
        );
        assert!(decoded.payload.schema.is_none());
    }

    #[tokio::test]
    async fn a_framed_payload_with_no_registry_says_which_id_it_could_not_resolve() {
        let bytes = [0x00, 0x00, 0x00, 0x00, 0x2a, 0x06];
        let decoded = decode(None, &bytes, Codec::Auto, MAX_PAYLOAD_CHARS).await;
        assert_eq!(
            decoded.payload.note.as_ref().map(|n| n.kind),
            Some(NoteKind::RegistryAbsent)
        );
        assert!(
            decoded
                .payload
                .note
                .as_ref()
                .is_some_and(|n| n.message.contains("42")),
            "the note has to name the id nothing could resolve"
        );
        // The bytes are still there, as hex.
        assert_eq!(decoded.payload.text, "000000002a06");
    }

    /// Downward overrides do not consult a registry, which is what makes them
    /// work while one is down.
    #[tokio::test]
    async fn falling_back_to_hex_needs_no_registry_even_on_a_framed_payload() {
        let bytes = [0x00, 0x00, 0x00, 0x00, 0x2a, 0x06];
        for want in [Codec::Hex, Codec::String] {
            let decoded = decode(None, &bytes, want, MAX_PAYLOAD_CHARS).await;
            assert_eq!(decoded.payload.codec, want);
            assert!(
                decoded.payload.note.is_none(),
                "{want:?} on framed bytes is a plain rendering, not a failure"
            );
        }
    }
}
