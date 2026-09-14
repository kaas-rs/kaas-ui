//! The six configuration rules.
//!
//! **Severity carries confidence, not importance.** Every rule here has a
//! configuration half — cheap, always available, and on its own only a
//! suspicion — and a size half that confirms it. A topic whose `segment.ms`
//! outlives its `retention.ms` is *usually fine*, because the byte roll fires
//! first and the segment is deleted on time; it is a real problem only when
//! the partition is too small to have ever filled a segment. So the config
//! half emits `info` and the pair emits `warn`. A lint that warned on the
//! config alone would fire on most topics in the fleet and be turned off.

use crate::dto::ConfigEntryDto;

use super::units::{holds, human_bytes, human_ms};
use super::{
    Diagnostic, DiagnosticCode, Severity, SizeEvidence, entry, is_explicit, key, limit, number,
    policy,
};

/// Bytes in one offset-index entry: a relative offset and a file position,
/// four bytes each.
///
/// A property of the index format rather than of a release, which is why it
/// can be a constant here at all.
pub(super) const INDEX_ENTRY_BYTES: i64 = 8;

/// How much larger than the mean a partition must be to be called skewed.
///
/// Integer, and applied as a multiplication rather than a ratio, so nothing
/// in this module divides by a partition count that could be zero.
const SKEW_FACTOR: i64 = 2;

/// Run every rule, worst first.
pub(super) fn all(
    entries: &[ConfigEntryDto],
    leader_bytes: Option<&[(i32, i64)]>,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    retention_outlives_its_segment(entries, leader_bytes, &mut out);
    retention_bytes_smaller_than_segment(entries, &mut out);
    no_retention_at_all(entries, &mut out);
    compacted_tail_never_cleaned(entries, leader_bytes, &mut out);
    segment_above_index_ceiling(entries, &mut out);
    skewed_partitions(leader_bytes, &mut out);
    // Warnings first, and stable within a severity: the chips render in this
    // order and a row that moves between refreshes reads as a change.
    out.sort_by_key(|found| found.severity == Severity::Info);
    out
}

/// What the partitions hold, summarised.
pub(super) fn evidence(leader_bytes: &[(i32, i64)]) -> Option<SizeEvidence> {
    let mut total: i64 = 0;
    let mut smallest = i64::MAX;
    let mut largest = i64::MIN;
    for (_, bytes) in leader_bytes {
        total = total.saturating_add(*bytes);
        smallest = smallest.min(*bytes);
        largest = largest.max(*bytes);
    }
    (!leader_bytes.is_empty()).then_some(SizeEvidence {
        logical_bytes: total,
        smallest_partition_bytes: smallest,
        largest_partition_bytes: largest,
        partitions_measured: leader_bytes.len(),
    })
}
/// The partitions holding something, but less than one whole segment.
///
/// The evidence that a byte roll has not fired inside the current window:
/// a partition whose entire retained log is smaller than `segment.bytes` has
/// at most one segment, and the one it has is the active one.
///
/// **An empty partition is not evidence.** It has also never rolled, and it
/// proves nothing: there is no record to be retained past its time and none
/// to compact. Counting it would put a warning on every topic nobody has
/// produced to yet, which is the fastest way to have this feature turned off.
fn below_one_segment(leader_bytes: Option<&[(i32, i64)]>, segment_bytes: i64) -> Vec<i32> {
    leader_bytes
        .unwrap_or(&[])
        .iter()
        .filter(|(_, bytes)| *bytes > 0 && *bytes < segment_bytes)
        .map(|(partition, _)| *partition)
        .collect()
}

// ---------------------------------------------------------------------------
// The rules
// ---------------------------------------------------------------------------

/// A record can outlive `retention.ms` by a whole segment, and here it does.
fn retention_outlives_its_segment(
    entries: &[ConfigEntryDto],
    leader_bytes: Option<&[(i32, i64)]>,
    out: &mut Vec<Diagnostic>,
) {
    let (deletes, _) = policy(entries);
    if !deletes {
        return;
    }
    let (Some(retention_ms), Some(segment_ms)) = (
        limit(number(entries, key::RETENTION_MS)),
        limit(number(entries, key::SEGMENT_MS)),
    ) else {
        return;
    };
    if segment_ms < retention_ms {
        return;
    }
    // Strictly longer is somebody's decision; equal is what every topic at
    // defaults looks like. Both double the worst-case age and both are worth
    // saying; only the first is worth warning about, or the chip fires on
    // most of the fleet and stops being read.
    let configured_longer = segment_ms > retention_ms;

    let segment_bytes = limit(number(entries, key::SEGMENT_BYTES));
    let stuck = segment_bytes
        .map(|bytes| below_one_segment(leader_bytes, bytes))
        .unwrap_or_default();
    let measured = leader_bytes.is_some_and(|rows| !rows.is_empty());
    let carries_records = leader_bytes.is_some_and(|rows| rows.iter().any(|(_, held)| *held > 0));
    let severity = if stuck.is_empty() || !configured_longer {
        Severity::Info
    } else {
        Severity::Warn
    };

    let floor = retention_ms.saturating_add(segment_ms);
    let mut detail = format!(
        "Deletion works on closed segments, so the worst-case age of a record here is at least \
         `retention.ms` ({}) plus the time it takes the segment holding it to roll. `segment.ms` \
         is {}{}, so that floor is {} — before `log.retention.check.interval.ms` and \
         `file.delete.delay.ms`, which are broker settings this report does not read.",
        human_ms(retention_ms),
        human_ms(segment_ms),
        default_note(entries, key::SEGMENT_MS),
        human_ms(floor),
    );

    match (segment_bytes, stuck.is_empty(), measured) {
        (Some(bytes), false, _) => {
            let (phrase, segments, pronoun) = holds(stuck.len(), leader_bytes.unwrap_or(&[]).len());
            detail.push_str(&format!(
                " And {phrase} less than one `segment.bytes` ({}), so the size roll has not fired \
                 there either: {segments}, and nothing in {pronoun} is being deleted. Setting \
                 `segment.ms` at or below {} would close {pronoun} on time.",
                human_bytes(bytes),
                human_ms(retention_ms),
            ));
        }
        (_, true, true) if !carries_records => detail.push_str(
            " Every measured partition is empty, so nothing is being retained past its time yet. \
             This becomes a question the first time the topic carries records and stays quiet \
             enough not to fill a segment.",
        ),
        (Some(bytes), true, true) => detail.push_str(&format!(
            " Every measured partition already holds more than one `segment.bytes` ({}), so the \
             size roll is firing and retention is probably keeping up. Volume is what makes that \
             true; it stops being true if the topic goes quiet.",
            human_bytes(bytes),
        )),
        _ => detail.push_str(
            " No partition sizes were read, so whether the size roll is covering for this is \
             unknown — it usually is on a busy topic and never is on a quiet one.",
        ),
    }

    out.push(Diagnostic {
        code: DiagnosticCode::RetentionOutlivesItsSegment,
        severity,
        summary: if configured_longer {
            format!(
                "`segment.ms` ({}) outlives `retention.ms` ({})",
                human_ms(segment_ms),
                human_ms(retention_ms)
            )
        } else {
            format!(
                "`segment.ms` matches `retention.ms` ({}), so a record can live twice that",
                human_ms(retention_ms)
            )
        },
        detail,
        partitions: stuck,
    });
}

/// A size limit that one segment can swallow whole.
fn retention_bytes_smaller_than_segment(entries: &[ConfigEntryDto], out: &mut Vec<Diagnostic>) {
    let (deletes, _) = policy(entries);
    if !deletes {
        return;
    }
    let (Some(retention_bytes), Some(segment_bytes)) = (
        limit(number(entries, key::RETENTION_BYTES)),
        limit(number(entries, key::SEGMENT_BYTES)),
    ) else {
        return;
    };
    if segment_bytes < retention_bytes {
        return;
    }

    out.push(Diagnostic {
        code: DiagnosticCode::RetentionBytesSmallerThanSegment,
        severity: Severity::Warn,
        summary: format!(
            "`segment.bytes` ({}) is at least `retention.bytes` ({})",
            human_bytes(segment_bytes),
            human_bytes(retention_bytes)
        ),
        detail: format!(
            "`retention.bytes` is {} per partition and `segment.bytes` is {}{}. The active \
             segment is never deleted, so the size limit cannot remove anything until a partition \
             holds a second segment — by which point it holds at least {}, which is more than the \
             limit asks for. A `segment.bytes` well below `retention.bytes` is what makes the \
             limit mean what it says.",
            human_bytes(retention_bytes),
            human_bytes(segment_bytes),
            default_note(entries, key::SEGMENT_BYTES),
            human_bytes(segment_bytes),
        ),
        partitions: Vec::new(),
    });
}

/// Nothing deletes and nothing compacts.
fn no_retention_at_all(entries: &[ConfigEntryDto], out: &mut Vec<Diagnostic>) {
    let (deletes, compacts) = policy(entries);
    if compacts || !deletes {
        return;
    }
    // The broker must have *reported* the keys. An absent `retention.ms` is
    // a describe that did not answer, not a topic with no limit, and "nothing
    // deletes" is too strong a claim to build on a missing field.
    if entry(entries, key::RETENTION_MS).is_none() && entry(entries, key::RETENTION_BYTES).is_none()
    {
        return;
    }
    if limit(number(entries, key::RETENTION_MS)).is_some()
        || limit(number(entries, key::RETENTION_BYTES)).is_some()
    {
        return;
    }

    out.push(Diagnostic {
        code: DiagnosticCode::NoRetentionAtAll,
        severity: Severity::Info,
        summary: "nothing deletes: no time limit and no size limit".to_owned(),
        detail: "`cleanup.policy` is delete, but neither `retention.ms` nor `retention.bytes` is \
                 a positive limit, so this topic grows until the disk does not. That is a \
                 legitimate configuration for a log somebody replays from the beginning; it is \
                 also what a topic looks like when a limit was meant to be set and was not."
            .to_owned(),
        partitions: Vec::new(),
    });
}

/// The cleaner never touches the active segment, and this one is not rolling.
fn compacted_tail_never_cleaned(
    entries: &[ConfigEntryDto],
    leader_bytes: Option<&[(i32, i64)]>,
    out: &mut Vec<Diagnostic>,
) {
    let (_, compacts) = policy(entries);
    if !compacts {
        return;
    }
    let Some(segment_ms) = limit(number(entries, key::SEGMENT_MS)) else {
        return;
    };
    let segment_bytes = limit(number(entries, key::SEGMENT_BYTES));
    let stuck = segment_bytes
        .map(|bytes| below_one_segment(leader_bytes, bytes))
        .unwrap_or_default();
    // A topic whose segments are rolling is being compacted behind them; the
    // condition worth reporting is one where neither trigger fires.
    if stuck.is_empty() && (leader_bytes.is_some() || is_explicit(entries, key::SEGMENT_MS)) {
        return;
    }

    out.push(Diagnostic {
        code: DiagnosticCode::CompactedTailNeverCleaned,
        severity: if stuck.is_empty() {
            Severity::Info
        } else {
            Severity::Warn
        },
        summary: format!(
            "the compacted tail can stay uncompacted for {}",
            human_ms(segment_ms)
        ),
        detail: format!(
            "Compaction runs on closed segments only — the active one is never cleaned, so a key \
             overwritten there keeps both copies until the segment rolls. `segment.ms` is {}{}, \
             which is how long that can last{}. On a changelog read by a restoring application \
             it is also how much of the restore is duplicate work.",
            human_ms(segment_ms),
            default_note(entries, key::SEGMENT_MS),
            match segment_bytes {
                Some(bytes) if !stuck.is_empty() => {
                    let (phrase, _, _) = holds(stuck.len(), leader_bytes.unwrap_or(&[]).len());
                    format!(
                        ", and {phrase} less than one `segment.bytes` ({}), so the size roll is \
                         not closing anything either",
                        human_bytes(bytes)
                    )
                }
                _ => String::new(),
            }
        ),
        partitions: stuck,
    });
}

/// `segment.bytes` above what the offset index can address.
fn segment_above_index_ceiling(entries: &[ConfigEntryDto], out: &mut Vec<Diagnostic>) {
    let (Some(segment_bytes), Some(index_bytes), Some(interval)) = (
        limit(number(entries, key::SEGMENT_BYTES)),
        limit(number(entries, key::SEGMENT_INDEX_BYTES)),
        limit(number(entries, key::INDEX_INTERVAL_BYTES)),
    ) else {
        return;
    };
    let ceiling = index_bytes
        .checked_div(INDEX_ENTRY_BYTES)
        .and_then(|entries| entries.checked_mul(interval));
    let Some(ceiling) = ceiling.filter(|found| *found > 0 && segment_bytes > *found) else {
        return;
    };

    out.push(Diagnostic {
        code: DiagnosticCode::SegmentAboveIndexCeiling,
        severity: Severity::Info,
        summary: format!(
            "`segment.bytes` ({}) is above the index ceiling ({})",
            human_bytes(segment_bytes),
            human_bytes(ceiling)
        ),
        detail: format!(
            "The offset index holds one {INDEX_ENTRY_BYTES}-byte entry per \
             `index.interval.bytes` of log. At {} of index and an interval of {}, it addresses \
             about {} — so a segment rolls there whatever `segment.bytes` says, and the {} \
             configured is not the number that governs. Harmless, but it means a change to \
             `segment.bytes` will do nothing until it drops below the ceiling.",
            human_bytes(index_bytes),
            human_bytes(interval),
            human_bytes(ceiling),
            human_bytes(segment_bytes),
        ),
        partitions: Vec::new(),
    });
}

/// One partition carrying much more than its share.
fn skewed_partitions(leader_bytes: Option<&[(i32, i64)]>, out: &mut Vec<Diagnostic>) {
    let rows = leader_bytes.unwrap_or(&[]);
    if rows.len() < 2 {
        return;
    }
    let mut total: i64 = 0;
    for (_, bytes) in rows {
        total = total.saturating_add(*bytes);
    }
    if total <= 0 {
        return;
    }
    let Some(count) = i64::try_from(rows.len()).ok().filter(|found| *found > 0) else {
        return;
    };

    // `largest > mean * SKEW_FACTOR`, multiplied out so nothing divides.
    let mut skewed: Vec<(i32, i64)> = rows
        .iter()
        .filter(|(_, bytes)| {
            bytes
                .checked_mul(count)
                .is_some_and(|scaled| scaled > total.saturating_mul(SKEW_FACTOR))
        })
        .copied()
        .collect();
    if skewed.is_empty() {
        return;
    }
    skewed.sort_by_key(|(_, bytes)| std::cmp::Reverse(*bytes));

    let largest = skewed.first().copied().unwrap_or((0, 0));
    out.push(Diagnostic {
        code: DiagnosticCode::SkewedPartitions,
        severity: Severity::Info,
        summary: format!(
            "partition {} holds {}, well above the {} average",
            largest.0,
            human_bytes(largest.1),
            human_bytes(total / count),
        ),
        detail: format!(
            "{} of {} measured partitions hold more than {SKEW_FACTOR}× the average of {}. More \
             partitions will not fix that — the keys decide which partition a record lands in, so \
             a skew this size is a key-distribution answer rather than a partition-count one. \
             Repartitioning a keyed topic moves every key and is not reversible.",
            skewed.len(),
            rows.len(),
            human_bytes(total / count),
        ),
        partitions: skewed.into_iter().map(|(partition, _)| partition).collect(),
    });
}

// ---------------------------------------------------------------------------
// Rendering numbers a person reads
// ---------------------------------------------------------------------------

/// `" (default)"` when nobody set the key, for splicing into prose.
pub(super) fn default_note(entries: &[ConfigEntryDto], name: &str) -> &'static str {
    if is_explicit(entries, name) {
        ""
    } else {
        " (default)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::ConfigEntryDto;

    fn setting(name: &str, value: &str, explicit: bool) -> ConfigEntryDto {
        ConfigEntryDto {
            name: name.to_owned(),
            value: Some(value.to_owned()),
            source: if explicit {
                "DynamicTopicConfig".to_owned()
            } else {
                "DefaultConfig".to_owned()
            },
            is_explicit: explicit,
            is_sensitive: false,
            read_only: false,
            documentation: None,
        }
    }

    /// A day of retention against the default week-long segment roll.
    fn quiet_topic() -> Vec<ConfigEntryDto> {
        vec![
            setting("cleanup.policy", "delete", false),
            setting("retention.ms", "86400000", true),
            setting("segment.ms", "604800000", false),
            setting("segment.bytes", "1073741824", false),
        ]
    }

    /// The rules, as the advisor runs them.
    fn lint(entries: &[ConfigEntryDto], leader_bytes: Option<&[(i32, i64)]>) -> Vec<Diagnostic> {
        all(entries, leader_bytes)
    }

    fn codes(found: &[Diagnostic]) -> Vec<DiagnosticCode> {
        found.iter().map(|entry| entry.code).collect()
    }

    fn one(found: &[Diagnostic], code: DiagnosticCode) -> &Diagnostic {
        found.iter().find(|entry| entry.code == code).unwrap()
    }

    #[test]
    fn a_quiet_topic_with_no_sizes_is_only_a_suspicion() {
        let report = lint(&quiet_topic(), None);
        let diagnostic = one(&report, DiagnosticCode::RetentionOutlivesItsSegment);
        assert_eq!(
            diagnostic.severity,
            Severity::Info,
            "without sizes the byte roll might be covering for it"
        );
        assert!(diagnostic.partitions.is_empty());
    }

    #[test]
    fn sizes_below_one_segment_promote_it_to_a_warning() {
        let report = lint(&quiet_topic(), Some(&[(0, 4_096), (1, 8_192)]));
        let diagnostic = one(&report, DiagnosticCode::RetentionOutlivesItsSegment);
        assert_eq!(diagnostic.severity, Severity::Warn);
        assert_eq!(diagnostic.partitions, vec![0, 1]);
    }

    #[test]
    fn an_empty_partition_is_not_evidence_of_a_trapped_segment() {
        // It has never rolled either, and it is holding nothing that could be
        // retained past its time. Warning here fires on every topic nobody
        // has produced to yet.
        let report = lint(&quiet_topic(), Some(&[(0, 0), (1, 0)]));
        let diagnostic = one(&report, DiagnosticCode::RetentionOutlivesItsSegment);
        assert_eq!(diagnostic.severity, Severity::Info);
        assert!(diagnostic.partitions.is_empty());
        assert!(
            diagnostic.detail.contains("is empty"),
            "{}",
            diagnostic.detail
        );
    }

    #[test]
    fn one_partition_holding_something_is_still_evidence() {
        let report = lint(&quiet_topic(), Some(&[(0, 4_096), (1, 0)]));
        let diagnostic = one(&report, DiagnosticCode::RetentionOutlivesItsSegment);
        assert_eq!(diagnostic.severity, Severity::Warn);
        assert_eq!(diagnostic.partitions, vec![0]);
        assert!(
            diagnostic
                .detail
                .contains("1 of 2 measured partitions hold"),
            "{}",
            diagnostic.detail
        );
    }

    #[test]
    fn a_busy_topic_rolls_on_bytes_and_is_left_alone() {
        // Both partitions hold more than one segment, so the roll is firing.
        let report = lint(
            &quiet_topic(),
            Some(&[(0, 3_221_225_472), (1, 2_147_483_648)]),
        );
        let diagnostic = one(&report, DiagnosticCode::RetentionOutlivesItsSegment);
        assert_eq!(diagnostic.severity, Severity::Info);
        assert!(diagnostic.detail.contains("size roll is firing"));
    }

    #[test]
    fn a_topic_at_defaults_is_a_note_however_small_its_partitions() {
        // retention.ms and segment.ms both a week, one partition holding a few
        // kilobytes: the worst-case age really is two weeks, and this is also
        // what most of the fleet looks like. Saying so is right; warning about
        // it is how a chip gets ignored.
        let entries = vec![
            setting("cleanup.policy", "delete", false),
            setting("retention.ms", "604800000", false),
            setting("segment.ms", "604800000", false),
            setting("segment.bytes", "1073741824", false),
        ];
        let report = lint(&entries, Some(&[(0, 8_828)]));
        let diagnostic = one(&report, DiagnosticCode::RetentionOutlivesItsSegment);
        assert_eq!(diagnostic.severity, Severity::Info);
        assert!(
            diagnostic.summary.contains("twice that"),
            "{}",
            diagnostic.summary
        );
    }

    #[test]
    fn a_segment_shorter_than_retention_says_nothing() {
        let entries = vec![
            setting("cleanup.policy", "delete", false),
            setting("retention.ms", "604800000", true),
            setting("segment.ms", "3600000", true),
        ];
        let report = lint(&entries, None);
        assert!(!codes(&report).contains(&DiagnosticCode::RetentionOutlivesItsSegment));
    }

    #[test]
    fn a_size_limit_one_segment_swallows_is_a_warning() {
        let entries = vec![
            setting("cleanup.policy", "delete", false),
            setting("retention.bytes", "1073741824", true),
            setting("segment.bytes", "1073741824", false),
        ];
        let report = lint(&entries, None);
        let diagnostic = one(&report, DiagnosticCode::RetentionBytesSmallerThanSegment);
        assert_eq!(diagnostic.severity, Severity::Warn);
    }

    #[test]
    fn unlimited_is_not_a_limit_to_compare_against() {
        let entries = vec![
            setting("cleanup.policy", "delete", false),
            setting("retention.ms", "-1", false),
            setting("retention.bytes", "-1", false),
            setting("segment.ms", "604800000", false),
        ];
        let report = lint(&entries, None);
        assert_eq!(codes(&report), vec![DiagnosticCode::NoRetentionAtAll]);
    }

    #[test]
    fn a_compacted_topic_does_not_get_the_delete_rules() {
        let entries = vec![
            setting("cleanup.policy", "compact", true),
            setting("retention.ms", "86400000", true),
            setting("segment.ms", "604800000", false),
        ];
        let report = lint(&entries, None);
        let codes = codes(&report);
        assert!(!codes.contains(&DiagnosticCode::RetentionOutlivesItsSegment));
        assert!(!codes.contains(&DiagnosticCode::NoRetentionAtAll));
        assert!(codes.contains(&DiagnosticCode::CompactedTailNeverCleaned));
    }

    #[test]
    fn a_compacted_topic_whose_segments_roll_is_left_alone() {
        let entries = vec![
            setting("cleanup.policy", "compact", true),
            setting("segment.ms", "604800000", false),
            setting("segment.bytes", "1073741824", false),
        ];
        let report = lint(&entries, Some(&[(0, 4_294_967_296i64)]));
        assert!(!codes(&report).contains(&DiagnosticCode::CompactedTailNeverCleaned));
    }

    #[test]
    fn a_segment_above_the_index_ceiling_is_reported_with_the_ceiling() {
        let entries = vec![
            setting("cleanup.policy", "delete", false),
            // 10 MiB of index at 4 KiB per entry addresses about 5 GiB.
            setting("segment.index.bytes", "10485760", false),
            setting("index.interval.bytes", "4096", false),
            setting("segment.bytes", "17179869184", true),
        ];
        let report = lint(&entries, None);
        let diagnostic = one(&report, DiagnosticCode::SegmentAboveIndexCeiling);
        assert!(
            diagnostic.summary.contains("5 GiB"),
            "{}",
            diagnostic.summary
        );
    }

    #[test]
    fn a_segment_under_the_ceiling_says_nothing() {
        let entries = vec![
            setting("segment.index.bytes", "10485760", false),
            setting("index.interval.bytes", "4096", false),
            setting("segment.bytes", "1073741824", false),
        ];
        let report = lint(&entries, None);
        assert!(!codes(&report).contains(&DiagnosticCode::SegmentAboveIndexCeiling));
    }

    #[test]
    fn one_fat_partition_is_skew_and_an_even_spread_is_not() {
        let even = lint(&[], Some(&[(0, 100), (1, 110), (2, 90)]));
        assert!(!codes(&even).contains(&DiagnosticCode::SkewedPartitions));

        let lopsided = lint(&[], Some(&[(0, 10), (1, 10), (2, 900)]));
        let diagnostic = one(&lopsided, DiagnosticCode::SkewedPartitions);
        assert_eq!(diagnostic.partitions, vec![2]);
    }

    #[test]
    fn an_empty_topic_is_not_skewed() {
        let report = lint(&[], Some(&[(0, 0), (1, 0)]));
        assert!(!codes(&report).contains(&DiagnosticCode::SkewedPartitions));
    }

    #[test]
    fn evidence_summarises_what_was_measured_not_what_exists() {
        // Three partitions, two measured: the fan-out lost a broker.
        let sizes = evidence(&[(0, 10), (2, 30)]).unwrap();
        assert_eq!(sizes.partitions_measured, 2);
        assert_eq!(sizes.logical_bytes, 40);
        assert_eq!(sizes.smallest_partition_bytes, 10);
        assert_eq!(sizes.largest_partition_bytes, 30);
        assert!(
            evidence(&[]).is_none(),
            "nothing measured is not zero bytes"
        );
    }

    #[test]
    fn a_value_nobody_set_is_named_as_inherited_in_the_prose() {
        let report = lint(&quiet_topic(), None);
        assert!(
            one(&report, DiagnosticCode::RetentionOutlivesItsSegment)
                .detail
                .contains("(default)"),
            "a derivation that leans on a default must say it is one"
        );
    }

    #[test]
    fn a_topic_with_nothing_reported_lints_nothing_rather_than_guessing() {
        assert!(lint(&[], None).is_empty());
    }

    #[test]
    fn warnings_sort_above_notes() {
        let entries = vec![
            setting("cleanup.policy", "delete", false),
            setting("retention.ms", "86400000", true),
            setting("retention.bytes", "1073741824", true),
            setting("segment.ms", "604800000", false),
            setting("segment.bytes", "1073741824", false),
            setting("segment.index.bytes", "1024", false),
            setting("index.interval.bytes", "1024", false),
        ];
        let report = lint(&entries, None);
        let severities: Vec<Severity> = report.iter().map(|found| found.severity).collect();
        let mut sorted = severities.clone();
        sorted.sort_by_key(|severity| *severity == Severity::Info);
        assert_eq!(severities, sorted);
        assert!(severities.len() > 1, "this fixture trips several rules");
    }
}
