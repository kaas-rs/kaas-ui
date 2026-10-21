//! MirrorMaker 2's internal records: heartbeats, checkpoints, offset syncs.
//!
//! MirrorMaker writes three kinds of record to ordinary Kafka topics, and
//! kaas-ui reads them the way it reads any topic — kaas-lib fetches the
//! bytes, and turning bytes into fields is payload decoding, which is this
//! crate's job. (kaas-lib is a Kafka client; this is a record's *contents*.)
//!
//! The encoding is Kafka's internal struct format, as in
//! `org.apache.kafka.connect.mirror.{Heartbeat,Checkpoint,OffsetSync}`:
//! big-endian integers, and a `STRING` as an `INT16` length followed by that
//! many bytes of UTF-8. Heartbeat and checkpoint values open with an `INT16`
//! format version, and only version 0 exists. A record carrying any other is
//! [`MirrorRecordError::UnrecognisedVersion`] — reported, never guessed at.
//! That version is the record's own format, not a broker's or a protocol's,
//! so no Kafka version number is involved in reading it.
//!
//! | record | key | value |
//! |---|---|---|
//! | heartbeat | source alias, target alias | version, timestamp (ms) |
//! | checkpoint | group, topic, partition | version, upstream offset, downstream offset, metadata |
//! | offset sync | topic, partition | upstream offset, downstream offset |

use serde::Serialize;
use utoipa::ToSchema;

/// The only format version MirrorMaker has written.
const VERSION_0: i16 = 0;

/// Why a record would not decode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MirrorRecordError {
    /// The key or value is missing, or ends before its fields do.
    #[error("the record ends before its fields do")]
    Truncated,
    /// A string field is not UTF-8.
    #[error("a string field is not UTF-8")]
    NotUtf8,
    /// The value opens with a format version this build does not read.
    #[error("unrecognised record format {0}")]
    UnrecognisedVersion(i16),
}

/// A heartbeat: proof that something was flowing between two aliases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Heartbeat {
    /// The alias the heartbeat says it came from.
    pub source_alias: String,
    /// The alias it was emitted towards.
    pub target_alias: String,
    /// When it was emitted, in epoch milliseconds.
    pub timestamp_ms: i64,
}

/// A checkpoint: where a consumer group on the source would resume on the
/// target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    /// The consumer group.
    pub group: String,
    /// The topic, by its **source** name.
    pub topic: String,
    /// The partition.
    pub partition: i32,
    /// The group's committed offset on the source.
    pub upstream_offset: i64,
    /// The translated offset on the target.
    pub downstream_offset: i64,
    /// The commit's metadata string, verbatim.
    pub metadata: String,
}

/// An offset sync: an upstream offset and the downstream offset it landed
/// at. Sparse by design — emitted when the gap since the last one is large
/// enough — so it is a lower bound on progress, not a cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OffsetSync {
    /// The topic, by its source name.
    pub topic: String,
    /// The partition.
    pub partition: i32,
    /// The offset on the source.
    pub upstream_offset: i64,
    /// Where that record landed on the target.
    pub downstream_offset: i64,
}

/// A cursor over one buffer. Every read is bounds-checked; nothing indexes.
struct Fields<'a> {
    bytes: &'a [u8],
}

impl<'a> Fields<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], MirrorRecordError> {
        let (head, rest) = self
            .bytes
            .split_first_chunk::<N>()
            .ok_or(MirrorRecordError::Truncated)?;
        self.bytes = rest;
        Ok(*head)
    }

    fn int16(&mut self) -> Result<i16, MirrorRecordError> {
        self.take::<2>().map(i16::from_be_bytes)
    }

    fn int32(&mut self) -> Result<i32, MirrorRecordError> {
        self.take::<4>().map(i32::from_be_bytes)
    }

    fn int64(&mut self) -> Result<i64, MirrorRecordError> {
        self.take::<8>().map(i64::from_be_bytes)
    }

    fn string(&mut self) -> Result<String, MirrorRecordError> {
        let length = usize::try_from(self.int16()?).map_err(|_| MirrorRecordError::Truncated)?;
        if self.bytes.len() < length {
            return Err(MirrorRecordError::Truncated);
        }
        let (text, rest) = self.bytes.split_at(length);
        self.bytes = rest;
        String::from_utf8(text.to_vec()).map_err(|_| MirrorRecordError::NotUtf8)
    }

    /// The value's format version, refused unless it is the one we read.
    fn version_0(&mut self) -> Result<(), MirrorRecordError> {
        match self.int16()? {
            VERSION_0 => Ok(()),
            other => Err(MirrorRecordError::UnrecognisedVersion(other)),
        }
    }
}

/// Decode a heartbeat record.
pub fn heartbeat(key: Option<&[u8]>, value: Option<&[u8]>) -> Result<Heartbeat, MirrorRecordError> {
    let mut key = Fields::new(key.ok_or(MirrorRecordError::Truncated)?);
    let mut value = Fields::new(value.ok_or(MirrorRecordError::Truncated)?);
    value.version_0()?;
    Ok(Heartbeat {
        source_alias: key.string()?,
        target_alias: key.string()?,
        timestamp_ms: value.int64()?,
    })
}

/// Decode a checkpoint record.
pub fn checkpoint(
    key: Option<&[u8]>,
    value: Option<&[u8]>,
) -> Result<Checkpoint, MirrorRecordError> {
    let mut key = Fields::new(key.ok_or(MirrorRecordError::Truncated)?);
    let mut value = Fields::new(value.ok_or(MirrorRecordError::Truncated)?);
    value.version_0()?;
    Ok(Checkpoint {
        group: key.string()?,
        topic: key.string()?,
        partition: key.int32()?,
        upstream_offset: value.int64()?,
        downstream_offset: value.int64()?,
        metadata: value.string()?,
    })
}

/// Decode an offset sync record. No version header: the format has none.
pub fn offset_sync(
    key: Option<&[u8]>,
    value: Option<&[u8]>,
) -> Result<OffsetSync, MirrorRecordError> {
    let mut key = Fields::new(key.ok_or(MirrorRecordError::Truncated)?);
    let mut value = Fields::new(value.ok_or(MirrorRecordError::Truncated)?);
    Ok(OffsetSync {
        topic: key.string()?,
        partition: key.int32()?,
        upstream_offset: value.int64()?,
        downstream_offset: value.int64()?,
    })
}

/// The heartbeats topic MirrorMaker writes on every cluster it targets.
pub const HEARTBEATS: &str = "heartbeats";

/// Where checkpoints for a flow from `source` live, on its target, under the
/// default replication policy's naming with `separator`.
#[must_use]
pub fn checkpoints_topic(source: &str, separator: &str) -> String {
    format!("{source}{separator}checkpoints.internal")
}

/// Where a flow towards `target` writes its offset syncs — on the source, by
/// default.
#[must_use]
pub fn offset_syncs_topic(target: &str, separator: &str) -> String {
    format!("mm2-offset-syncs{separator}{target}{separator}internal")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode the way `org.apache.kafka.common.protocol.types` does.
    fn string(out: &mut Vec<u8>, text: &str) {
        out.extend(i16::try_from(text.len()).unwrap().to_be_bytes());
        out.extend(text.as_bytes());
    }

    #[test]
    fn a_heartbeat_decodes_as_kafka_writes_it() {
        let mut key = Vec::new();
        string(&mut key, "kaas");
        string(&mut key, "strimzi");
        let mut value = 0_i16.to_be_bytes().to_vec();
        value.extend(1_792_568_252_534_i64.to_be_bytes());
        assert_eq!(
            heartbeat(Some(&key), Some(&value)).unwrap(),
            Heartbeat {
                source_alias: "kaas".into(),
                target_alias: "strimzi".into(),
                timestamp_ms: 1_792_568_252_534,
            }
        );
    }

    #[test]
    fn a_checkpoint_decodes_as_kafka_writes_it() {
        let mut key = Vec::new();
        string(&mut key, "kaas-consumer-canary");
        string(&mut key, "kaas-canary-v1");
        key.extend(2_i32.to_be_bytes());
        let mut value = 0_i16.to_be_bytes().to_vec();
        value.extend(1500_i64.to_be_bytes());
        value.extend(1490_i64.to_be_bytes());
        string(&mut value, "");
        let decoded = checkpoint(Some(&key), Some(&value)).unwrap();
        assert_eq!(decoded.group, "kaas-consumer-canary");
        assert_eq!(decoded.topic, "kaas-canary-v1");
        assert_eq!(decoded.partition, 2);
        assert_eq!(
            (decoded.upstream_offset, decoded.downstream_offset),
            (1500, 1490)
        );
    }

    #[test]
    fn an_offset_sync_has_no_version_header() {
        let mut key = Vec::new();
        string(&mut key, "gtfs-rt-service-alerts");
        key.extend(0_i32.to_be_bytes());
        let mut value = 42_i64.to_be_bytes().to_vec();
        value.extend(17_i64.to_be_bytes());
        let decoded = offset_sync(Some(&key), Some(&value)).unwrap();
        assert_eq!(
            (decoded.upstream_offset, decoded.downstream_offset),
            (42, 17)
        );
    }

    #[test]
    fn an_unknown_version_is_reported_and_not_guessed() {
        let mut key = Vec::new();
        string(&mut key, "a");
        string(&mut key, "b");
        let mut value = 1_i16.to_be_bytes().to_vec();
        value.extend(5_i64.to_be_bytes());
        assert_eq!(
            heartbeat(Some(&key), Some(&value)),
            Err(MirrorRecordError::UnrecognisedVersion(1))
        );
    }

    #[test]
    fn short_or_garbled_bytes_are_errors_never_panics() {
        assert_eq!(
            heartbeat(None, Some(&[0, 0])),
            Err(MirrorRecordError::Truncated)
        );
        assert_eq!(
            heartbeat(Some(&[0, 9, b'a']), Some(&[0, 0, 0])),
            Err(MirrorRecordError::Truncated)
        );
        // A negative string length.
        assert_eq!(
            offset_sync(Some(&[0xff, 0xff]), Some(&[0; 16])),
            Err(MirrorRecordError::Truncated)
        );
        let mut bad = vec![0, 2, 0xff, 0xfe];
        bad.extend([0, 1, b'b']);
        assert_eq!(
            heartbeat(Some(&bad), Some(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 1])),
            Err(MirrorRecordError::NotUtf8)
        );
    }

    #[test]
    fn the_internal_topic_names_follow_the_default_policy() {
        assert_eq!(checkpoints_topic("kaas", "."), "kaas.checkpoints.internal");
        assert_eq!(
            offset_syncs_topic("kaas", "."),
            "mm2-offset-syncs.kaas.internal"
        );
    }
}
