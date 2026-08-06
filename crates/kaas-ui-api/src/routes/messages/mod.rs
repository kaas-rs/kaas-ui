//! The message browser.
//!
//! Four routes, and the split between them is the shape of the library
//! underneath rather than a taste in API design:
//!
//! | route | call | why |
//! |---|---|---|
//! | `messages/tail` | `tail` | the default topic view, one shot, cacheable |
//! | `messages/stream` | `scan` or `tail` | the seven seek modes, over SSE |
//! | `messages` | `scan` or `tail` | one page, for "load more" |
//! | `messages/{partition}/{offset}` | `scan` | the one record someone opened |
//!
//! The last one exists because the first three never send a whole payload. A
//! topic carrying 1 KB values at ten thousand records a second is 10 MB/s the
//! browser would parse, hold and never draw; the list shows one truncated line
//! per row whatever arrives, and the rest is fetched for the record that was
//! actually selected. See [`kaas_ui_core::dto::Payload::preview`].

pub mod resolve;
pub mod seek;
pub mod stream;

use axum::Json;
use axum::extract::{Path, Query, State};
use futures::StreamExt;
use kaas_ui_core::dto::{
    MalformedDetail, Message, MessageDetail, Payload, ResolvedSeek, StreamRow,
};
use kaas_ui_core::envelope::Envelope;
use kafka_read::{ScanEvent, ScanSpec, StartPosition, TailSpec};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::routes::split_list;
use kaas_ui_auth::Kind;

use crate::{ApiError, ApiResult, AppState, Caller};

pub use seek::{Plan, SeekMode, SeekQuery};
pub use stream::stream;

/// The most records one tail request will return.
///
/// Not a performance guard so much as a browser guard: a tab that receives
/// fifty thousand rows in one response stops responding, and the person who
/// asked for it cannot tell that from a hung server.
const MAX_LIMIT: usize = 5_000;

/// The tail query.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TailQuery {
    /// How many records to return, after merging. Defaults to 100.
    pub limit: Option<usize>,
    /// Restrict to these partitions, comma-separated.
    pub partitions: Option<String>,
}

/// `GET /api/clusters/{id}/topics/{topic}/messages/tail`
///
/// `TailSpec::limit` is a per-topic target that kaas-lib spreads across
/// partitions with `div_ceil`, so asking for 20 on a 16-partition topic
/// genuinely fetches 32. "The last n of a topic" has no single answer across
/// partitions, so this layer picks one and says so: **the n most recent by
/// timestamp**, merged across partitions and truncated after the fact.
/// `total` reports how many were fetched before truncation.
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/topics/{topic}/messages/tail",
    params(
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
        ("limit" = Option<usize>, Query, description = "Records to return after merging"),
        ("partitions" = Option<String>, Query, description = "Comma-separated partition list"),
    ),
    responses((status = 200, description = "The tail of a topic", body = Envelope<Message>)),
    tag = "messages",
)]
pub async fn tail(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, topic)): Path<(String, String)>,
    Query(query): Query<TailQuery>,
) -> ApiResult<Json<Envelope<Message>>> {
    let (handle, admin) = state.connected(&id, &caller)?;
    // Payloads are the sensitive surface, so this is where the `messages`
    // grant is spent — after the lookup, which already decided the cluster is
    // visible at all, and against the topic name because a role may grant
    // payload access to `public-*` and nothing else.
    caller.require_topic(&id, &handle.labels, &topic)?;

    let limit = query.limit.unwrap_or(100);
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ApiError::bad_request(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }

    let mut spec = TailSpec::new(topic.clone(), limit);
    if let Some(raw) = query.partitions.as_deref() {
        let partitions: Vec<i32> = split_list(raw)
            .iter()
            .map(|part| {
                part.parse::<i32>().map_err(|_| {
                    ApiError::bad_request(format!("partition {part:?} is not a number"))
                })
            })
            .collect::<ApiResult<Vec<i32>>>()?;
        if partitions.is_empty() {
            return Err(ApiError::bad_request("?partitions= was empty"));
        }
        spec = spec.partitions(partitions);
    }

    let tails = crate::call("tail", kafka_read::tail(admin.cluster(), &spec)).await?;

    let mut malformed = 0usize;
    let mut messages: Vec<Message> = Vec::new();
    for partition in &tails {
        malformed += partition.malformed;
        messages.extend(partition.records.iter().map(Message::from));
    }

    let fetched = messages.len();
    // Newest first, and offset breaks the tie so two records written in the
    // same millisecond do not swap places between two loads of the page.
    messages.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then_with(|| b.offset.cmp(&a.offset))
            .then_with(|| a.partition.cmp(&b.partition))
    });
    messages.truncate(limit);

    // Before the payload leaves, not after. A read nobody could record is a
    // read that does not happen — see `kaas_ui_auth::audit`.
    state.record_read(
        &caller
            .reading(&id, &topic, Kind::Tail)
            .with_range(messages.iter().map(|row| row.offset), messages.len()),
    )?;

    let mut envelope = Envelope::new(messages).with_total(fetched);
    if malformed > 0 {
        // A batch that would not decode at the protocol level is a fact about
        // the topic, not a failed request. It is reported and the rest of the
        // tail still renders.
        envelope.errors.push(kaas_ui_core::ResourceError {
            resource: topic,
            kind: kaas_ui_core::ErrorKind::Decode,
            code: None,
            code_number: None,
            message: format!("{malformed} batch(es) could not be decoded and were skipped"),
            unsupported_api: None,
            retriable: false,
        });
    }

    Ok(Json(envelope))
}

/// One page of a window, for "load more".
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    /// The rows, in the mode's own order.
    pub items: Vec<StreamRow>,
    /// Whatever failed while reading them.
    pub errors: Vec<kaas_ui_core::ResourceError>,
    /// Whether the page filled, which is the only honest signal that there is
    /// more. A short page means the window ran out.
    pub has_more: bool,
    /// The anchor to ask for next, or `None` at the end of the window.
    ///
    /// One number for every partition, because that is what the seek modes
    /// take: partitions are at different offsets, so this is where to *start*
    /// the next page and not a claim about any particular partition.
    pub next_offset: Option<i64>,
    /// What a time-mode instant resolved to, and `None` for the other five
    /// modes. See [`resolve`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<ResolvedSeek>,
}

/// `GET /api/clusters/{id}/topics/{topic}/messages`
///
/// One bounded page, as a plain JSON resource. The same seven modes as the
/// stream and the same rows — a "load more" button must not produce rows that
/// differ in shape from the ones already on screen.
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/topics/{topic}/messages",
    params(
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
        ("mode" = Option<SeekMode>, Query, description = "Which window"),
        ("offset" = Option<i64>, Query, description = "For fromOffset and toOffset"),
        ("timestamp" = Option<i64>, Query, description = "Epoch millis, for sinceTime and toTime"),
        ("partitions" = Option<String>, Query, description = "Comma-separated partition list"),
        ("visibility" = Option<String>, Query, description = "all | committed"),
        ("filter" = Option<String>, Query, description = "Substring match on the value"),
        ("limit" = Option<usize>, Query, description = "Records in this page"),
    ),
    responses((status = 200, description = "One page of messages", body = MessagePage)),
    tag = "messages",
)]
pub async fn page(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, topic)): Path<(String, String)>,
    Query(query): Query<SeekQuery>,
) -> ApiResult<Json<MessagePage>> {
    let (handle, admin) = state.connected(&id, &caller)?;
    // Payloads are the sensitive surface, so this is where the `messages`
    // grant is spent — after the lookup, which already decided the cluster is
    // visible at all, and against the topic name because a role may grant
    // payload access to `public-*` and nothing else.
    caller.require_topic(&id, &handle.labels, &topic)?;
    let (mode, plan) = Plan::build(&topic, &query)?;

    if mode.is_live() {
        // A page of a stream that has no end is a contradiction, and answering
        // one would return an arbitrary slice of the present.
        return Err(ApiError::bad_request(
            "mode=live has no pages; open the stream instead",
        ));
    }
    let limit = query.limit.unwrap_or(seek::DEFAULT_LIMIT);

    // Before the read, so an empty window arrives with the reason it is empty
    // rather than after it.
    let resolved =
        resolve::resolve(&admin, &topic, plan.partitions(), mode.timestamp_of(&query)).await;

    let mut rows = Vec::new();
    let mut errors = Vec::new();

    match plan {
        Plan::Backward { spec } => {
            let tails = crate::call("tail", kafka_read::tail(admin.cluster(), &spec)).await?;
            let mut malformed = 0usize;
            for partition in &tails {
                malformed += partition.malformed;
                rows.extend(partition.records.iter().map(StreamRow::of));
            }
            if malformed > 0 {
                errors.push(kaas_ui_core::ResourceError {
                    resource: topic.clone(),
                    kind: kaas_ui_core::ErrorKind::Decode,
                    code: None,
                    code_number: None,
                    message: format!("{malformed} batch(es) could not be decoded"),
                    unsupported_api: None,
                    retriable: false,
                });
            }
            // Newest first, matching what a backward mode renders.
            rows.sort_by_key(|row| std::cmp::Reverse(sort_key(row)));
        }
        Plan::Forward { spec, floor } => {
            rows = collect_forward(admin.cluster(), *spec, floor, limit).await?;
            rows.sort_by_key(sort_key);
        }
    }

    rows.truncate(limit);
    let has_more = rows.len() >= limit;
    let next_offset = next_anchor(mode, &rows);

    state.record_read(
        &caller
            .reading(&id, &topic, Kind::Page)
            .with_mode(format!("{mode:?}").to_lowercase())
            // `sort_key` already knows how to get an offset out of either row
            // kind; a malformed batch is a disclosure too, and its offset is
            // the one the reader saw.
            .with_range(rows.iter().map(|row| sort_key(row).1), rows.len()),
    )?;

    Ok(Json(MessagePage {
        items: rows,
        errors,
        has_more,
        next_offset,
        resolved,
    }))
}

/// Read a bounded forward window into a `Vec`.
async fn collect_forward(
    cluster: &kafka_meta::Cluster,
    spec: ScanSpec,
    floor: Option<i64>,
    limit: usize,
) -> ApiResult<Vec<StreamRow>> {
    let scan = crate::call("scan", kafka_read::scan(cluster, spec)).await?;
    let mut stream = Box::pin(scan);
    let mut rows = Vec::new();

    while let Some(event) = stream.next().await {
        match event {
            Ok(ScanEvent::Record(record)) => {
                if floor.is_some_and(|floor| record.offset < floor) {
                    continue;
                }
                rows.push(StreamRow::of(&record));
            }
            Ok(ScanEvent::Malformed {
                partition,
                offset,
                last_offset,
                reason,
                ..
            }) => rows.push(StreamRow::malformed(partition, offset, last_offset, reason)),
            Ok(ScanEvent::Done(_)) => break,
            Ok(_) => {}
            Err(error) => return Err(ApiError::from(error)),
        }
        if rows.len() >= limit {
            break;
        }
    }
    Ok(rows)
}

/// Sort a row by when it happened, then by where, so the order is total.
fn sort_key(row: &StreamRow) -> (i64, i64, i32) {
    match row {
        StreamRow::Record(record) => (record.timestamp, record.offset, record.partition),
        // A batch that did not decode has no timestamp to sort by. `i64::MIN`
        // keeps it adjacent to the offsets it covers rather than floating to
        // whichever end of the list happened to be nearest.
        StreamRow::Malformed(row) => (i64::MIN, row.offset, row.partition),
    }
}

/// Where the next page starts, given this one.
fn next_anchor(mode: SeekMode, rows: &[StreamRow]) -> Option<i64> {
    let offsets = rows.iter().map(|row| match row {
        StreamRow::Record(record) => record.offset,
        StreamRow::Malformed(row) => row.offset,
    });
    if mode.is_backward() {
        // Walking back: the next window ends just below the oldest offset in
        // this one.
        offsets.min()?.checked_sub(1)
    } else {
        offsets.max()?.checked_add(1)
    }
}

/// `GET /api/clusters/{id}/topics/{topic}/messages/{partition}/{offset}`
///
/// The full payload of one record, fetched when a row is selected and never
/// before. A record at a given offset never changes, so the frontend caches it
/// with `staleTime: Infinity` and re-selecting a row costs no request at all.
///
/// Implemented with a one-record `scan` rather than the anchored tail the
/// obvious reading suggests: both land on the right record, but only the scan
/// carries the **raw bytes of a batch that would not decode**, and a detail
/// panel that cannot show those is useless for exactly the row that most needs
/// explaining.
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/topics/{topic}/messages/{partition}/{offset}",
    params(
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
        ("partition" = i32, Path, description = "Partition"),
        ("offset" = i64, Path, description = "Offset"),
    ),
    responses(
        (status = 200, description = "The record, or the batch that covered it", body = MessageDetail),
        (status = 404, description = "No record at that offset"),
    ),
    tag = "messages",
)]
pub async fn one(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, topic, partition, offset)): Path<(String, String, i32, i64)>,
) -> ApiResult<Json<MessageDetail>> {
    let (handle, admin) = state.connected(&id, &caller)?;
    // Payloads are the sensitive surface, so this is where the `messages`
    // grant is spent — after the lookup, which already decided the cluster is
    // visible at all, and against the topic name because a role may grant
    // payload access to `public-*` and nothing else.
    caller.require_topic(&id, &handle.labels, &topic)?;
    if offset < 0 {
        return Err(ApiError::bad_request("offset must not be negative"));
    }

    let spec = ScanSpec::new(topic.clone())
        .partitions([partition])
        .from(StartPosition::Offset(offset))
        .limit(1);
    let scan = crate::call("scan", kafka_read::scan(admin.cluster(), spec)).await?;
    let mut stream = Box::pin(scan);

    while let Some(event) = stream.next().await {
        match event {
            Ok(ScanEvent::Record(record)) => {
                // `scan` clamps a start position into the log, so an offset
                // past the end answers with the last record and one below the
                // start answers with the first. Neither is the record that was
                // asked for, and returning it would show the reader a payload
                // belonging to a different row.
                if record.offset != offset {
                    break;
                }
                state.record_read(
                    &caller
                        .reading(&id, &topic, Kind::Record)
                        .at_record(partition, offset),
                )?;
                return Ok(Json(MessageDetail::of(&record)));
            }
            Ok(ScanEvent::Malformed {
                offset: base,
                last_offset,
                raw,
                reason,
                ..
            }) => {
                let end = last_offset.unwrap_or(base).max(base);
                if offset < base || offset > end {
                    break;
                }
                state.record_read(
                    &caller
                        .reading(&id, &topic, Kind::Record)
                        .at_record(partition, offset),
                )?;
                return Ok(Json(MessageDetail::Malformed(MalformedDetail {
                    partition,
                    offset: base,
                    last_offset: end,
                    reason: reason.to_string(),
                    raw: Payload::full(&raw),
                })));
            }
            Ok(ScanEvent::Done(_)) => break,
            Ok(_) => {}
            Err(error) => return Err(ApiError::from(error)),
        }
    }

    Err(ApiError::offset_out_of_range(&topic, partition, offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaas_ui_core::dto::{MalformedRow, StreamRecord};

    fn record(partition: i32, offset: i64, timestamp: i64) -> StreamRow {
        StreamRow::Record(StreamRecord {
            partition,
            offset,
            timestamp,
            timestamp_type: "createTime".to_owned(),
            key: None,
            value: None,
            transactional: false,
        })
    }

    #[test]
    fn a_backward_page_points_at_the_offset_below_its_oldest() {
        let rows = vec![record(0, 900, 5), record(1, 880, 4), record(0, 895, 3)];
        assert_eq!(next_anchor(SeekMode::ToOffset, &rows), Some(879));
    }

    #[test]
    fn a_forward_page_points_at_the_offset_above_its_newest() {
        let rows = vec![record(0, 100, 1), record(1, 140, 2)];
        assert_eq!(next_anchor(SeekMode::FromOffset, &rows), Some(141));
    }

    #[test]
    fn an_empty_page_has_nowhere_to_go_next() {
        assert_eq!(next_anchor(SeekMode::ToOffset, &[]), None);
        assert_eq!(next_anchor(SeekMode::Oldest, &[]), None);
    }

    #[test]
    fn a_malformed_row_sorts_by_where_it_happened_not_by_when() {
        // It has no timestamp. Sorting it to the end of the list would move a
        // decode failure away from the offsets it covers, which is the one
        // piece of information it carries.
        let malformed = StreamRow::Malformed(MalformedRow {
            partition: 3,
            offset: 4102,
            last_offset: 4530,
            reason: "unsupported compression codec".to_owned(),
        });
        assert_eq!(sort_key(&malformed).0, i64::MIN);
        assert_eq!(sort_key(&malformed).1, 4102);
    }

    #[test]
    fn a_row_ids_itself_the_same_way_everywhere() {
        // `{partition}-{offset}`, in the SSE id, the React key, the selection
        // state and the query key alike.
        assert_eq!(record(3, 16_733, 0).id(), "3-16733");
        assert_eq!(
            StreamRow::malformed(3, 4102, Some(4530), "bad codec").id(),
            "3-4102"
        );
    }

    #[test]
    fn a_malformed_batch_with_no_end_still_names_a_range() {
        // A header too damaged to report its last offset leaves nothing to
        // compute a range from, and a range of zero renders as a gap.
        match StreamRow::malformed(0, 77, None, "truncated header") {
            StreamRow::Malformed(row) => {
                assert_eq!(row.offset, 77);
                assert_eq!(row.last_offset, 77);
            }
            other => panic!("expected a malformed row, got {other:?}"),
        }
    }
}
