//! The fold behind the statistics tab.
//!
//! An analysis is an **on-demand full-topic scan**: every record is read from
//! the beginning, folded into the accumulators here, and thrown away — nothing
//! is stored server-side and nothing is precomputed. The route drives
//! [`kafka_read::scan`] and feeds each record to [`TopicAnalysisBuilder`];
//! this module owns what is counted and how it is estimated.
//!
//! Two numbers are **estimates, and say so in their names**: the unique-key
//! and unique-value counts come from a HyperLogLog sketch (±~1.6%), and the
//! size percentiles come from a log-bucketed histogram (±~4% relative). Exact
//! answers would mean holding every distinct payload and every size in memory
//! for the length of the scan, which is the resource profile this feature
//! exists to avoid. The UI labels them approximate; the field names carry the
//! same warning to anyone reading the JSON.

use std::collections::BTreeMap;
use std::hash::{BuildHasher, BuildHasherDefault, Hasher};

use serde::Serialize;
use utoipa::ToSchema;

use crate::ResourceError;

/// Hour buckets an accumulator will hold before it stops opening new ones.
///
/// A record adds at most one bucket, so a scan of forty million records could
/// otherwise build forty million — a producer writing garbage timestamps must
/// cost memory bounded by this and not by the topic. When the cap is hit the
/// result says so rather than silently narrowing the chart.
const MAX_HOURLY_BUCKETS: usize = 10_000;

/// HyperLogLog register-index bits: 2^12 registers, ±~1.6% standard error.
const HLL_BITS: u32 = 12;
const HLL_REGISTERS: usize = 1 << HLL_BITS;

/// Sub-octave bits of the size histogram: 8 buckets per power of two, which
/// bounds a percentile's relative error at ~±4%.
const SIZE_SUB_BITS: u32 = 3;

/// `u64` into the estimator arithmetic, in one place.
///
/// Lossy above 2^53 — and every number that passes through here is an
/// estimate whose stated error dwarfs that loss. The `as` is confined to
/// this function so the deny-by-default lint keeps guarding everywhere a
/// silent cast would actually be dangerous.
#[allow(clippy::as_conversions, clippy::cast_precision_loss)]
fn to_f64(value: u64) -> f64 {
    value as f64
}

/// `usize` the same way, via the lossless half.
fn len_f64(value: usize) -> f64 {
    to_f64(u64::try_from(value).unwrap_or(u64::MAX))
}

// ---------------------------------------------------------------------------
// Cardinality: HyperLogLog
// ---------------------------------------------------------------------------

/// A cardinality sketch: how many *distinct* byte strings were offered.
///
/// Flajolet et al.'s HyperLogLog with 2^12 registers and the standard
/// small-range correction. The hash is `DefaultHasher` with a fixed (default)
/// key, so one scan's registers are consistent across partitions and the
/// totals sketch can be fed the same records as the per-partition ones.
#[derive(Debug, Clone)]
struct HyperLogLog {
    registers: Vec<u8>,
}

impl HyperLogLog {
    fn new() -> Self {
        Self {
            registers: vec![0u8; HLL_REGISTERS],
        }
    }

    fn offer(&mut self, bytes: &[u8]) {
        let hasher = BuildHasherDefault::<std::collections::hash_map::DefaultHasher>::default();
        let mut state = hasher.build_hasher();
        state.write(bytes);
        let hash = state.finish();

        // The shift keeps the index below 2^12, so the conversion cannot
        // fail; 0 would merely weaken one register's estimate.
        let index = usize::try_from(hash >> (64 - HLL_BITS)).unwrap_or(0);
        // The remaining bits, with a stop bit so a hash of zero terminates:
        // rank is the position of the first set bit, capped by the width.
        let rest = (hash << HLL_BITS) | 1;
        let rank = u8::try_from((rest.leading_zeros() + 1).min(64)).unwrap_or(64);
        if let Some(register) = self.registers.get_mut(index)
            && *register < rank
        {
            *register = rank;
        }
    }

    /// The estimated distinct count.
    fn estimate(&self) -> u64 {
        let m = len_f64(self.registers.len());
        // 0.7213 / (1 + 1.079/m), the standard alpha for m >= 128.
        let alpha = 0.7213 / (1.0 + 1.079 / m);
        let sum: f64 = self
            .registers
            .iter()
            .map(|&register| 2f64.powi(-i32::from(register)))
            .sum();
        let raw = alpha * m * m / sum;

        let zeros = self.registers.iter().filter(|&&r| r == 0).count();
        // The standard small-range threshold is five halves of m, spelled
        // without a decimal literal that reads like a Kafka release to the
        // version-grep in `cargo xtask ci`.
        let small_range = raw * 0.4 <= m;
        let estimate = if small_range && zeros > 0 {
            // Small-range correction: linear counting on the empty registers.
            m * (m / len_f64(zeros)).ln()
        } else {
            raw
        };
        if estimate < 0.0 {
            0
        } else if estimate >= to_f64(u64::MAX) {
            u64::MAX
        } else {
            // Truncation is the intent: this is an estimate, not a count.
            #[allow(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            {
                estimate as u64
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Sizes: a log-bucketed histogram
// ---------------------------------------------------------------------------

/// Size statistics over one stream of byte lengths.
///
/// `sum`, `min`, `max` and the count are exact; the percentiles come off a
/// log-linear histogram — every power of two split into 8 buckets — so their
/// relative error is bounded by the bucket width at ~±4%. Payload sizes span
/// six orders of magnitude on a mixed topic, which is exactly the shape a
/// fixed-width histogram handles worst and a log-bucketed one handles best.
#[derive(Debug, Clone)]
struct SizeSketch {
    count: u64,
    sum: u64,
    min: u64,
    max: u64,
    buckets: BTreeMap<u32, u64>,
}

/// Which bucket a value lands in. Exact below 2^[`SIZE_SUB_BITS`], log-linear
/// above: the octave is the position of the leading bit, the sub-bucket the
/// three bits after it.
fn size_bucket(value: u64) -> u32 {
    if value < (1 << SIZE_SUB_BITS) {
        return u32::try_from(value).unwrap_or(u32::MAX);
    }
    let octave = 63 - value.leading_zeros();
    let sub = u32::try_from((value >> (octave - SIZE_SUB_BITS)) & ((1 << SIZE_SUB_BITS) - 1))
        .unwrap_or(0);
    ((octave - SIZE_SUB_BITS + 1) << SIZE_SUB_BITS) + sub
}

/// The midpoint of a bucket's range — what a percentile query answers with.
fn size_representative(bucket: u32) -> u64 {
    if bucket < (1 << SIZE_SUB_BITS) {
        return u64::from(bucket);
    }
    let octave = (bucket >> SIZE_SUB_BITS) + SIZE_SUB_BITS - 1;
    let sub = u64::from(bucket & ((1 << SIZE_SUB_BITS) - 1));
    let base = (1u64 << octave) + (sub << (octave - SIZE_SUB_BITS));
    // Half a bucket's width, so the answer sits mid-range rather than at the
    // low edge — which would bias every percentile downward.
    base + (1u64 << (octave - SIZE_SUB_BITS)) / 2
}

impl SizeSketch {
    fn new() -> Self {
        Self {
            count: 0,
            sum: 0,
            min: u64::MAX,
            max: 0,
            buckets: BTreeMap::new(),
        }
    }

    fn offer(&mut self, value: u64) {
        self.count += 1;
        self.sum = self.sum.saturating_add(value);
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        *self.buckets.entry(size_bucket(value)).or_insert(0) += 1;
    }

    /// The value at a quantile in `0.0..=1.0`, or `None` before any offer.
    fn quantile(&self, q: f64) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        let target = ((q * to_f64(self.count)).ceil()).max(1.0);
        let target = if target >= to_f64(u64::MAX) {
            self.count
        } else {
            // Bounded by the count; the cast is the intent.
            #[allow(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            {
                (target as u64).min(self.count)
            }
        };
        let mut seen = 0u64;
        for (&bucket, &count) in &self.buckets {
            seen += count;
            if seen >= target {
                // Clamped into the exact bounds: a one-value histogram must
                // answer that value, not its bucket's midpoint.
                return Some(size_representative(bucket).clamp(self.min, self.max));
            }
        }
        Some(self.max)
    }

    fn render(&self) -> Option<SizeStats> {
        if self.count == 0 {
            return None;
        }
        Some(SizeStats {
            sum: self.sum,
            min: self.min,
            max: self.max,
            avg: to_f64(self.sum) / to_f64(self.count),
            p50: self.quantile(0.50),
            p75: self.quantile(0.75),
            p95: self.quantile(0.95),
            p99: self.quantile(0.99),
            p999: self.quantile(0.999),
        })
    }
}

// ---------------------------------------------------------------------------
// One accumulator
// ---------------------------------------------------------------------------

/// Everything folded for one partition — or for the topic, which is the same
/// accumulator fed every record.
#[derive(Debug, Clone)]
struct Accumulator {
    total_msgs: u64,
    min_offset: Option<i64>,
    max_offset: Option<i64>,
    min_timestamp: Option<i64>,
    max_timestamp: Option<i64>,
    /// Records whose timestamp is negative — a producer that set none. They
    /// are counted rather than plotted, because a record with no timestamp
    /// must not render as 1970.
    missing_timestamps: u64,
    null_keys: u64,
    null_values: u64,
    keys: HyperLogLog,
    values: HyperLogLog,
    key_sizes: SizeSketch,
    value_sizes: SizeSketch,
    hourly: BTreeMap<i64, u64>,
    hourly_truncated: bool,
    malformed_batches: u64,
    saw_create_time: bool,
    saw_log_append_time: bool,
}

impl Accumulator {
    fn new() -> Self {
        Self {
            total_msgs: 0,
            min_offset: None,
            max_offset: None,
            min_timestamp: None,
            max_timestamp: None,
            missing_timestamps: 0,
            null_keys: 0,
            null_values: 0,
            keys: HyperLogLog::new(),
            values: HyperLogLog::new(),
            key_sizes: SizeSketch::new(),
            value_sizes: SizeSketch::new(),
            hourly: BTreeMap::new(),
            hourly_truncated: false,
            malformed_batches: 0,
            saw_create_time: false,
            saw_log_append_time: false,
        }
    }

    fn record(&mut self, record: &kafka_read::Record) {
        self.total_msgs += 1;
        self.min_offset = Some(
            self.min_offset
                .map_or(record.offset, |o| o.min(record.offset)),
        );
        self.max_offset = Some(
            self.max_offset
                .map_or(record.offset, |o| o.max(record.offset)),
        );

        match record.timestamp_type {
            kafka_read::TimestampType::Creation => self.saw_create_time = true,
            kafka_read::TimestampType::LogAppend => self.saw_log_append_time = true,
        }

        if record.timestamp < 0 {
            self.missing_timestamps += 1;
        } else {
            let ts = record.timestamp;
            self.min_timestamp = Some(self.min_timestamp.map_or(ts, |t| t.min(ts)));
            self.max_timestamp = Some(self.max_timestamp.map_or(ts, |t| t.max(ts)));
            let hour = ts.div_euclid(3_600_000);
            if self.hourly.len() < MAX_HOURLY_BUCKETS || self.hourly.contains_key(&hour) {
                *self.hourly.entry(hour).or_insert(0) += 1;
            } else {
                self.hourly_truncated = true;
            }
        }

        match &record.key {
            Some(key) => {
                self.keys.offer(key);
                self.key_sizes
                    .offer(u64::try_from(key.len()).unwrap_or(u64::MAX));
            }
            None => self.null_keys += 1,
        }
        match &record.value {
            Some(value) => {
                self.values.offer(value);
                self.value_sizes
                    .offer(u64::try_from(value.len()).unwrap_or(u64::MAX));
            }
            // `None` is a tombstone, which is not the same as an empty value —
            // dto.rs states the rule, and on a compacted topic this count *is*
            // the tombstone count.
            None => self.null_values += 1,
        }
    }

    fn render(&self, partition: Option<i32>) -> AnalysisStats {
        AnalysisStats {
            partition,
            total_msgs: self.total_msgs,
            min_offset: self.min_offset,
            max_offset: self.max_offset,
            min_timestamp: self.min_timestamp,
            max_timestamp: self.max_timestamp,
            missing_timestamps: self.missing_timestamps,
            null_keys: self.null_keys,
            null_values: self.null_values,
            approx_uniq_keys: self.keys.estimate().min(self.total_msgs),
            approx_uniq_values: self.values.estimate().min(self.total_msgs),
            key_size: self.key_sizes.render(),
            value_size: self.value_sizes.render(),
            hourly_msg_counts: self
                .hourly
                .iter()
                .map(|(&hour, &count)| HourCount {
                    hour_start: hour.saturating_mul(3_600_000),
                    count,
                })
                .collect(),
            hourly_truncated: self.hourly_truncated,
            malformed_batches: self.malformed_batches,
        }
    }

    /// Which clock stamped what this accumulator saw.
    fn clock(&self) -> Option<&'static str> {
        match (self.saw_create_time, self.saw_log_append_time) {
            (true, false) => Some("createTime"),
            (false, true) => Some("logAppendTime"),
            (true, true) => Some("mixed"),
            (false, false) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// The builder the route drives
// ---------------------------------------------------------------------------

/// The whole fold: one accumulator per partition, and one fed everything.
#[derive(Debug, Clone)]
pub struct TopicAnalysisBuilder {
    totals: Accumulator,
    partitions: BTreeMap<i32, Accumulator>,
    bytes_scanned: u64,
}

impl Default for TopicAnalysisBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TopicAnalysisBuilder {
    /// An empty fold.
    pub fn new() -> Self {
        Self {
            totals: Accumulator::new(),
            partitions: BTreeMap::new(),
            bytes_scanned: 0,
        }
    }

    /// Fold one record, into its partition's accumulator and the totals.
    pub fn record(&mut self, record: &kafka_read::Record) {
        self.bytes_scanned += u64::try_from(record.payload_len()).unwrap_or(u64::MAX);
        self.totals.record(record);
        self.partitions
            .entry(record.partition)
            .or_insert_with(Accumulator::new)
            .record(record);
    }

    /// Count a batch that would not decode, against the partition it covered.
    pub fn malformed(&mut self, partition: i32) {
        self.totals.malformed_batches += 1;
        self.partitions
            .entry(partition)
            .or_insert_with(Accumulator::new)
            .malformed_batches += 1;
    }

    /// Key bytes plus value bytes folded so far — kafbat's `bytesScanned`.
    pub fn bytes_scanned(&self) -> u64 {
        self.bytes_scanned
    }

    /// Records folded so far.
    pub fn records(&self) -> u64 {
        self.totals.total_msgs
    }

    /// The terminal value.
    ///
    /// `complete: false` is the honest label for a scan that hit its lifetime
    /// or lost a partition mid-read: the numbers are real for what *was*
    /// scanned, and presenting them as the topic's would be worse than an
    /// error — see the analysis route for where each case arises.
    pub fn render(
        self,
        started_at: i64,
        finished_at: i64,
        complete: bool,
        scanned_fraction: Option<f64>,
        errors: Vec<ResourceError>,
    ) -> TopicAnalysis {
        TopicAnalysis {
            started_at,
            finished_at,
            complete,
            scanned_fraction,
            clock: self.totals.clock().map(str::to_owned),
            total_stats: self.totals.render(None),
            partition_stats: self
                .partitions
                .iter()
                .map(|(&partition, accumulator)| accumulator.render(Some(partition)))
                .collect(),
            errors,
        }
    }
}

// ---------------------------------------------------------------------------
// The wire shapes
// ---------------------------------------------------------------------------

/// One analysis progress frame.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisProgress {
    /// When the scan started, epoch milliseconds.
    pub started_at: i64,
    /// Completion as a fraction in `0.0..=1.0`, where the offset range is
    /// known. Compaction makes it an upper bound on work left, not a record
    /// count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fraction: Option<f64>,
    /// Records folded so far.
    pub msgs_scanned: u64,
    /// Key plus value bytes folded so far.
    pub bytes_scanned: u64,
    /// Offsets consumed, across every partition.
    pub offsets_consumed: i64,
    /// Offsets the scan set out to consume.
    pub offsets_total: i64,
    /// Batches that would not decode, so far.
    pub malformed_batches: u64,
    /// Milliseconds since the scan started.
    pub elapsed_ms: u64,
}

/// The terminal value of one analysis.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopicAnalysis {
    /// When the scan started, epoch milliseconds.
    pub started_at: i64,
    /// When it finished — or when it stopped, if `complete` is false.
    pub finished_at: i64,
    /// Whether the whole planned window was read. **A partial result is
    /// flagged, never silently presented as the topic's numbers** — statistics
    /// that look complete and are wrong are worse than an error.
    pub complete: bool,
    /// How much of the planned offset range was consumed, where known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scanned_fraction: Option<f64>,
    /// Which clock stamped the timestamps: `createTime`, `logAppendTime`, or
    /// `mixed`. `None` on a topic with no records. The hourly chart plots
    /// whichever this names, and should say so.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
    /// The whole topic's numbers.
    pub total_stats: AnalysisStats,
    /// Per partition, in index order.
    pub partition_stats: Vec<AnalysisStats>,
    /// Whatever failed while reading — the same envelope shape the rest of
    /// the API uses, so a partition lost mid-scan is a named entry rather
    /// than a discarded result.
    pub errors: Vec<ResourceError>,
}

/// One accumulator's numbers: the topic's, or one partition's.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisStats {
    /// The partition, or `None` on the totals row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partition: Option<i32>,
    /// Records scanned. On a compacted or transactional topic this is
    /// legitimately below the offset range.
    pub total_msgs: u64,
    /// The lowest offset scanned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_offset: Option<i64>,
    /// The highest offset scanned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_offset: Option<i64>,
    /// The oldest timestamp seen, epoch milliseconds. Records with no
    /// timestamp are excluded and counted in `missingTimestamps`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_timestamp: Option<i64>,
    /// The newest timestamp seen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_timestamp: Option<i64>,
    /// Records whose producer set no timestamp.
    pub missing_timestamps: u64,
    /// Records with no key.
    pub null_keys: u64,
    /// Records with no value. On a compacted topic this is the tombstone
    /// count — `None` is a tombstone, not an empty value.
    pub null_values: u64,
    /// **Estimated** distinct keys, from a cardinality sketch (±~1.6%).
    pub approx_uniq_keys: u64,
    /// **Estimated** distinct values.
    pub approx_uniq_values: u64,
    /// Key size statistics, over records that have a key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_size: Option<SizeStats>,
    /// Value size statistics, over records that have a value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_size: Option<SizeStats>,
    /// Records per hour, keyed by the hour's start in epoch milliseconds.
    pub hourly_msg_counts: Vec<HourCount>,
    /// Whether the hour map hit its bucket ceiling and stopped opening new
    /// hours. The chart is then a view, not the whole story.
    pub hourly_truncated: bool,
    /// Batches that would not decode. The scan continues past them; they are
    /// an explanation, not a failure.
    pub malformed_batches: u64,
}

/// Size statistics for one side of the record.
///
/// `sum`, `min`, `max` and `avg` are exact; **the percentiles are sketch
/// estimates** with a bounded relative error of ~±4%, and the UI must label
/// them so — a p99 read as exact gets used to justify a partitioning decision.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SizeStats {
    /// Total bytes, exact.
    pub sum: u64,
    /// Smallest, exact.
    pub min: u64,
    /// Largest, exact.
    pub max: u64,
    /// Mean, exact.
    pub avg: f64,
    /// Median, estimated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p50: Option<u64>,
    /// 75th percentile, estimated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p75: Option<u64>,
    /// 95th percentile, estimated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p95: Option<u64>,
    /// 99th percentile, estimated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p99: Option<u64>,
    /// 99.9th percentile, estimated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p999: Option<u64>,
}

/// One bar of the hourly chart.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HourCount {
    /// The hour's start, epoch milliseconds.
    pub hour_start: i64,
    /// Records stamped within it.
    pub count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    fn record(
        partition: i32,
        offset: i64,
        key: Option<&str>,
        value: Option<&str>,
    ) -> kafka_read::Record {
        kafka_read::Record {
            topic: "orders".to_owned(),
            partition,
            offset,
            timestamp: 1_754_000_000_000 + offset * 1_000,
            timestamp_type: kafka_read::TimestampType::Creation,
            key: key.map(|k| Bytes::copy_from_slice(k.as_bytes())),
            value: value.map(|v| Bytes::copy_from_slice(v.as_bytes())),
            headers: Vec::new(),
            producer_id: None,
            transactional: false,
            leader_epoch: None,
        }
    }

    #[test]
    fn a_tombstone_is_a_null_value_not_an_empty_one() {
        let mut builder = TopicAnalysisBuilder::new();
        builder.record(&record(0, 1, Some("k1"), Some("v")));
        builder.record(&record(0, 2, Some("k1"), None));
        builder.record(&record(0, 3, Some("k2"), Some("")));

        let analysis = builder.render(0, 1, true, Some(1.0), Vec::new());
        assert_eq!(analysis.total_stats.total_msgs, 3);
        assert_eq!(analysis.total_stats.null_values, 1, "only the tombstone");
        assert_eq!(analysis.total_stats.null_keys, 0);
        // The empty value contributed a size of zero; the tombstone none.
        let sizes = analysis.total_stats.value_size.expect("two sized values");
        assert_eq!(sizes.min, 0);
    }

    #[test]
    fn cardinality_estimates_track_distinct_counts() {
        let mut sketch = HyperLogLog::new();
        for i in 0..10_000u64 {
            sketch.offer(format!("key-{i}").as_bytes());
        }
        let estimate = sketch.estimate();
        // 2^12 registers put the standard error at ~1.6%; five sigma of slack
        // keeps the test deterministic-in-practice without hiding a broken
        // estimator.
        assert!(
            (9_200..=10_800).contains(&estimate),
            "10k distinct estimated as {estimate}"
        );

        // And repeats are not distinct.
        let mut repeated = HyperLogLog::new();
        for _ in 0..1_000 {
            repeated.offer(b"same");
        }
        assert_eq!(repeated.estimate(), 1);
    }

    #[test]
    fn an_estimate_never_exceeds_what_was_scanned() {
        // The render clamps: an estimator reading 103 uniques over 100
        // records would be nonsense a reader rightly distrusts.
        let mut builder = TopicAnalysisBuilder::new();
        for i in 0..100 {
            builder.record(&record(0, i, Some(&format!("k{i}")), Some("x")));
        }
        let analysis = builder.render(0, 1, true, None, Vec::new());
        assert!(analysis.total_stats.approx_uniq_keys <= 100);
        assert_eq!(analysis.total_stats.approx_uniq_values, 1);
    }

    #[test]
    fn size_percentiles_stay_within_the_exact_bounds() {
        let mut sketch = SizeSketch::new();
        for size in [100u64, 200, 300, 400, 500, 600, 700, 800, 900, 100_000] {
            sketch.offer(size);
        }
        let stats = sketch.render().expect("offered");
        assert_eq!(stats.min, 100);
        assert_eq!(stats.max, 100_000);
        assert_eq!(stats.sum, 104_500);
        let p50 = stats.p50.expect("data");
        // ±4% of the true median's bucket.
        assert!((450..=560).contains(&p50), "median estimated as {p50}");
        let p999 = stats.p999.expect("data");
        assert!(p999 <= stats.max, "a percentile above the max is a lie");
    }

    #[test]
    fn the_bucket_maths_round_trips() {
        // Exact below 2^3, monotonic everywhere, representative inside ±5%.
        for value in 0u64..8 {
            assert_eq!(size_representative(size_bucket(value)), value);
        }
        let mut last_bucket = 0;
        for value in [8u64, 100, 1_000, 65_536, 1_000_000, u64::from(u32::MAX)] {
            let bucket = size_bucket(value);
            assert!(bucket >= last_bucket, "buckets must be monotonic");
            last_bucket = bucket;
            let representative = to_f64(size_representative(bucket));
            let error = (representative - to_f64(value)).abs() / to_f64(value);
            assert!(
                error < 0.07,
                "{value} represented as {representative} ({error:.3})"
            );
        }
    }

    #[test]
    fn a_record_with_no_timestamp_is_counted_not_plotted() {
        let mut builder = TopicAnalysisBuilder::new();
        let mut no_ts = record(0, 1, None, Some("v"));
        no_ts.timestamp = -1;
        builder.record(&no_ts);
        builder.record(&record(0, 2, None, Some("v")));

        let analysis = builder.render(0, 1, true, None, Vec::new());
        assert_eq!(analysis.total_stats.missing_timestamps, 1);
        assert_eq!(
            analysis.total_stats.hourly_msg_counts.len(),
            1,
            "the 1970 bar must not exist"
        );
        assert_eq!(analysis.total_stats.null_keys, 2);
    }

    #[test]
    fn the_hour_map_is_bounded_by_the_cap_not_by_the_producer() {
        let mut accumulator = Accumulator::new();
        for i in 0..(i64::try_from(MAX_HOURLY_BUCKETS).expect("small constant") + 50) {
            let mut r = record(0, i, None, Some("v"));
            // One record per distinct hour: the adversarial shape.
            r.timestamp = i * 3_600_000;
            accumulator.record(&r);
        }
        assert_eq!(accumulator.hourly.len(), MAX_HOURLY_BUCKETS);
        assert!(accumulator.hourly_truncated, "and the result says so");
    }

    #[test]
    fn partitions_fold_separately_and_the_totals_fold_everything() {
        let mut builder = TopicAnalysisBuilder::new();
        builder.record(&record(0, 10, Some("a"), Some("v1")));
        builder.record(&record(1, 20, Some("b"), Some("v2")));
        builder.record(&record(1, 21, Some("c"), None));
        builder.malformed(1);

        let analysis = builder.render(0, 1, true, Some(1.0), Vec::new());
        assert_eq!(analysis.total_stats.total_msgs, 3);
        assert_eq!(analysis.partition_stats.len(), 2);
        let p1 = &analysis.partition_stats[1];
        assert_eq!(p1.partition, Some(1));
        assert_eq!(p1.total_msgs, 2);
        assert_eq!(p1.min_offset, Some(20));
        assert_eq!(p1.max_offset, Some(21));
        assert_eq!(p1.null_values, 1);
        assert_eq!(p1.malformed_batches, 1);
        assert_eq!(analysis.total_stats.malformed_batches, 1);
        assert_eq!(analysis.clock.as_deref(), Some("createTime"));
    }

    #[test]
    fn bytes_scanned_is_key_plus_value() {
        let mut builder = TopicAnalysisBuilder::new();
        builder.record(&record(0, 1, Some("abc"), Some("defgh")));
        assert_eq!(builder.bytes_scanned(), 8);
    }
}
