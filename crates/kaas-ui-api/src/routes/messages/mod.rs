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
use kaas_ui_core::decode::{CodecOverride, PayloadDecoder};
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
    /// How to read keys, overriding the per-topic configuration.
    pub key_codec: Option<kaas_ui_serde::Codec>,
    /// How to read values, overriding the per-topic configuration.
    pub value_codec: Option<kaas_ui_serde::Codec>,
}

impl TailQuery {
    /// The codec override, as this request set it.
    fn codecs(&self) -> CodecOverride {
        CodecOverride {
            key: self.key_codec,
            value: self.value_codec,
        }
    }
}

/// `GET /api/clusters/{id}/topics/{topic}/messages/tail`
///
/// `TailSpec::limit` is a per-topic target, and kaas-lib keeps each
/// partition's last chunk whole rather than splitting it, so asking for 20
/// fetches somewhat more than 20. "The last n of a topic" has no single answer
/// across partitions, so this layer picks one and says so: **the n most recent
/// by timestamp**, merged across partitions and truncated after the fact.
/// `total` reports how many were fetched before truncation.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/topics/{topic}/messages/tail",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
        ("limit" = Option<usize>, Query, description = "Records to return after merging"),
        ("partitions" = Option<String>, Query, description = "Comma-separated partition list"),
        ("keyCodec" = Option<kaas_ui_serde::Codec>, Query, description = "Override how keys are read"),
        ("valueCodec" = Option<kaas_ui_serde::Codec>, Query, description = "Override how values are read"),
    ),
    responses((status = 200, description = "The tail of a topic", body = Envelope<Message>)),
    tag = "messages",
)]
pub async fn tail(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, topic)): Path<(String, String, String)>,
    Query(query): Query<TailQuery>,
) -> ApiResult<Json<Envelope<Message>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
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

    // Built once per request, not once per record: the registry client it
    // holds is the shared one, and a decoder per record would be a decoder
    // with no cache.
    let decoder = PayloadDecoder::new(&handle, &topic, query.codecs());

    let mut malformed = 0usize;
    let mut messages: Vec<Message> = Vec::new();
    for partition in &tails {
        malformed += partition.malformed;
        for record in &partition.records {
            messages.push(Message::of(record, &decoder).await);
        }
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
        envelope = envelope.with_errors([kaas_ui_core::ResourceError {
            resource: topic,
            kind: kaas_ui_core::ErrorKind::Decode,
            code: None,
            code_number: None,
            message: format!("{malformed} batch(es) could not be decoded and were skipped"),
            unsupported_api: None,
            retriable: false,
        }]);
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
    /// Whether the **read** filled its budget, which is the only honest signal
    /// that there is more.
    ///
    /// Not "the page is full": the payload filter runs after the decode, so a
    /// page of three rows may have read five hundred records and have five
    /// hundred thousand still to walk. Deriving this from the row count would
    /// end paging at the first window a selective filter emptied.
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
    path = "/api/environments/{env}/clusters/{id}/topics/{topic}/messages",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
        ("mode" = Option<SeekMode>, Query, description = "Which window"),
        ("offset" = Option<i64>, Query, description = "For fromOffset and toOffset"),
        ("timestamp" = Option<i64>, Query, description = "Epoch millis, for sinceTime and toTime"),
        ("partitions" = Option<String>, Query, description = "Comma-separated partition list"),
        ("visibility" = Option<String>, Query, description = "all | committed"),
        ("filter" = Option<String>, Query, description = "Literal substring of the decoded value"),
        ("limit" = Option<usize>, Query, description = "Records to read for this page"),
        ("keyCodec" = Option<kaas_ui_serde::Codec>, Query, description = "Override how keys are read"),
        ("valueCodec" = Option<kaas_ui_serde::Codec>, Query, description = "Override how values are read"),
    ),
    responses((status = 200, description = "One page of messages", body = MessagePage)),
    tag = "messages",
)]
pub async fn page(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, topic)): Path<(String, String, String)>,
    Query(query): Query<SeekQuery>,
) -> ApiResult<Json<MessagePage>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
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

    // Taken before the read, so a needle nobody can serve costs no round trip.
    let decoder =
        PayloadDecoder::new(&handle, &topic, query.codecs()).with_filter(query.payload_filter()?);

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    // What this read *looked at*, filter or no filter. The rows are what
    // survived; only this can say where the next page starts.
    let mut window = Window::default();

    // `None` until a backward read learns the honest answer; the budget
    // heuristic below is the fallback.
    let mut backward_more: Option<bool> = None;

    match plan {
        Plan::Backward { spec } => {
            let tails = crate::call("tail", kafka_read::tail(admin.cluster(), &spec)).await?;
            backward_more = Some(more_below(&tails));
            let mut malformed = 0usize;
            for partition in &tails {
                malformed += partition.malformed;
                for record in &partition.records {
                    window.saw(record.offset);
                    if let Some(decoded) =
                        decoder.accept(record, kaas_ui_serde::PREVIEW_CHARS).await
                    {
                        rows.push(StreamRow::render(record, decoded));
                    }
                }
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
            (rows, window) =
                collect_forward(admin.cluster(), *spec, floor, limit, &decoder).await?;
            rows.sort_by_key(sort_key);
        }
    }

    let collected = rows.len();
    rows.truncate(limit);
    // A backward read over-fetches — a partition's last chunk is kept whole
    // rather than split — so rows past the limit are cut here and were never
    // shown. Where that happened, the last row shown is the boundary;
    // anywhere else it is the far end of what was read.
    let cut = collected > rows.len();
    // A backward read knows the honest answer — whether any walk stopped
    // short of the start of its partition's retention. "Did the read fill its
    // budget" is the fallback, and the only signal a forward read has; a page
    // cut to its limit discarded records past the cut, so those are more
    // whatever the walks said.
    let has_more = match backward_more {
        Some(more) => cut || more,
        None => window.examined >= limit,
    };
    let next_offset = next_anchor(mode, &rows, &window, cut);

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

/// Whether a backward read left records below the window it returned.
///
/// A walk that reached the start of what its partition retains has nothing
/// below it; one that stopped anywhere else does. kaas-lib measures those
/// bounds to plan the walk and reports the answer with the records, so this
/// is a statement about the topic rather than about the budget — and it costs
/// no `ListOffsets` of kaas-ui's own.
///
/// kaas-ui asked the question itself until kaas-lib 0.9: `tail` divided its
/// limit across every partition before knowing which held anything, so a
/// window of 500 over a topic with two idle partitions of three read ⌈500/3⌉
/// = 167, and 167 < 500 read as "exhausted" with most of the topic still
/// below it. That is kaas-rs/kaas-lib#17, and it landed.
fn more_below(tails: &[kafka_read::PartitionTail]) -> bool {
    tails.iter().any(|tail| !tail.reached_log_start)
}

/// The extent of what one read looked at, before anything was filtered out.
///
/// The page's rows cannot answer "where does the next page start" on their
/// own: with a payload filter every one of them may have been dropped, and an
/// anchor derived from an empty list is no anchor at all — paging would stop
/// at the first window that matched nothing.
#[derive(Debug, Default)]
struct Window {
    /// Records and malformed batches read, matched or not.
    examined: usize,
    lowest: Option<i64>,
    highest: Option<i64>,
}

impl Window {
    fn saw(&mut self, offset: i64) {
        self.examined += 1;
        self.lowest = Some(self.lowest.map_or(offset, |low| low.min(offset)));
        self.highest = Some(self.highest.map_or(offset, |high| high.max(offset)));
    }

    /// A batch that did not decode covers a range, and the reader has seen all
    /// of it — the row says so — so the next page starts past its end.
    fn saw_range(&mut self, offset: i64, last_offset: Option<i64>) {
        self.saw(offset);
        if let Some(last) = last_offset {
            self.highest = Some(self.highest.map_or(last, |high| high.max(last)));
        }
    }
}

/// Read a bounded forward window into a `Vec`, with what it walked.
async fn collect_forward(
    cluster: &kafka_meta::Cluster,
    spec: ScanSpec,
    floor: Option<i64>,
    limit: usize,
    decoder: &PayloadDecoder,
) -> ApiResult<(Vec<StreamRow>, Window)> {
    let scan = crate::call("scan", kafka_read::scan(cluster, spec)).await?;
    let mut stream = Box::pin(scan);
    let mut rows = Vec::new();
    let mut window = Window::default();

    while let Some(event) = stream.next().await {
        match event {
            Ok(ScanEvent::Record(record)) => {
                if floor.is_some_and(|floor| record.offset < floor) {
                    continue;
                }
                // Counted before the decode: this is the budget the scan spec
                // is bounded by, and it is what `hasMore` is read from.
                window.saw(record.offset);
                if let Some(decoded) = decoder.accept(&record, kaas_ui_serde::PREVIEW_CHARS).await {
                    rows.push(StreamRow::render(&record, decoded));
                }
            }
            Ok(ScanEvent::Malformed {
                partition,
                offset,
                last_offset,
                reason,
                ..
            }) => {
                window.saw_range(offset, last_offset);
                rows.push(StreamRow::malformed(partition, offset, last_offset, reason));
            }
            Ok(ScanEvent::Done(_)) => break,
            Ok(_) => {}
            Err(error) => return Err(ApiError::from(error)),
        }
        // Both ceilings, because they are no longer the same number: a page
        // is full at `limit` rows, and a read is spent at `limit` records
        // whether or not any of them matched.
        if rows.len() >= limit || window.examined >= limit {
            break;
        }
    }
    Ok((rows, window))
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
///
/// Off the **window** rather than off the rows, so a filter that rejected
/// every record still moves the cursor: the reader has seen the fate of
/// everything that was read, not just of what survived. The exception is a
/// page that was cut to its limit — records past the cut were read and
/// discarded unseen, so the last row shown is the boundary and stepping past
/// it would skip them.
fn next_anchor(mode: SeekMode, rows: &[StreamRow], window: &Window, cut: bool) -> Option<i64> {
    let (low, high) = if cut {
        let offsets = rows.iter().map(|row| match row {
            StreamRow::Record(record) => record.offset,
            StreamRow::Malformed(row) => row.offset,
        });
        (offsets.clone().min(), offsets.max())
    } else {
        (window.lowest, window.highest)
    };
    if mode.is_backward() {
        // Walking back: the next window ends just below the oldest offset in
        // this one.
        low?.checked_sub(1)
    } else {
        high?.checked_add(1)
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
    path = "/api/environments/{env}/clusters/{id}/topics/{topic}/messages/{partition}/{offset}",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
        ("partition" = i32, Path, description = "Partition"),
        ("offset" = i64, Path, description = "Offset"),
        ("keyCodec" = Option<kaas_ui_serde::Codec>, Query, description = "Override how keys are read"),
        ("valueCodec" = Option<kaas_ui_serde::Codec>, Query, description = "Override how values are read"),
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
    Path((env, id, topic, partition, offset)): Path<(String, String, String, i32, i64)>,
    Query(query): Query<TailQuery>,
) -> ApiResult<Json<MessageDetail>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
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
                        .with_record(partition, offset),
                )?;
                let decoder = PayloadDecoder::new(&handle, &topic, query.codecs());
                return Ok(Json(MessageDetail::of(&record, &decoder).await));
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
                        .with_record(partition, offset),
                )?;
                return Ok(Json(MessageDetail::Malformed(Box::new(MalformedDetail {
                    partition,
                    offset: base,
                    last_offset: end,
                    reason: reason.to_string(),
                    // The one place the raw bytes of a batch are shown, and
                    // they are shown as hex whatever they look like: a batch
                    // that did not decode has no codec to have been read with.
                    raw: Payload::hex(&raw, kaas_ui_serde::DETAIL_PAYLOAD_CHARS),
                }))));
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
        StreamRow::Record(Box::new(StreamRecord {
            partition,
            offset,
            timestamp,
            timestamp_type: "createTime".to_owned(),
            key: None,
            value: None,
            transactional: false,
        }))
    }

    /// The window a page of these rows would have walked, unfiltered.
    fn walked(rows: &[StreamRow]) -> Window {
        let mut window = Window::default();
        for row in rows {
            window.saw(match row {
                StreamRow::Record(record) => record.offset,
                StreamRow::Malformed(row) => row.offset,
            });
        }
        window
    }

    #[test]
    fn a_backward_page_points_at_the_offset_below_its_oldest() {
        let rows = vec![record(0, 900, 5), record(1, 880, 4), record(0, 895, 3)];
        let window = walked(&rows);
        assert_eq!(
            next_anchor(SeekMode::ToOffset, &rows, &window, false),
            Some(879)
        );
    }

    #[test]
    fn a_forward_page_points_at_the_offset_above_its_newest() {
        let rows = vec![record(0, 100, 1), record(1, 140, 2)];
        let window = walked(&rows);
        assert_eq!(
            next_anchor(SeekMode::FromOffset, &rows, &window, false),
            Some(141)
        );
    }

    #[test]
    fn an_empty_page_has_nowhere_to_go_next() {
        let nothing = Window::default();
        assert_eq!(next_anchor(SeekMode::ToOffset, &[], &nothing, false), None);
        assert_eq!(next_anchor(SeekMode::Oldest, &[], &nothing, false), None);
    }

    /// The regression the window exists for.
    ///
    /// The payload filter runs after the decode, so a window can be read in
    /// full and produce no rows at all. Anchoring on the rows would return
    /// `None` there — "nothing further in this direction" — and paging would
    /// stop on the first window a selective filter emptied, with the topic
    /// barely touched.
    #[test]
    fn a_page_whose_filter_matched_nothing_still_says_where_to_look_next() {
        let mut window = Window::default();
        for offset in 4_000..4_500 {
            window.saw(offset);
        }
        assert_eq!(
            next_anchor(SeekMode::FromOffset, &[], &window, false),
            Some(4_500)
        );
        assert_eq!(
            next_anchor(SeekMode::ToOffset, &[], &window, false),
            Some(3_999)
        );
    }

    /// The other half of the same rule, and it points the other way.
    ///
    /// A backward read over-fetches — a partition's last chunk is kept whole
    /// rather than split — and the rows past the limit are cut before anyone
    /// sees them. The next window has to start just below the oldest
    /// row *shown*, not below the oldest record read, or the cut records are
    /// skipped and never appear in any page.
    #[test]
    fn a_page_cut_to_its_limit_resumes_from_the_last_row_shown() {
        let shown = vec![record(0, 900, 5), record(1, 895, 4)];
        let mut window = walked(&shown);
        for offset in 880..895 {
            window.saw(offset);
        }
        assert_eq!(
            next_anchor(SeekMode::Newest, &shown, &window, true),
            Some(894)
        );
    }

    /// The bug this pins: `kaas-canary-v1` holds 89,478 records in one of its
    /// three partitions and nothing in the other two, so a backward window of
    /// 500 read ⌈500/3⌉ = 167 from the one that could answer — and 167 < 500
    /// meant `has_more` was false forever, with most of the topic below the
    /// window. The budget can no longer answer the question; the walks do.
    #[test]
    fn a_walk_stopped_short_of_the_log_start_means_there_is_more() {
        // Two idle partitions, and the busy one still 78,000 records from the
        // bottom of its retention.
        assert!(more_below(&[
            tail_of(0, true),
            tail_of(1, true),
            tail_of(2, false),
        ]));
    }

    #[test]
    fn a_topic_read_down_to_its_log_start_is_exhausted() {
        assert!(!more_below(&[tail_of(0, true), tail_of(1, true)]));
        // No partitions at all — an unknown topic is `tail`'s failure to name,
        // not a page that claims a next one.
        assert!(!more_below(&[]));
    }

    fn tail_of(partition: i32, reached_log_start: bool) -> kafka_read::PartitionTail {
        kafka_read::PartitionTail {
            partition,
            records: Vec::new(),
            malformed: 0,
            fetches: 1,
            log_start: 0,
            log_end: 0,
            reached_log_start,
        }
    }

    #[test]
    fn a_malformed_batch_is_read_to_its_end_before_the_next_page_begins() {
        // The row names a range and the reader has seen all of it. Resuming at
        // the base offset would re-read the same batch forever.
        let mut window = Window::default();
        window.saw_range(4_102, Some(4_530));
        assert_eq!(
            next_anchor(SeekMode::FromOffset, &[], &window, false),
            Some(4_531)
        );
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
