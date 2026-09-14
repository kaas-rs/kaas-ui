//! Seven profiles, and a recommendation for each.
//!
//! The same measurements imply different configurations depending on what a
//! topic is *for*, and nothing in a log says which. So the advisor computes
//! all seven in one pass and lets the operator choose: switching costs no
//! request, and every profile carries the signal that would have suggested it
//! — measured where it can be, declared where it cannot.
//!
//! Three rules hold across every profile:
//!
//! **A divisor that can be zero yields "not enough data", never a number.**
//! An idle topic has no rate, a one-hour window has no peak-to-mean, an empty
//! topic has no bytes per record. Each of those is a `None` that reaches the
//! table as a row saying what is missing.
//!
//! **Every number shows its arithmetic.** The `why` on a row names the inputs
//! it used and where each came from, because the operator cannot tell whether
//! an assumption matches their topic unless they can see it.
//!
//! **The irreversible changes say so.** Partition count only increases;
//! increasing it on a keyed topic permanently breaks per-key ordering; on a
//! Streams topic it invalidates every changelog. Those are `caution`, not
//! prose buried in the derivation.

use serde::Serialize;
use utoipa::ToSchema;

use crate::analysis::TopicAnalysis;
use crate::dto::ConfigEntryDto;

use super::units::{human_bytes, human_ms, i64_to_f64, round_to_power_of_two, to_f64, to_i64};
use super::{is_explicit, key, limit, number, policy, text};

const KIB: i64 = 1024;
const MIB: i64 = KIB * KIB;
const SECOND_MS: i64 = 1_000;
const MINUTE_MS: i64 = 60 * SECOND_MS;
const HOUR_MS: i64 = 60 * MINUTE_MS;

/// The topic's shape, which is metadata rather than measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Topology {
    /// Partitions.
    pub partitions: usize,
    /// The smallest replica count across them.
    pub replication_factor: usize,
    /// Brokers in the cluster, which bounds what a replication factor may be.
    pub brokers: usize,
}

/// What the scan measured, in the units the advice reasons in.
///
/// Every field is `None` when its divisor was zero rather than `0.0`: an idle
/// topic has no write rate, and reporting one as zero would put a
/// recommendation of one partition on a topic nobody has produced to yet.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Measured {
    /// Records the scan folded.
    pub records: u64,
    /// Whether the scan read the whole retained topic. A capped scan measures
    /// a sample, and every rate below describes that sample.
    pub complete: bool,
    /// The span the folded records cover, milliseconds.
    pub window_ms: Option<i64>,
    /// The resolution the peak was measured at.
    ///
    /// The scan buckets by hour, so a burst inside one hour is invisible here
    /// and `peakRecordsPerSec` is an hourly average rather than an
    /// instantaneous rate. Carried so the UI can say so rather than implying
    /// a precision the fold does not have.
    pub bucket_ms: i64,
    /// Records per second across the whole window.
    pub mean_records_per_sec: Option<f64>,
    /// Records per second in the busiest bucket.
    pub peak_records_per_sec: Option<f64>,
    /// Peak over mean. Above the burst ratio, one number cannot size a topic.
    pub peak_to_mean: Option<f64>,
    /// Bytes on disk per record.
    pub bytes_per_record: Option<f64>,
    /// Where that figure came from — log directories, or the payload sums.
    pub bytes_per_record_source: String,
    /// Key plus value bytes per record, before compression and framing.
    pub payload_bytes_per_record: Option<f64>,
    /// Payload bytes over on-disk bytes.
    ///
    /// Above 1 means the log is smaller than what was produced into it, which
    /// is compression working. Near 1 on a compressible payload means it is
    /// not turned on. Below 1 means per-record overhead outweighs the
    /// payload, which happens on very small records.
    pub compression_ratio: Option<f64>,
    /// Bytes per second across the window.
    pub mean_bytes_per_sec: Option<f64>,
    /// Bytes per second in the busiest bucket.
    pub peak_bytes_per_sec: Option<f64>,
    /// The busiest partition's records over the mean partition's.
    pub partition_skew: Option<f64>,
    /// Records that carried a key.
    pub keyed_fraction: Option<f64>,
    /// Distinct keys over records — compaction headroom, from a sketch.
    pub distinct_key_fraction: Option<f64>,
    /// Records with a null value.
    pub tombstone_fraction: Option<f64>,
    /// Which clock stamped the timestamps the rates were derived from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
}

impl Measured {
    /// Derive the measurement from a finished scan.
    #[must_use]
    pub fn of(scan: &TopicAnalysis, disk_bytes: Option<i64>) -> Self {
        let totals = &scan.total_stats;
        let records = totals.total_msgs;
        let window_ms = match (totals.min_timestamp, totals.max_timestamp) {
            (Some(low), Some(high)) if high > low => Some(high - low),
            _ => None,
        };

        let mean_records_per_sec = window_ms.and_then(|span| {
            let seconds = i64_to_f64(span) / 1000.0;
            (seconds > 0.0).then(|| to_f64(records) / seconds)
        });
        let peak_records_per_sec = totals
            .hourly_msg_counts
            .iter()
            .map(|bucket| bucket.count)
            .max()
            .map(|busiest| to_f64(busiest) / (i64_to_f64(HOUR_MS) / 1000.0));
        let peak_to_mean = match (peak_records_per_sec, mean_records_per_sec) {
            (Some(peak), Some(mean)) if mean > 0.0 => Some(peak / mean),
            _ => None,
        };

        let payload_bytes = totals
            .key_size
            .as_ref()
            .map_or(0, |size| size.sum)
            .saturating_add(totals.value_size.as_ref().map_or(0, |size| size.sum));
        let payload_bytes_per_record =
            (records > 0).then(|| to_f64(payload_bytes) / to_f64(records));

        // On-disk bytes are what `segment.bytes` counts, so they are the
        // figure to size a segment from. The payload sums are the fallback,
        // and they are *smaller* than the truth — no framing, no headers, no
        // batch overhead — which is worth naming on the row rather than
        // silently under-sizing a segment.
        let (bytes_per_record, source) = match (disk_bytes, records) {
            (Some(disk), count) if count > 0 && disk > 0 => (
                Some(i64_to_f64(disk) / to_f64(count)),
                "log directories, post-compression",
            ),
            _ => (
                payload_bytes_per_record,
                "payload sums — no framing or compression, so an underestimate",
            ),
        };

        let compression_ratio = match (disk_bytes, payload_bytes) {
            (Some(disk), payload) if disk > 0 && payload > 0 => {
                Some(to_f64(payload) / i64_to_f64(disk))
            }
            _ => None,
        };

        let scale = |rate: Option<f64>| match (rate, bytes_per_record) {
            (Some(rate), Some(bytes)) => Some(rate * bytes),
            _ => None,
        };

        let partition_skew = partition_skew(scan);
        let fraction = |part: u64| (records > 0).then(|| to_f64(part) / to_f64(records));

        Self {
            records,
            complete: scan.complete,
            window_ms,
            bucket_ms: HOUR_MS,
            mean_records_per_sec,
            peak_records_per_sec,
            peak_to_mean,
            bytes_per_record,
            bytes_per_record_source: source.to_owned(),
            payload_bytes_per_record,
            compression_ratio,
            mean_bytes_per_sec: scale(mean_records_per_sec),
            peak_bytes_per_sec: scale(peak_records_per_sec),
            partition_skew,
            keyed_fraction: fraction(records.saturating_sub(totals.null_keys)),
            distinct_key_fraction: fraction(totals.approx_uniq_keys),
            tombstone_fraction: fraction(totals.null_values),
            clock: scan.clock.clone(),
        }
    }
}

fn partition_skew(scan: &TopicAnalysis) -> Option<f64> {
    let counts: Vec<u64> = scan
        .partition_stats
        .iter()
        .map(|stats| stats.total_msgs)
        .collect();
    if counts.len() < 2 {
        return None;
    }
    let total: u64 = counts.iter().copied().fold(0, u64::saturating_add);
    let busiest = counts.iter().copied().max()?;
    let mean = to_f64(total) / to_f64(u64::try_from(counts.len()).unwrap_or(1));
    (mean > 0.0).then(|| to_f64(busiest) / mean)
}

/// The numbers nothing in a log can supply.
///
/// Every one of these is a **default, not a measurement**, and the UI renders
/// them beside the recommendations for exactly that reason. The per-partition
/// capacities in particular span an order of magnitude across published
/// figures depending on record size, replication, acks and disk; these are
/// deliberately conservative, so a recommendation derived from them errs
/// toward more partitions rather than fewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Assumptions {
    /// What one partition is assumed to absorb, MiB per second.
    pub produce_mib_per_sec_per_partition: i64,
    /// What one consumer instance is assumed to keep up with, MiB per second.
    pub consume_mib_per_sec_per_consumer: i64,
    /// Headroom over the measured peak, as a percentage.
    pub headroom_percent: i64,
    /// How many segments a retention window should be divided into.
    pub segments_per_retention: i64,
    /// The same, for a profile that wants fewer and larger files.
    pub throughput_segments_per_retention: i64,
    /// How long a compacted topic's active segment may stay open, minutes.
    pub compacted_roll_minutes: i64,
    /// How far past `retention.ms` a record may live, as a percentage, when
    /// the profile is solving for a deletion deadline.
    pub overshoot_percent: i64,
    /// The smallest segment worth recommending, MiB.
    pub min_segment_mib: i64,
    /// The replica count to aim for, bounded by the broker count.
    pub target_replication: i64,
    /// The peak-over-mean above which one number cannot size a topic.
    pub burst_ratio: i64,
    /// The byte rate above which the throughput profile's signal fires, MiB
    /// per second.
    pub high_throughput_mib_per_sec: i64,
    /// How short a `retention.ms` has to be before missing it by a segment is
    /// a compliance problem rather than an accounting detail, hours.
    pub precision_window_hours: i64,
    /// The record size below which a topic reads as small-record, KiB.
    pub small_record_kib: i64,
}

impl Default for Assumptions {
    fn default() -> Self {
        Self {
            produce_mib_per_sec_per_partition: 10,
            consume_mib_per_sec_per_consumer: 5,
            headroom_percent: 150,
            segments_per_retention: 24,
            throughput_segments_per_retention: 8,
            compacted_roll_minutes: 60,
            overshoot_percent: 10,
            min_segment_mib: 16,
            target_replication: 3,
            burst_ratio: 10,
            high_throughput_mib_per_sec: 5,
            precision_window_hours: 24,
            small_record_kib: 10,
        }
    }
}

/// What a topic is for, which nothing in its log records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Profile {
    /// Moderate volume, no special constraint, never tuned.
    Balanced,
    /// CDC output, Streams state stores, config topics.
    CompactedChangelog,
    /// Logs, telemetry, clickstream.
    HighThroughput,
    /// Order events, notifications, anything user-facing.
    LowLatency,
    /// Nightly ETL, seasonal peaks.
    Bursty,
    /// Repartition and changelog topics a Streams topology owns.
    StreamsInternal,
    /// A deletion deadline to meet, whatever the throughput.
    RetentionPrecision,
}

impl Profile {
    /// Every profile, in the order the UI offers them.
    pub const ALL: &'static [Profile] = &[
        Profile::Balanced,
        Profile::CompactedChangelog,
        Profile::HighThroughput,
        Profile::LowLatency,
        Profile::Bursty,
        Profile::StreamsInternal,
        Profile::RetentionPrecision,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced / general purpose",
            Self::CompactedChangelog => "Compacted changelog",
            Self::HighThroughput => "High throughput / cost",
            Self::LowLatency => "Low latency",
            Self::Bursty => "Bursty / batch-driven",
            Self::StreamsInternal => "Kafka Streams internal",
            Self::RetentionPrecision => "Retention precision",
        }
    }

    fn optimises(self) -> &'static str {
        match self {
            Self::Balanced => {
                "Catching defaults that were never appropriate. Partitions at the throughput \
                 floor with headroom, segments derived from the write rate with `segment.ms` as \
                 a backstop."
            }
            Self::CompactedChangelog => {
                "Cleaner effectiveness and restore time, not throughput. Small segments so the \
                 cleaner has closed ones to work on."
            }
            Self::HighThroughput => {
                "Cost per byte. Bound by the file-descriptor budget and by batching efficiency, \
                 which pushes the partition count *down* toward the floor."
            }
            Self::LowLatency => {
                "Consumer parallelism. Partitions go up — at the price of fragmenting every \
                 producer batch, which can eat the gain."
            }
            Self::Bursty => {
                "The peak and the mean at once. Partitions sized on the peak, segments sized on \
                 the mean, `segment.ms` doing the real work."
            }
            Self::StreamsInternal => {
                "Restore time, and never changing the partition count: the number propagates \
                 across the topology through co-partitioning."
            }
            Self::RetentionPrecision => {
                "A deletion deadline. Solves for `segment.ms` against an SLA rather than for \
                 throughput at all."
            }
        }
    }
}

/// Which way a row points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Change {
    /// Raise it.
    Increase,
    /// Lower it — where the setting can be lowered at all.
    Decrease,
    /// It is already right, or it cannot be changed.
    Keep,
    /// Not a number: look at it and decide.
    Review,
}

/// One row of a profile's table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    /// The setting, spelled as the broker spells it.
    pub setting: String,
    /// What is in force now, with `(default)` where nobody set it.
    pub current: Option<String>,
    /// What this profile would set. `None` when the measurement did not
    /// support a number — the `why` then says which input was missing.
    pub recommended: Option<String>,
    /// Which way the row points.
    pub change: Change,
    /// The derivation: every input it used, and where each came from.
    pub why: String,
    /// What is irreversible about making the change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caution: Option<String>,
}

/// One profile's whole answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileAdvice {
    /// Which profile.
    pub profile: Profile,
    /// Its name, for a chip.
    pub label: String,
    /// What it optimises for, and what it trades away.
    pub optimises: String,
    /// Whether this topic's signals point here.
    pub matched: bool,
    /// Why it matched, or what would make it match.
    pub signal: String,
    /// The recommendations.
    pub rows: Vec<Recommendation>,
}

// ---------------------------------------------------------------------------
// Signals
// ---------------------------------------------------------------------------

/// What the topic itself says about which profile it belongs to.
#[derive(Debug, Clone, Copy)]
struct Signals {
    compacted: bool,
    deletes: bool,
    streams_suffix: bool,
    bursty: bool,
    high_throughput: bool,
    small_records: bool,
    quiet: bool,
    precision_risk: bool,
}

fn signals(
    topic: &str,
    entries: &[ConfigEntryDto],
    measured: &Measured,
    assumptions: &Assumptions,
    leader_bytes: Option<&[(i32, i64)]>,
) -> Signals {
    let (deletes, compacted) = policy(entries);
    let streams_suffix = topic.ends_with("-changelog") || topic.ends_with("-repartition");
    let bursty = measured
        .peak_to_mean
        .is_some_and(|ratio| ratio >= i64_to_f64(assumptions.burst_ratio));
    let rate_ceiling = i64_to_f64(assumptions.high_throughput_mib_per_sec * MIB);
    let small_records = measured
        .payload_bytes_per_record
        .is_some_and(|bytes| bytes <= i64_to_f64(assumptions.small_record_kib * KIB));
    let compressed = is_explicit(entries, key::COMPRESSION_TYPE)
        && text(entries, key::COMPRESSION_TYPE).is_some_and(|value| value != "none");
    let high_throughput = measured
        .peak_bytes_per_sec
        .is_some_and(|rate| rate >= rate_ceiling)
        || (compressed && small_records);
    let quiet = measured
        .mean_bytes_per_sec
        .is_some_and(|rate| rate < i64_to_f64(MIB));

    // The silent-failure combination, and all three halves have to be true.
    //
    // **Short**, first: a window measured in hours is one somebody chose for
    // a reason, and missing it by a segment is a compliance problem. Seven
    // days that become eight is arithmetic nobody is audited on, and firing
    // there would put this profile on most of the fleet.
    //
    // **Too slow to roll on bytes**, second — measured rather than
    // snapshotted. A partition currently holding less than one segment says
    // nothing on its own; what matters is whether it can *fill* one inside
    // the retention window at the rate the scan just measured.
    //
    // **And no time roll to fall back on**, third. Either roll closes the
    // segment; the risk is only real when neither fires inside the window.
    let retention_ms = limit(number(entries, key::RETENTION_MS));
    let short_retention = retention_ms
        .is_some_and(|window| window <= assumptions.precision_window_hours.saturating_mul(HOUR_MS));
    let partitions = i64::try_from(leader_bytes.unwrap_or(&[]).len())
        .unwrap_or(1)
        .max(1);
    let fills_in_time = match (
        limit(number(entries, key::SEGMENT_BYTES)),
        measured.mean_bytes_per_sec,
        retention_ms,
    ) {
        (Some(segment), Some(rate), Some(window)) if rate > 0.0 => {
            let per_partition = rate / i64_to_f64(partitions);
            i64_to_f64(segment) / per_partition * 1000.0 <= i64_to_f64(window)
        }
        // No rate, no claim: a topic nobody has produced to is not a
        // compliance risk, it is an empty topic.
        _ => true,
    };
    let time_roll_too_late = limit(number(entries, key::SEGMENT_MS))
        .zip(retention_ms)
        .is_some_and(|(segment, window)| segment >= window);
    let precision_risk = deletes && short_retention && !fills_in_time && time_roll_too_late;

    Signals {
        compacted,
        deletes,
        streams_suffix,
        bursty,
        high_throughput,
        small_records,
        quiet,
        precision_risk,
    }
}

/// Which profile the signals point at, and why.
///
/// Ordered by how unambiguous the signal is, not by how common the profile
/// is. `LowLatency` is never suggested: its signal is weak by nature — small
/// records at a low volume are equally consistent with a topic nobody cares
/// about — so it stays an operator declaration, which is what it actually is.
fn suggest(signals: &Signals, measured: &Measured, assumptions: &Assumptions) -> (Profile, String) {
    if signals.streams_suffix {
        return (
            Profile::StreamsInternal,
            "the topic name carries a Streams suffix, so the topology owns this topic and its \
             partition count is not yours to choose"
                .to_owned(),
        );
    }
    if signals.compacted {
        return (
            Profile::CompactedChangelog,
            "`cleanup.policy` names compaction, which is unambiguous: this topic is a keyed \
             state, not a stream of events"
                .to_owned(),
        );
    }
    if signals.precision_risk {
        return (
            Profile::RetentionPrecision,
            "a `retention.ms` measured in hours, on a topic writing too slowly to fill a \
             segment inside it and with a `segment.ms` no shorter — the combination that \
             retains records long past what the topic claims, while every setting on it reads \
             correctly"
                .to_owned(),
        );
    }
    if signals.bursty {
        let ratio = measured.peak_to_mean.unwrap_or_default();
        return (
            Profile::Bursty,
            format!(
                "the busiest hour ran {ratio:.1}× the mean, over the {}× that makes one number \
                 unable to size a topic",
                assumptions.burst_ratio
            ),
        );
    }
    if signals.high_throughput {
        return (
            Profile::HighThroughput,
            format!(
                "sustained bytes past {} MiB/s, or small records with compression configured",
                assumptions.high_throughput_mib_per_sec
            ),
        );
    }
    (
        Profile::Balanced,
        "no other profile's signal fired, which is the balanced profile's signal".to_owned(),
    )
}

fn signal_text(
    profile: Profile,
    signals: &Signals,
    measured: &Measured,
    a: &Assumptions,
) -> String {
    match profile {
        Profile::Balanced => {
            if signals.compacted || signals.streams_suffix || signals.bursty {
                "another profile's signal fired, which is what takes this one out of the running"
                    .to_owned()
            } else {
                "nothing else matched, which is this profile's signal".to_owned()
            }
        }
        Profile::CompactedChangelog => {
            if signals.compacted {
                let distinct = measured
                    .distinct_key_fraction
                    .map(|found| {
                        format!(
                            ", and {:.0}% of records carry a distinct key",
                            found * 100.0
                        )
                    })
                    .unwrap_or_default();
                format!("`cleanup.policy` names compaction{distinct}")
            } else {
                "`cleanup.policy` does not name compaction, so nothing here is being cleaned"
                    .to_owned()
            }
        }
        Profile::HighThroughput => match measured.peak_bytes_per_sec {
            Some(rate) => format!(
                "peak {}/s against the {} MiB/s this signal fires at",
                human_bytes(to_i64(rate).unwrap_or_default()),
                a.high_throughput_mib_per_sec
            ),
            None => "no byte rate was measurable, so this signal could not fire".to_owned(),
        },
        Profile::LowLatency => {
            let hint = if signals.small_records && signals.quiet {
                "small records at a low volume, which is suggestive and no more"
            } else {
                "nothing measured points here"
            };
            format!(
                "{hint} — latency is a requirement somebody has, not a property of a log, so \
                 this profile is only ever a declaration"
            )
        }
        Profile::Bursty => match measured.peak_to_mean {
            Some(ratio) => format!(
                "the busiest hour ran {ratio:.1}× the mean, against the {}× this signal fires at",
                a.burst_ratio
            ),
            None => {
                "the window held one bucket or none, so peak over mean is not defined".to_owned()
            }
        },
        Profile::StreamsInternal => {
            if signals.streams_suffix {
                "the name ends in `-changelog` or `-repartition`".to_owned()
            } else {
                "the name carries no Streams suffix. A topology can name its topics anything, so \
                 this is the one signal here that a rename defeats"
                    .to_owned()
            }
        }
        Profile::RetentionPrecision => {
            if signals.precision_risk {
                "a `retention.ms` in hours that neither roll can close inside".to_owned()
            } else if !signals.deletes {
                "nothing is deleted here, so there is no deletion deadline to miss".to_owned()
            } else {
                "the segment roll is keeping up with the retention window".to_owned()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The recommendations
// ---------------------------------------------------------------------------

/// Build every profile's advice, and say which one the signals point at.
pub(super) fn all(
    topic: &str,
    topology: Topology,
    entries: &[ConfigEntryDto],
    measured: &Measured,
    assumptions: &Assumptions,
    leader_bytes: Option<&[(i32, i64)]>,
) -> (Profile, String, Vec<ProfileAdvice>) {
    let signals = signals(topic, entries, measured, assumptions, leader_bytes);
    let (suggested, because) = suggest(&signals, measured, assumptions);

    let profiles = Profile::ALL
        .iter()
        .map(|profile| {
            let mut rows = vec![
                replication_row(*profile, topology, assumptions),
                partitions_row(*profile, topology, measured, assumptions),
            ];
            rows.extend(segment_rows(
                *profile,
                topology,
                entries,
                measured,
                assumptions,
            ));
            rows.extend(extra_rows(*profile, entries, measured));
            ProfileAdvice {
                profile: *profile,
                label: profile.label().to_owned(),
                optimises: profile.optimises().to_owned(),
                matched: *profile == suggested,
                signal: signal_text(*profile, &signals, measured, assumptions),
                rows,
            }
        })
        .collect();

    (suggested, because, profiles)
}

/// The replica count, and why this profile wants it.
fn replication_row(profile: Profile, topology: Topology, a: &Assumptions) -> Recommendation {
    let brokers = i64::try_from(topology.brokers).unwrap_or(i64::MAX);
    let current = i64::try_from(topology.replication_factor).unwrap_or(i64::MAX);
    let target = a.target_replication.min(brokers.max(1)).max(1);

    let rationale = match profile {
        Profile::Balanced => {
            "Three replicas is one broker of headroom: a partition stays writable with \
             `acks=all` through one failure, and readable through two."
        }
        Profile::CompactedChangelog => {
            "A changelog is the state store's only copy. At one replica a broker replacement \
             loses it outright, and the restore that would have rebuilt the store reads from \
             exactly this topic."
        }
        Profile::HighThroughput => {
            "This is where the cost is, and it is still the right number: each replica is a \
             full copy on disk *and* a full stream over the network, so the replication traffic \
             is the write rate times two. Lowering it buys disk at the price of the majority \
             `acks=all` depends on."
        }
        Profile::LowLatency => {
            "`acks=all` waits for `min.insync.replicas`, not for every replica — so three \
             replicas with a minimum of two leave a broker of headroom without adding a wait."
        }
        Profile::Bursty => {
            "A burst is when a broker is most likely to fall behind, which is exactly when the \
             ISR shrinks. Three replicas keep a minimum of two satisfiable while one catches up."
        }
        Profile::StreamsInternal => {
            "`replication.factor` in the Streams config sets this, and it applies to every \
             internal topic the topology creates. One replica turns a broker restart into a \
             full state restore for every instance."
        }
        Profile::RetentionPrecision => {
            "A deletion guarantee is per replica: every copy runs its own retention. A partition \
             whose leader moves mid-window is one whose deletion clock you cannot account for \
             unless the replicas were keeping up."
        }
    };

    let (change, note) = if brokers == 0 {
        (
            Change::Keep,
            " The broker count is unknown here, so this is the target rather than a check \
             against what the cluster can hold."
                .to_owned(),
        )
    } else if target < a.target_replication {
        (
            if current < target {
                Change::Increase
            } else {
                Change::Keep
            },
            format!(
                " This cluster has {brokers} broker(s), so {} is the most it can hold — \
                 {} is not available here.",
                target, a.target_replication
            ),
        )
    } else if current < target {
        (Change::Increase, String::new())
    } else {
        (Change::Keep, String::new())
    };

    Recommendation {
        setting: "replication factor".to_owned(),
        current: Some(current.to_string()),
        recommended: Some(target.to_string()),
        change,
        why: format!("{rationale}{note}"),
        caution: Some(
            "Not a topic configuration: changing it is a partition reassignment, which moves \
             data between brokers while the topic is live. kaas-ui cannot make one — it is \
             read-only — so this is a line for your GitOps repo, not a button."
                .to_owned(),
        ),
    }
}

/// The partition count, which is the row that is never freely reversible.
fn partitions_row(
    profile: Profile,
    topology: Topology,
    measured: &Measured,
    a: &Assumptions,
) -> Recommendation {
    let current = i64::try_from(topology.partitions).unwrap_or(1).max(1);
    let produce = a.produce_mib_per_sec_per_partition.saturating_mul(MIB);
    let consume = a.consume_mib_per_sec_per_consumer.saturating_mul(MIB);

    // Which rate and which capacity this profile sizes against.
    let (rate, capacity, headroom, basis) = match profile {
        Profile::LowLatency => (
            measured.peak_bytes_per_sec,
            consume,
            a.headroom_percent,
            format!(
                "one consumer instance at an assumed {} MiB/s",
                a.consume_mib_per_sec_per_consumer
            ),
        ),
        Profile::HighThroughput => (
            measured.peak_bytes_per_sec,
            produce,
            100,
            format!(
                "one partition at an assumed {} MiB/s, with no headroom added",
                a.produce_mib_per_sec_per_partition
            ),
        ),
        _ => (
            measured.peak_bytes_per_sec,
            produce,
            a.headroom_percent,
            format!(
                "one partition at an assumed {} MiB/s",
                a.produce_mib_per_sec_per_partition
            ),
        ),
    };

    let floor = rate.and_then(|rate| partitions_for(rate, capacity, headroom));

    // Three profiles do not size this number at all, and saying why is the
    // recommendation.
    let fixed = match profile {
        Profile::CompactedChangelog => Some(
            "Key distribution decides this, not throughput. Raising the count rehashes every \
             key to a different partition, so the same key exists in two places at once and \
             per-key ordering — the entire contract of a compacted topic — is broken for every \
             key already written."
                .to_owned(),
        ),
        Profile::StreamsInternal => Some(
            "Not yours to choose. The count comes from the topology's input topics through \
             co-partitioning, and changing it invalidates every changelog: each instance \
             discards its state store and restores from the beginning of this topic."
                .to_owned(),
        ),
        Profile::RetentionPrecision => Some(
            "Not a factor. Deletion is per segment and per partition alike, so more or fewer \
             partitions changes nothing about when a record goes."
                .to_owned(),
        ),
        _ => None,
    };

    if let Some(why) = fixed {
        return Recommendation {
            setting: "partitions".to_owned(),
            current: Some(current.to_string()),
            recommended: Some(current.to_string()),
            change: Change::Keep,
            why: match floor {
                Some(need) if need > current => format!(
                    "{why} For reference, the measured peak alone would want {need} — if that \
                     gap is real, it is an argument for a new topic, not for a reshard."
                ),
                _ => why,
            },
            caution: partition_caution(measured),
        };
    }

    let Some(need) = floor else {
        return Recommendation {
            setting: "partitions".to_owned(),
            current: Some(current.to_string()),
            recommended: None,
            change: Change::Keep,
            why: format!(
                "Not enough data: the scan measured no byte rate to size against — an idle \
                 topic, or a window too short to have a busiest bucket. Sizing would have been \
                 against {basis}."
            ),
            caution: None,
        };
    };

    let peak = rate
        .and_then(to_i64)
        .map(human_bytes)
        .unwrap_or_else(|| "an unknown rate".to_owned());
    let arithmetic = format!(
        "Peak {peak}/s ÷ {basis}, with {}% of that as headroom, is {need} partition(s)",
        headroom
    );

    if profile == Profile::HighThroughput && need < current {
        return Recommendation {
            setting: "partitions".to_owned(),
            current: Some(current.to_string()),
            recommended: Some(need.to_string()),
            change: Change::Decrease,
            why: format!(
                "{arithmetic} against the {current} configured. This profile pushes the count \
                 *down*: `batch.size` is allocated per partition, so one stream spread over \
                 {current} partitions divides every producer batch by {current} and costs a \
                 request per partition per batch. Kafka cannot lower a partition count — the \
                 only way down is a new topic — so read this as \"do not add any\"."
            ),
            caution: partition_caution(measured),
        };
    }

    let (change, why) = if need > current {
        (
            Change::Increase,
            format!("{arithmetic}, against the {current} configured."),
        )
    } else {
        (
            Change::Keep,
            format!(
                "{arithmetic}, which the {current} configured already covers. Partition count \
                 only ever increases, so a surplus stays; it costs open files and a little \
                 batching efficiency, not correctness."
            ),
        )
    };

    Recommendation {
        setting: "partitions".to_owned(),
        current: Some(current.to_string()),
        recommended: Some(need.max(current).to_string()),
        change,
        why: match profile {
            Profile::Bursty => format!(
                "{why} Sized on the peak deliberately — the mean would want {}, and a topic \
                 partitioned for its mean stalls for the length of every burst.",
                measured
                    .mean_bytes_per_sec
                    .and_then(|rate| partitions_for(rate, capacity, headroom))
                    .map_or_else(|| "an unknown number".to_owned(), |found| found.to_string())
            ),
            Profile::LowLatency => format!(
                "{why} Sized on consumer parallelism rather than on produce capacity: latency \
                 here is usually a consumer falling behind. The trade is real — every partition \
                 added divides the producer's batches, and past some point the fragmentation \
                 costs more latency than the parallelism buys."
            ),
            _ => why,
        },
        caution: partition_caution(measured),
    }
}

fn partition_caution(measured: &Measured) -> Option<String> {
    let keyed = measured.keyed_fraction.unwrap_or(0.0);
    if keyed > 0.0 {
        Some(format!(
            "A one-way door. Partition count only increases, and {:.0}% of the records scanned \
             carry a key — raising the count sends those keys to different partitions from here \
             on, so per-key ordering breaks for every key already written.",
            keyed * 100.0
        ))
    } else {
        Some(
            "A one-way door: Kafka can raise a partition count and never lower it. Nothing \
             scanned carried a key, so ordering is not at risk here."
                .to_owned(),
        )
    }
}

/// `segment.bytes` and `segment.ms`, which are one decision in two settings.
fn segment_rows(
    profile: Profile,
    topology: Topology,
    entries: &[ConfigEntryDto],
    measured: &Measured,
    a: &Assumptions,
) -> Vec<Recommendation> {
    let retention = limit(number(entries, key::RETENTION_MS));
    let current_bytes = number(entries, key::SEGMENT_BYTES);
    let current_ms = number(entries, key::SEGMENT_MS);
    let floor = a.min_segment_mib.saturating_mul(MIB);
    let ceiling = index_ceiling(entries);

    let (roll_ms, roll_why) = match profile {
        Profile::CompactedChangelog | Profile::StreamsInternal => (
            Some(a.compacted_roll_minutes.saturating_mul(MINUTE_MS)),
            format!(
                "The cleaner only ever touches closed segments, so the active one is the part \
                 of the log that is never compacted. {} minutes is the usual budget for how \
                 long a duplicate key may sit there.",
                a.compacted_roll_minutes
            ),
        ),
        Profile::RetentionPrecision => (
            retention.map(|window| (window / a.overshoot_percent.max(1)).max(MINUTE_MS)),
            match retention {
                Some(window) => format!(
                    "Solved against a deadline rather than a rate: allowing {}% overshoot on a \
                     `retention.ms` of {} leaves {} for the segment to roll in. Two broker \
                     settings add to that and are not read here — \
                     `log.retention.check.interval.ms` and `file.delete.delay.ms`.",
                    a.overshoot_percent,
                    human_ms(window),
                    human_ms((window / a.overshoot_percent.max(1)).max(MINUTE_MS))
                ),
                None => "No positive `retention.ms`, so there is no deadline to solve against."
                    .to_owned(),
            },
        ),
        Profile::HighThroughput => (
            retention
                .map(|window| (window / a.throughput_segments_per_retention.max(1)).max(MINUTE_MS)),
            format!(
                "Fewer, larger files: {} segments across the retention window rather than {}, \
                 which is the same bytes in a fraction of the open file descriptors.",
                a.throughput_segments_per_retention, a.segments_per_retention
            ),
        ),
        _ => (
            retention.map(|window| (window / a.segments_per_retention.max(1)).max(MINUTE_MS)),
            format!(
                "The retention window divided into {} segments, so deletion moves in steps of \
                 about {}% of the window rather than in one lump.",
                a.segments_per_retention,
                100 / a.segments_per_retention.max(1)
            ),
        ),
    };

    // Bursty sizes the segment on the mean, deliberately: a segment sized for
    // the peak stops rolling the moment the burst ends.
    let (rate, rate_name) = match profile {
        Profile::Bursty => (measured.mean_bytes_per_sec, "mean"),
        _ => (measured.peak_bytes_per_sec, "peak"),
    };

    let partitions = i64::try_from(topology.partitions).unwrap_or(1).max(1);
    let per_partition = rate.map(|rate| rate / i64_to_f64(partitions));

    let recommended_bytes = match (per_partition, roll_ms) {
        (Some(rate), Some(roll)) => segment_from_rate(rate, roll, floor, ceiling),
        _ => None,
    };

    let bytes_why = match (recommended_bytes, per_partition, roll_ms) {
        (Some(bytes), Some(rate), Some(roll)) => {
            let clamped = match ceiling {
                Some(cap) if bytes >= cap => format!(
                    " Clamped at the offset index's ceiling of {} — \
                     `segment.index.bytes` ÷ 8 × `index.interval.bytes` — because the index \
                     fills first and rolls the segment there whatever this says.",
                    human_bytes(cap)
                ),
                _ if bytes <= floor => format!(
                    " Raised to the {} floor: below that a partition holds thousands of files \
                     for no benefit.",
                    human_bytes(floor)
                ),
                _ => String::new(),
            };
            format!(
                "The {rate_name} rate of {}/s over {partitions} partitions is {}/s each; across \
                 a {} roll that is about {}, rounded to a power of two.{clamped}",
                to_i64(rate * i64_to_f64(partitions))
                    .map_or_else(|| "an unknown rate".to_owned(), human_bytes),
                to_i64(rate).map_or_else(|| "?".to_owned(), human_bytes),
                human_ms(roll),
                human_bytes(bytes),
            )
        }
        _ => format!(
            "Not enough data: sizing a segment needs a {rate_name} byte rate and a roll \
             interval, and one of the two was not measurable here."
        ),
    };

    let bytes_change = match (recommended_bytes, current_bytes) {
        (Some(want), Some(have)) if want > have => Change::Increase,
        (Some(want), Some(have)) if want < have => Change::Decrease,
        (Some(_), Some(_)) => Change::Keep,
        _ => Change::Keep,
    };
    let ms_change = match (roll_ms, current_ms) {
        (Some(want), Some(have)) if want > have => Change::Increase,
        (Some(want), Some(have)) if want < have => Change::Decrease,
        (Some(_), Some(_)) => Change::Keep,
        _ => Change::Keep,
    };

    vec![
        Recommendation {
            setting: key::SEGMENT_BYTES.to_owned(),
            current: current_bytes
                .map(|found| current_label(entries, key::SEGMENT_BYTES, human_bytes(found))),
            recommended: recommended_bytes.map(human_bytes),
            change: bytes_change,
            why: bytes_why,
            caution: None,
        },
        Recommendation {
            setting: key::SEGMENT_MS.to_owned(),
            current: current_ms
                .map(|found| current_label(entries, key::SEGMENT_MS, human_ms(found))),
            recommended: roll_ms.map(human_ms),
            change: ms_change,
            why: format!(
                "{roll_why} It is the backstop the byte size cannot be: a topic that goes quiet \
                 stops rolling on bytes entirely, and this is what closes the segment anyway."
            ),
            caution: None,
        },
    ]
}

/// The settings only some profiles have anything to say about.
fn extra_rows(
    profile: Profile,
    entries: &[ConfigEntryDto],
    measured: &Measured,
) -> Vec<Recommendation> {
    let mut rows = Vec::new();

    if matches!(
        profile,
        Profile::CompactedChangelog | Profile::StreamsInternal
    ) {
        let distinct = measured.distinct_key_fraction;
        let current = text(entries, key::MIN_CLEANABLE_DIRTY_RATIO).map(str::to_owned);
        let (recommended, change, why) = match distinct {
            Some(fraction) if fraction < 0.5 => (
                Some("0.1".to_owned()),
                Change::Decrease,
                format!(
                    "Only about {:.0}% of the records scanned carry a distinct key, so roughly \
                     {:.0}% of this log is superseded and waiting for the cleaner. The ratio is \
                     how dirty a log must be before cleaning starts; lowering it makes the \
                     cleaner run sooner and keeps the restore smaller.",
                    fraction * 100.0,
                    (1.0 - fraction) * 100.0
                ),
            ),
            Some(fraction) => (
                current.clone(),
                Change::Keep,
                format!(
                    "About {:.0}% of records carry a distinct key, so there is little \
                     duplication for the cleaner to remove and no reason to make it work harder.",
                    fraction * 100.0
                ),
            ),
            None => (
                None,
                Change::Keep,
                "Not enough data: the scan folded no records, so there is no duplication to \
                 measure."
                    .to_owned(),
            ),
        };
        rows.push(Recommendation {
            setting: key::MIN_CLEANABLE_DIRTY_RATIO.to_owned(),
            current,
            recommended,
            change,
            why,
            caution: None,
        });

        rows.push(Recommendation {
            setting: key::DELETE_RETENTION_MS.to_owned(),
            current: number(entries, key::DELETE_RETENTION_MS)
                .map(|found| current_label(entries, key::DELETE_RETENTION_MS, human_ms(found))),
            recommended: number(entries, key::DELETE_RETENTION_MS).map(human_ms),
            change: Change::Review,
            why: format!(
                "How long a tombstone survives compaction, and therefore the longest a consumer \
                 may be down and still learn that a key was deleted. {} It is a statement about \
                 your slowest consumer's downtime, so no measurement here can set it.",
                match measured.tombstone_fraction {
                    Some(fraction) if fraction > 0.0 => format!(
                        "About {:.1}% of the records scanned are tombstones.",
                        fraction * 100.0
                    ),
                    _ => "The scan found no tombstones on this topic.".to_owned(),
                }
            ),
            caution: None,
        });
    }

    if profile == Profile::HighThroughput {
        let current = text(entries, key::COMPRESSION_TYPE).map(str::to_owned);
        let (recommended, change, why) = match measured.compression_ratio {
            Some(ratio) if ratio < 1.2 => (
                Some("zstd".to_owned()),
                Change::Review,
                format!(
                    "The payload produced was {ratio:.2}× the bytes on disk, which is close \
                     enough to 1 that little is being compressed. On a topic whose cost is its \
                     byte volume this is usually the single largest saving available, and it \
                     costs producer CPU rather than anything structural."
                ),
            ),
            Some(ratio) => (
                current.clone(),
                Change::Keep,
                format!(
                    "The payload produced was {ratio:.2}× the bytes on disk, so compression is \
                     working. Leave it where it is."
                ),
            ),
            None => (
                None,
                Change::Review,
                "Not enough data: comparing produced bytes against bytes on disk needs both, \
                 and the log directories did not answer."
                    .to_owned(),
            ),
        };
        rows.push(Recommendation {
            setting: key::COMPRESSION_TYPE.to_owned(),
            current,
            recommended,
            change,
            why,
            caution: None,
        });
    }

    if matches!(
        profile,
        Profile::LowLatency | Profile::RetentionPrecision | Profile::CompactedChangelog
    ) {
        let current = number(entries, key::MIN_INSYNC_REPLICAS);
        rows.push(Recommendation {
            setting: key::MIN_INSYNC_REPLICAS.to_owned(),
            current: current
                .map(|found| current_label(entries, key::MIN_INSYNC_REPLICAS, found.to_string())),
            recommended: Some("2".to_owned()),
            change: match current {
                Some(found) if found >= 2 => Change::Keep,
                Some(_) => Change::Increase,
                None => Change::Review,
            },
            why: "The durability half of the replica count: with three replicas, a minimum of \
                  two means an `acks=all` write survives one broker being down and still \
                  refuses to succeed when two are. One means the leader alone can acknowledge, \
                  which makes the replication factor decorative."
                .to_owned(),
            caution: None,
        });
    }

    if profile == Profile::RetentionPrecision {
        rows.push(Recommendation {
            setting: key::RETENTION_MS.to_owned(),
            current: number(entries, key::RETENTION_MS)
                .map(|found| current_label(entries, key::RETENTION_MS, human_ms(found))),
            recommended: number(entries, key::RETENTION_MS).map(human_ms),
            change: Change::Keep,
            why: "Left alone deliberately. This profile exists because the stated window is \
                  already the requirement — what it fixes is the gap between the window and \
                  when deletion actually happens, and that gap is `segment.ms`."
                .to_owned(),
            caution: None,
        });
    }

    rows
}

// ---------------------------------------------------------------------------
// Arithmetic
// ---------------------------------------------------------------------------

/// How many partitions a rate needs, with headroom. `None` when the capacity
/// assumption is not a positive number.
fn partitions_for(rate: f64, capacity_bytes: i64, headroom_percent: i64) -> Option<i64> {
    if capacity_bytes <= 0 || !rate.is_finite() || rate < 0.0 {
        return None;
    }
    let wanted = rate * i64_to_f64(headroom_percent.max(1)) / 100.0;
    let needed = (wanted / i64_to_f64(capacity_bytes)).ceil();
    to_i64(needed).map(|found| found.max(1))
}

/// A segment size from a per-partition rate and a roll interval.
fn segment_from_rate(
    rate_per_partition: f64,
    roll_ms: i64,
    floor: i64,
    ceiling: Option<i64>,
) -> Option<i64> {
    let bytes = to_i64(rate_per_partition * i64_to_f64(roll_ms) / 1000.0)?;
    let rounded = round_to_power_of_two(bytes, floor);
    Some(match ceiling {
        Some(cap) if cap > 0 => rounded.min(cap).max(floor.min(cap)),
        _ => rounded,
    })
}

/// What the offset index can address, when the topic reports both halves.
fn index_ceiling(entries: &[ConfigEntryDto]) -> Option<i64> {
    let index_bytes = limit(number(entries, key::SEGMENT_INDEX_BYTES))?;
    let interval = limit(number(entries, key::INDEX_INTERVAL_BYTES))?;
    index_bytes
        .checked_div(super::lint::INDEX_ENTRY_BYTES)
        .and_then(|slots| slots.checked_mul(interval))
        .filter(|found| *found > 0)
}

/// A current value with `(default)` on it where nobody set it.
fn current_label(entries: &[ConfigEntryDto], name: &str, rendered: String) -> String {
    if is_explicit(entries, name) {
        rendered
    } else {
        format!("{rendered} (default)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{AnalysisStats, HourCount, SizeStats};

    fn setting(name: &str, value: &str, explicit: bool) -> ConfigEntryDto {
        ConfigEntryDto {
            name: name.to_owned(),
            value: Some(value.to_owned()),
            source: "x".to_owned(),
            is_explicit: explicit,
            is_sensitive: false,
            read_only: false,
            documentation: None,
        }
    }

    fn stats(partition: Option<i32>, records: u64, hourly: &[u64]) -> AnalysisStats {
        AnalysisStats {
            partition,
            total_msgs: records,
            min_offset: Some(0),
            max_offset: Some(records.try_into().unwrap_or(i64::MAX)),
            min_timestamp: Some(0),
            // One hour per bucket, so the window matches the hourly counts.
            max_timestamp: Some(HOUR_MS * hourly.len().try_into().unwrap_or(1)),
            missing_timestamps: 0,
            null_keys: 0,
            null_values: 0,
            approx_uniq_keys: records,
            approx_uniq_values: records,
            key_size: None,
            value_size: Some(SizeStats {
                sum: records.saturating_mul(1_024),
                min: 1_024,
                max: 1_024,
                avg: 1_024.0,
                p50: Some(1_024),
                p75: Some(1_024),
                p95: Some(1_024),
                p99: Some(1_024),
                p999: Some(1_024),
            }),
            hourly_msg_counts: hourly
                .iter()
                .enumerate()
                .map(|(hour, count)| HourCount {
                    hour_start: HOUR_MS * i64::try_from(hour).unwrap_or(0),
                    count: *count,
                })
                .collect(),
            hourly_truncated: false,
            malformed_batches: 0,
        }
    }

    fn scan(hourly: &[u64], partitions: usize) -> TopicAnalysis {
        let records: u64 = hourly.iter().copied().sum();
        let each = records / u64::try_from(partitions).unwrap_or(1).max(1);
        TopicAnalysis {
            started_at: 0,
            finished_at: 1,
            complete: true,
            stopped_by: crate::analysis::AnalysisStop::End,
            scanned_fraction: Some(1.0),
            clock: Some("logAppendTime".to_owned()),
            total_stats: stats(None, records, hourly),
            partition_stats: (0..partitions)
                .map(|index| stats(Some(i32::try_from(index).unwrap_or(0)), each, &[each]))
                .collect(),
            errors: Vec::new(),
            sizing: None,
        }
    }

    fn topology(partitions: usize, replicas: usize, brokers: usize) -> Topology {
        Topology {
            partitions,
            replication_factor: replicas,
            brokers,
        }
    }

    fn week() -> Vec<ConfigEntryDto> {
        vec![
            setting("cleanup.policy", "delete", false),
            setting("retention.ms", "604800000", false),
            setting("segment.ms", "604800000", false),
            setting("segment.bytes", "1073741824", false),
            setting("segment.index.bytes", "10485760", false),
            setting("index.interval.bytes", "4096", false),
        ]
    }

    fn advise(
        topic: &str,
        entries: &[ConfigEntryDto],
        scan: &TopicAnalysis,
        topology: Topology,
    ) -> (Profile, String, Vec<ProfileAdvice>) {
        let measured = Measured::of(scan, None);
        all(
            topic,
            topology,
            entries,
            &measured,
            &Assumptions::default(),
            None,
        )
    }

    fn rows_of(profiles: &[ProfileAdvice], profile: Profile) -> &[Recommendation] {
        &profiles
            .iter()
            .find(|entry| entry.profile == profile)
            .unwrap()
            .rows
    }

    fn row<'a>(rows: &'a [Recommendation], setting: &str) -> &'a Recommendation {
        rows.iter().find(|row| row.setting == setting).unwrap()
    }

    #[test]
    fn an_idle_topic_gets_no_number_rather_than_a_wrong_one() {
        let scan = scan(&[], 3);
        let (_, _, profiles) = advise("orders", &week(), &scan, topology(3, 3, 3));
        let partitions = row(rows_of(&profiles, Profile::Balanced), "partitions");
        assert_eq!(partitions.recommended, None);
        assert!(
            partitions.why.contains("Not enough data"),
            "{}",
            partitions.why
        );

        let segment = row(rows_of(&profiles, Profile::Balanced), "segment.bytes");
        assert_eq!(segment.recommended, None);
    }

    #[test]
    fn every_profile_answers_for_replication_partitions_and_segments() {
        let scan = scan(&[3_600, 3_600, 3_600], 3);
        let (_, _, profiles) = advise("orders", &week(), &scan, topology(3, 3, 3));
        assert_eq!(
            profiles.len(),
            7,
            "seven profiles, all computed in one pass"
        );
        for entry in &profiles {
            let names: Vec<&str> = entry.rows.iter().map(|row| row.setting.as_str()).collect();
            for wanted in [
                "replication factor",
                "partitions",
                "segment.bytes",
                "segment.ms",
            ] {
                assert!(
                    names.contains(&wanted),
                    "{:?} has no {wanted} row: {names:?}",
                    entry.profile
                );
            }
        }
    }

    #[test]
    fn compaction_is_the_signal_that_needs_no_measurement() {
        let mut entries = week();
        entries[0] = setting("cleanup.policy", "compact", true);
        let scan = scan(&[3_600], 1);
        let (suggested, because, _) = advise("state", &entries, &scan, topology(1, 3, 3));
        assert_eq!(suggested, Profile::CompactedChangelog);
        assert!(because.contains("cleanup.policy"), "{because}");
    }

    #[test]
    fn a_streams_suffix_outranks_the_policy_that_comes_with_it() {
        let mut entries = week();
        entries[0] = setting("cleanup.policy", "compact", true);
        let scan = scan(&[3_600], 1);
        let (suggested, _, _) = advise("app-store-changelog", &entries, &scan, topology(1, 3, 3));
        assert_eq!(
            suggested,
            Profile::StreamsInternal,
            "a changelog is compacted too, and the Streams warning is the one that matters"
        );
    }

    /// Nineteen quiet hours and one busy one — a nightly dump.
    ///
    /// The busy bucket is in the mean too, so the ratio a single spike can
    /// reach is bounded by the bucket count: nine quiet hours asymptote at
    /// ten and never cross it.
    fn nightly() -> Vec<u64> {
        let mut hours = vec![36; 19];
        hours.push(36_000);
        hours
    }

    #[test]
    fn a_tenfold_peak_is_measured_not_declared() {
        let scan = scan(&nightly(), 3);
        let (suggested, because, _) = advise("etl", &week(), &scan, topology(3, 3, 3));
        assert_eq!(suggested, Profile::Bursty);
        assert!(because.contains("× the mean"), "{because}");
    }

    #[test]
    fn the_bursty_profile_sizes_partitions_on_the_peak_and_segments_on_the_mean() {
        let scan = scan(&nightly(), 3);
        let (_, _, profiles) = advise("etl", &week(), &scan, topology(3, 3, 3));
        let rows = rows_of(&profiles, Profile::Bursty);
        assert!(
            row(rows, "partitions").why.contains("the mean would want"),
            "the peak/mean split is the profile: it has to say both numbers"
        );
        assert!(
            row(rows, "segment.bytes").why.contains("mean rate"),
            "a segment sized for the peak stops rolling when the burst ends"
        );
    }

    #[test]
    fn low_latency_is_never_suggested_because_its_signal_cannot_be_measured() {
        let scan = scan(&[36], 1);
        let (suggested, _, profiles) = advise("orders", &week(), &scan, topology(1, 3, 3));
        assert_ne!(suggested, Profile::LowLatency);
        let latency = profiles
            .iter()
            .find(|entry| entry.profile == Profile::LowLatency)
            .unwrap();
        assert!(latency.signal.contains("declaration"), "{}", latency.signal);
    }

    #[test]
    fn the_throughput_profile_pushes_the_count_down_and_says_it_cannot_be_lowered() {
        // 16 partitions carrying a trickle: the floor is one.
        let scan = scan(&[3_600], 16);
        let (_, _, profiles) = advise("logs", &week(), &scan, topology(16, 3, 3));
        let partitions = row(rows_of(&profiles, Profile::HighThroughput), "partitions");
        assert_eq!(partitions.change, Change::Decrease);
        assert!(partitions.why.contains("batch.size"), "{}", partitions.why);
        assert!(
            partitions.why.contains("cannot lower"),
            "a recommendation to lower a partition count has to say it is not possible: {}",
            partitions.why
        );
    }

    #[test]
    fn the_profiles_that_must_not_reshard_say_so_rather_than_sizing() {
        let mut entries = week();
        entries[0] = setting("cleanup.policy", "compact", true);
        let scan = scan(&[36_000], 1);
        let (_, _, profiles) = advise("app-store-changelog", &entries, &scan, topology(1, 3, 3));
        for profile in [Profile::CompactedChangelog, Profile::StreamsInternal] {
            let partitions = row(rows_of(&profiles, profile), "partitions");
            assert_eq!(partitions.change, Change::Keep, "{profile:?}");
            assert_eq!(partitions.current, partitions.recommended, "{profile:?}");
        }
        assert!(
            row(rows_of(&profiles, Profile::StreamsInternal), "partitions")
                .why
                .contains("changelog"),
            "the Streams warning is the whole reason that profile exists"
        );
    }

    #[test]
    fn a_keyed_topic_carries_the_ordering_warning_on_the_partition_row() {
        let mut scan = scan(&[3_600], 2);
        scan.total_stats.null_keys = 0;
        let (_, _, profiles) = advise("orders", &week(), &scan, topology(2, 3, 3));
        let caution = row(rows_of(&profiles, Profile::Balanced), "partitions")
            .caution
            .clone()
            .unwrap();
        assert!(caution.contains("one-way"), "{caution}");
        assert!(caution.contains("per-key ordering"), "{caution}");
    }

    #[test]
    fn replication_is_bounded_by_the_brokers_that_exist() {
        let scan = scan(&[3_600], 1);
        let (_, _, profiles) = advise("orders", &week(), &scan, topology(1, 1, 2));
        let replication = row(rows_of(&profiles, Profile::Balanced), "replication factor");
        assert_eq!(replication.recommended.as_deref(), Some("2"));
        assert!(
            replication.why.contains("2 broker(s)"),
            "{}",
            replication.why
        );
        assert!(
            replication
                .caution
                .as_deref()
                .is_some_and(|text| text.contains("reassignment")),
            "changing a replica count is not a config edit and must not read like one"
        );
    }

    #[test]
    fn a_week_long_retention_is_not_a_compliance_risk() {
        // Seven days that become eight is arithmetic nobody is audited on.
        // Firing here would put this profile on most of the fleet.
        let scan = scan(&[36], 1);
        let (suggested, _, _) = advise("events", &week(), &scan, topology(1, 3, 3));
        assert_ne!(suggested, Profile::RetentionPrecision);
    }

    #[test]
    fn an_hour_of_retention_nothing_can_close_inside_is() {
        let mut entries = week();
        entries[1] = setting("retention.ms", "3600000", true);
        // 36 records an hour at 1 KiB: a 1 GiB segment is decades away.
        let scan = scan(&[36], 1);
        let (suggested, because, _) = advise("audit", &entries, &scan, topology(1, 3, 3));
        assert_eq!(suggested, Profile::RetentionPrecision);
        assert!(
            because.contains("too slowly to fill a segment"),
            "{because}"
        );
    }

    #[test]
    fn a_topic_that_fills_a_segment_inside_its_window_is_not_at_risk() {
        let mut entries = week();
        entries[1] = setting("retention.ms", "3600000", true);
        entries[3] = setting("segment.bytes", "1048576", true);
        // 36k records an hour at 1 KiB fills a 1 MiB segment in seconds.
        let scan = scan(&[36_000], 1);
        let (suggested, _, _) = advise("audit", &entries, &scan, topology(1, 3, 3));
        assert_ne!(suggested, Profile::RetentionPrecision);
    }

    #[test]
    fn retention_precision_solves_for_the_roll_and_leaves_the_window_alone() {
        let mut entries = week();
        entries[1] = setting("retention.ms", "3600000", true);
        let scan = scan(&[3_600], 1);
        let (_, _, profiles) = advise("audit", &entries, &scan, topology(1, 3, 3));
        let rows = rows_of(&profiles, Profile::RetentionPrecision);
        assert_eq!(row(rows, "retention.ms").change, Change::Keep);
        let roll = row(rows, "segment.ms");
        assert_eq!(roll.recommended.as_deref(), Some("6 min"));
        assert!(
            roll.why.contains("log.retention.check.interval.ms"),
            "the two broker-side addends are not read here and have to be named: {}",
            roll.why
        );
    }

    #[test]
    fn a_segment_recommendation_stays_under_the_index_ceiling() {
        // A firehose: 100k records an hour at 1 KiB each over one partition.
        let scan = scan(&[3_600_000_000], 1);
        let (_, _, profiles) = advise("firehose", &week(), &scan, topology(1, 3, 3));
        let segment = row(rows_of(&profiles, Profile::HighThroughput), "segment.bytes");
        assert_eq!(
            segment.recommended.as_deref(),
            Some("5 GiB"),
            "clamped at segment.index.bytes / 8 x index.interval.bytes"
        );
        assert!(segment.why.contains("Clamped"), "{}", segment.why);
    }

    #[test]
    fn a_compacted_profile_reads_the_duplication_it_measured() {
        let mut entries = week();
        entries[0] = setting("cleanup.policy", "compact", true);
        entries.push(setting("min.cleanable.dirty.ratio", "0.5", false));
        let mut scan = scan(&[1_000], 1);
        // One in ten records carries a key nothing else uses.
        scan.total_stats.approx_uniq_keys = 100;
        let (_, _, profiles) = advise("state", &entries, &scan, topology(1, 3, 3));
        let ratio = row(
            rows_of(&profiles, Profile::CompactedChangelog),
            "min.cleanable.dirty.ratio",
        );
        assert_eq!(ratio.change, Change::Decrease);
        assert!(ratio.why.contains("10%"), "{}", ratio.why);
    }

    #[test]
    fn a_measured_window_of_one_bucket_has_no_peak_to_mean() {
        let scan = scan(&[3_600], 1);
        let measured = Measured::of(&scan, None);
        assert!(measured.mean_records_per_sec.is_some());
        // One bucket over one hour: peak and mean are the same, so the ratio
        // is defined but says nothing. What must never happen is a division
        // by a zero window.
        assert!(measured.peak_to_mean.is_some_and(|ratio| ratio > 0.0));
    }

    #[test]
    fn bytes_per_record_names_where_it_came_from() {
        let scan = scan(&[1_000], 1);
        let from_payload = Measured::of(&scan, None);
        assert!(
            from_payload
                .bytes_per_record_source
                .contains("underestimate"),
            "{}",
            from_payload.bytes_per_record_source
        );
        let from_disk = Measured::of(&scan, Some(4_096_000));
        assert!(
            from_disk
                .bytes_per_record_source
                .contains("log directories"),
            "{}",
            from_disk.bytes_per_record_source
        );
        assert_eq!(from_disk.bytes_per_record, Some(4_096.0));
    }
}
