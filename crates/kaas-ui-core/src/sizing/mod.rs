//! The sizing advisor.
//!
//! Two halves, and the second is why the first moved onto the statistics tab.
//!
//! **The lint** ([`lint`]) reads `DescribeConfigs` and `DescribeLogDirs` and
//! says what a topic's configuration implies about its segments — cheap,
//! always available, and only ever a statement about configuration.
//!
//! **The advice** ([`advice`]) needs a measurement, and the only measurement
//! that exists here is the statistics tab's full-topic scan. A write-rate
//! curve, a byte size per record, a peak against a mean: none of that is in
//! metadata, and all of it is in the fold the scan already produces. So the
//! advice rides on the analysis result rather than on a route of its own, and
//! a recommendation can say *measured* where it would otherwise have to say
//! *assumed*.
//!
//! What it produces is a recommendation per named profile — seven of them —
//! because the same measurements imply different configurations depending on
//! what the topic is for, and nothing in a log says which. The profiles are
//! [`advice::Profile`]; the signals that suggest one are measured where they
//! can be and declared where they cannot, and each profile says which.
//!
//! **Nothing here knows a Kafka version.** Every threshold is read from the
//! topic's own configuration — including the index ceiling, which is
//! `segment.index.bytes` and `index.interval.bytes` doing arithmetic rather
//! than a number this crate believes about a release. That is rule 2, and it
//! is also just better: a broker with a non-default index size gets an answer
//! about itself.

pub mod advice;
mod lint;
mod units;

use serde::Serialize;
use utoipa::ToSchema;

use crate::analysis::TopicAnalysis;
use crate::dto::ConfigEntryDto;

pub use advice::{Assumptions, Change, Measured, Profile, ProfileAdvice, Recommendation, Topology};

/// The configuration keys this module reasons about, spelled once.
mod key {
    pub const CLEANUP_POLICY: &str = "cleanup.policy";
    pub const RETENTION_MS: &str = "retention.ms";
    pub const RETENTION_BYTES: &str = "retention.bytes";
    pub const SEGMENT_MS: &str = "segment.ms";
    pub const SEGMENT_BYTES: &str = "segment.bytes";
    pub const SEGMENT_INDEX_BYTES: &str = "segment.index.bytes";
    pub const INDEX_INTERVAL_BYTES: &str = "index.interval.bytes";
    pub const MESSAGE_TIMESTAMP_TYPE: &str = "message.timestamp.type";
    pub const MIN_CLEANABLE_DIRTY_RATIO: &str = "min.cleanable.dirty.ratio";
    pub const DELETE_RETENTION_MS: &str = "delete.retention.ms";
    pub const MAX_MESSAGE_BYTES: &str = "max.message.bytes";
    pub const COMPRESSION_TYPE: &str = "compression.type";
    pub const MIN_INSYNC_REPLICAS: &str = "min.insync.replicas";

    /// What the report carries back, in the order it renders.
    pub const REPORTED: &[&str] = &[
        CLEANUP_POLICY,
        RETENTION_MS,
        RETENTION_BYTES,
        SEGMENT_MS,
        SEGMENT_BYTES,
        SEGMENT_INDEX_BYTES,
        INDEX_INTERVAL_BYTES,
        MIN_CLEANABLE_DIRTY_RATIO,
        DELETE_RETENTION_MS,
        MIN_INSYNC_REPLICAS,
        MESSAGE_TIMESTAMP_TYPE,
        MAX_MESSAGE_BYTES,
        COMPRESSION_TYPE,
    ];
}

/// How confident a diagnostic is, not how much it matters.
///
/// `Info` is "the configuration allows this"; `Warn` is "and the partitions
/// show it happening". See [`lint`] — a rule that cannot tell those apart
/// gets disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// The sizes confirm the configuration's implication.
    Warn,
    /// The configuration allows it; nothing measured says it is happening.
    Info,
}

/// What a diagnostic is about.
///
/// A stable name the UI keys on, so the prose can be rewritten without
/// breaking a chip, a filter or a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticCode {
    /// `segment.ms` outlives `retention.ms`, so a record can be retained for
    /// longer than the topic says.
    RetentionOutlivesItsSegment,
    /// `segment.bytes` is at least `retention.bytes`, so the size limit
    /// cannot delete anything until a second segment exists.
    RetentionBytesSmallerThanSegment,
    /// Nothing deletes: no time limit, no size limit, no compaction.
    NoRetentionAtAll,
    /// A compacted topic's active segment is never cleaned, and this one is
    /// not rolling.
    CompactedTailNeverCleaned,
    /// `segment.bytes` is above what the offset index can address, so the
    /// index rolls the segment first and the setting does not govern.
    SegmentAboveIndexCeiling,
    /// One partition holds much more than its share.
    SkewedPartitions,
}

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// What this is about.
    pub code: DiagnosticCode,
    /// How confident it is.
    pub severity: Severity,
    /// One line, for a chip.
    pub summary: String,
    /// The derivation, for the panel underneath it. Every number this names
    /// came from the cluster; none is assumed.
    pub detail: String,
    /// The partitions the evidence came from, where it came from partitions.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub partitions: Vec<i32>,
}

/// One configuration value the advice reasoned from.
///
/// Carried back rather than left for the caller to re-fetch, because a
/// recommendation that does not show its inputs is not actionable — and
/// because `is_explicit` is the difference between "1 GiB" and "1 GiB
/// (default)", which is the difference between a decision and an inheritance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingValue {
    /// The key.
    pub name: String,
    /// The effective value. `None` when the broker did not report the key at
    /// all — which is not the same as an empty value.
    pub value: Option<String>,
    /// Whether somebody set it, rather than it being inherited.
    pub is_explicit: bool,
}

/// What the partitions hold, when log directories answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SizeEvidence {
    /// The leader's copy of every measured partition, summed.
    pub logical_bytes: i64,
    /// The smallest measured partition.
    pub smallest_partition_bytes: i64,
    /// The largest.
    pub largest_partition_bytes: i64,
    /// How many partitions a broker reported a leader's copy for.
    ///
    /// Against the topic's partition count this says how much of the topic
    /// the evidence covers — a fan-out that lost a broker measures fewer.
    pub partitions_measured: usize,
}

/// The whole advisor's output for one topic.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SizingAdvice {
    /// The topic.
    pub topic: String,
    /// What the cluster is, structurally: partitions, replicas, brokers.
    pub topology: Topology,
    /// The configuration every recommendation below reasoned from.
    pub settings: Vec<SettingValue>,
    /// What the disks hold, when `DescribeLogDirs` answered.
    ///
    /// `None` means the sizes did not arrive. It never means the topic is
    /// empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sizes: Option<SizeEvidence>,
    /// What the scan measured, in the units the advice reasons in.
    pub measured: Measured,
    /// The numbers nothing in a log can supply, with the defaults used.
    pub assumptions: Assumptions,
    /// The profile the signals point at. Never a silent choice — the UI opens
    /// on this one and says why.
    pub suggested: Profile,
    /// Why that one, in a sentence.
    pub suggested_because: String,
    /// Every profile, each with its own recommendation. All seven are
    /// computed in one pass so switching between them costs no request.
    pub profiles: Vec<ProfileAdvice>,
    /// The configuration lint, which needs no scan and is profile-independent.
    pub diagnostics: Vec<Diagnostic>,
}

impl SizingAdvice {
    /// Build the advice for one topic.
    ///
    /// `leader_bytes` is `(partition, the leader's copy)` for the partitions a
    /// broker answered for — the log-dirs fan-out is allowed to be short, and
    /// a short answer narrows the evidence rather than voiding it.
    #[must_use]
    pub fn build(
        topic: impl Into<String>,
        topology: Topology,
        entries: &[ConfigEntryDto],
        leader_bytes: Option<&[(i32, i64)]>,
        scan: &TopicAnalysis,
    ) -> Self {
        let topic = topic.into();
        let sizes = leader_bytes.and_then(lint::evidence);
        let measured = Measured::of(scan, sizes.map(|found| found.logical_bytes));
        let assumptions = Assumptions::default();
        let (suggested, suggested_because, profiles) = advice::all(
            &topic,
            topology,
            entries,
            &measured,
            &assumptions,
            leader_bytes,
        );

        Self {
            topic,
            topology,
            settings: settings(entries),
            sizes,
            measured,
            assumptions,
            suggested,
            suggested_because,
            profiles,
            diagnostics: lint::all(entries, leader_bytes),
        }
    }
}

// ---------------------------------------------------------------------------
// Reading the configuration
// ---------------------------------------------------------------------------

fn entry<'a>(entries: &'a [ConfigEntryDto], name: &str) -> Option<&'a ConfigEntryDto> {
    entries.iter().find(|found| found.name == name)
}

/// A numeric setting's effective value.
///
/// `None` covers "not reported", "redacted" and "not a number" alike: all
/// three mean this module cannot reason about it, and inventing a default
/// here would be a version table by another name.
fn number(entries: &[ConfigEntryDto], name: &str) -> Option<i64> {
    entry(entries, name)?.value.as_deref()?.trim().parse().ok()
}

fn text<'a>(entries: &'a [ConfigEntryDto], name: &str) -> Option<&'a str> {
    entry(entries, name)?.value.as_deref()
}

fn is_explicit(entries: &[ConfigEntryDto], name: &str) -> bool {
    entry(entries, name).is_some_and(|found| found.is_explicit)
}

/// Whether the value is a real limit rather than "unlimited".
///
/// Kafka spells unlimited as a negative, and every rule wants to skip that
/// case rather than compare against it.
fn limit(value: Option<i64>) -> Option<i64> {
    value.filter(|found| *found > 0)
}

/// The cleanup policy, split into the two things it can say at once.
fn policy(entries: &[ConfigEntryDto]) -> (bool, bool) {
    let raw = text(entries, key::CLEANUP_POLICY).unwrap_or("delete");
    let mut deletes = false;
    let mut compacts = false;
    for part in raw.split(',') {
        match part.trim() {
            "delete" => deletes = true,
            "compact" => compacts = true,
            _ => {}
        }
    }
    (deletes, compacts)
}

fn settings(entries: &[ConfigEntryDto]) -> Vec<SettingValue> {
    key::REPORTED
        .iter()
        .filter_map(|name| {
            entry(entries, name).map(|found| SettingValue {
                name: found.name.clone(),
                value: found.value.clone(),
                is_explicit: found.is_explicit,
            })
        })
        .collect()
}
