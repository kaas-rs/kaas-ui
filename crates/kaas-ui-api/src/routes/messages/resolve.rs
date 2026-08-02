//! What a timestamp seek actually landed on.
//!
//! One `ListOffsets` per time-mode read — not per record — and it exists
//! entirely so a seek that was answered correctly but unhelpfully can be seen
//! for what it is.
//!
//! The failure this catches is quiet. `ListOffsets` answers "the first offset
//! at or after this instant", and a broker holding no timestamp index answers
//! with none. That is a valid response, and it is the same response as
//! "nothing has been written since then". kaas-lib cannot tell them apart, and
//! neither can kaas-ui — deciding that a broker "must" have an index is
//! exactly the kind of version knowledge that belongs downstairs or nowhere.
//!
//! So the answer is reported rather than interpreted. A window that comes back
//! empty alongside "14:30 resolved to: no offset on any of 16 partitions" is a
//! cluster telling you something; the same empty window on its own is a bug
//! report.

use kaas_ui_core::dto::{ResolvedPartition, ResolvedSeek};
use kafka_admin::{Admin, OffsetSpec};

/// Ask a cluster what an instant means on each partition.
///
/// Returns `None` when the read is not anchored to a time, which is five of
/// the seven modes.
pub async fn resolve(
    admin: &Admin,
    topic: &str,
    partitions: Option<&[i32]>,
    timestamp: Option<i64>,
) -> Option<ResolvedSeek> {
    let timestamp = timestamp?;

    let wanted: Vec<i32> = match partitions {
        Some(list) => list.to_vec(),
        None => admin
            .cluster()
            .snapshot()
            .topic(topic)
            .map(|info| info.partitions.iter().map(|p| p.partition).collect())
            .unwrap_or_default(),
    };
    if wanted.is_empty() {
        return None;
    }

    let keys: Vec<(String, i32)> = wanted
        .iter()
        .map(|partition| (topic.to_owned(), *partition))
        .collect();

    let listed = crate::call(
        "list_offsets",
        admin.list_offsets(keys, OffsetSpec::Timestamp(timestamp)),
    )
    .await
    .ok()?;

    let mut resolved: Vec<ResolvedPartition> = listed
        .into_iter()
        .map(|((_, partition), outcome)| match outcome {
            Ok(listed) => ResolvedPartition {
                partition,
                offset: listed.offset,
                timestamp: listed.timestamp,
                error: None,
            },
            // Per-partition, because a partition mid-election must not take
            // the other fifteen answers down with it.
            Err(error) => ResolvedPartition {
                partition,
                offset: None,
                timestamp: None,
                error: Some(error.to_string()),
            },
        })
        .collect();
    resolved.sort_by_key(|entry| entry.partition);

    let unresolved = resolved.iter().all(|entry| entry.offset.is_none());
    Some(ResolvedSeek {
        timestamp,
        partitions: resolved,
        unresolved,
    })
}
