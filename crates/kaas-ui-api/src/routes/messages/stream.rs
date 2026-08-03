//! The message stream.
//!
//! The only streaming route in the application, and the one place a handler
//! outlives its own request. Four properties are built in rather than bolted
//! on, and each of them is easier to keep than to restore:
//!
//! **Coalescing.** One SSE event per record saturates the connection and the
//! browser's parser long before the list is the bottleneck. Records accumulate
//! for [`FLUSH_INTERVAL`] and leave as one `messages` event, so ten thousand
//! records a second is ten events a second.
//!
//! **Drop-oldest, never block.** The hand-off to the SSE writer is a bounded
//! ring. Awaiting a full queue would push back through the writer into the
//! fetch loop, so one browser on a bad connection would slow the scan for
//! everyone reading that cluster. Dropped records are *counted and reported* —
//! silently losing records in a debugging tool is worse than showing a gap.
//!
//! **The pump dies with its response.** It runs on a spawned task, which is
//! what lets it drop records instead of waiting, and that task selects on the
//! reader going away. Closing a browser tab drops the scan within a poll
//! rather than leaving it fetching into a queue nobody reads. kaas-lib is
//! cancel-safe by construction; the job here is not to undo that.
//!
//! **A backward window has no partial results.** [`kafka_read::tail`] returns
//! a `Vec`, so `newest`, `toOffset` and `toTime` emit `phase: seeking`, then
//! the whole window at once. A progress bar there would be a lie, and a
//! spinner that never moves is indistinguishable from a hang.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, header};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::{Stream, StreamExt};
use kaas_ui_core::ResourceError;
use kaas_ui_core::dto::{Dropped, ResolvedSeek, StreamPhase, StreamProgress, StreamRow};
use kafka_read::{ScanEvent, ScanProgress, ScanSpec};
use serde::Serialize;

use super::seek::{Plan, SeekMode, SeekQuery};
use crate::streaming::{self, Principal, Refusal};
use kaas_ui_auth::Action;

use crate::{ApiError, ApiResult, AppState, Caller};

/// How long records accumulate before they leave as one event.
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);

/// How many flushed batches the writer may fall behind by.
///
/// Batches, not records: coalescing happens before the ring, so this is a
/// ceiling on a queue that is normally one or two entries deep.
const RING_CAPACITY: usize = 2_000;

/// How many rows one event may carry.
///
/// A flush interval's worth of a very fast topic is otherwise one enormous
/// `data:` line, which the browser parses as a single blocking JSON.
const MAX_ROWS_PER_EVENT: usize = 500;

/// How long any one stream may stay open.
///
/// Closed with `phase: done` rather than dropped, so the client can decide
/// whether to reopen instead of guessing whether the network broke.
const MAX_LIFETIME: Duration = Duration::from_secs(30 * 60);

/// How often to send a comment so proxies see traffic on an idle topic.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// One thing to write to the client.
#[derive(Debug)]
enum Frame {
    Rows(Vec<StreamRow>),
    Progress(StreamProgress),
    Phase(StreamPhase),
    Resolved(Box<ResolvedSeek>),
    Failed(Box<ResourceError>),
}

/// `GET /api/clusters/{id}/topics/{topic}/messages/stream`
///
/// The seven seek modes, over `text/event-stream`. Events are `messages`,
/// `progress`, `phase`, `dropped` and `error`; the `id:` on a `messages` event
/// is `{partition}-{offset}` of its last row.
///
/// An `error` event carries the same `ResourceError` shape the rest of the API
/// uses, so an `UnsupportedApi` arrives with **both** version ranges intact and
/// the reader can tell "this cluster does not implement it" from "this build
/// cannot speak it". That distinction is the entire reason the variant carries
/// two ranges, and flattening it into a message string discards it.
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/topics/{topic}/messages/stream",
    params(
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
        ("mode" = Option<SeekMode>, Query, description = "Which window"),
        ("offset" = Option<i64>, Query, description = "For fromOffset and toOffset"),
        ("timestamp" = Option<i64>, Query, description = "Epoch millis, for sinceTime and toTime"),
        ("partitions" = Option<String>, Query, description = "Comma-separated partition list"),
        ("visibility" = Option<String>, Query, description = "all | committed"),
        ("filter" = Option<String>, Query, description = "Substring match on the value"),
        ("limit" = Option<usize>, Query, description = "Records, ignored by live"),
    ),
    responses(
        (status = 200, description = "An event stream of messages", content_type = "text/event-stream"),
        (status = 429, description = "Too many streams are open"),
    ),
    tag = "messages",
)]
pub async fn stream(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, topic)): Path<(String, String)>,
    Query(query): Query<SeekQuery>,
    principal: Principal,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let (handle, admin) = state.connected(&id, &caller)?;
    // Payloads are the sensitive surface, so this is where the `messages`
    // grant is spent — after the lookup, which already decided the cluster is
    // visible at all, and against the topic name because a role may grant
    // payload access to `public-*` and nothing else.
    caller.require_topic(&id, &handle.labels, &topic)?;
    let (mode, plan) = Plan::build(&topic, &query)?;

    // Before the stream opens, which is when disclosure begins. There are no
    // offsets yet — the seek is what there is to record, and a stream that
    // could not be recorded is never opened.
    state.record_read(
        &caller
            .reading(&id, &topic, Action::Stream)
            .with_mode(format!("{mode:?}").to_lowercase()),
    )?;

    // Taken before anything expensive happens, and released by dropping the
    // permit — which is the only release there is, because a stream that ends
    // by the client vanishing runs no teardown of its own.
    let permit = state
        .streams()
        .acquire(&principal)
        .map_err(ApiError::too_many_streams)?;

    tracing::debug!(
        cluster = %id,
        %topic,
        ?mode,
        principal = %principal.key,
        distinguishable = principal.distinguishable,
        "opening a message stream"
    );

    // A reconnecting browser replays its last id. It names one partition, so
    // it can only resume a stream that covers exactly that one; anything wider
    // would need a cursor per partition, and quietly resuming the wrong ones
    // is worse than saying the gap exists.
    let resume = last_event_id(&headers);

    let shutdown = state.shutdown();
    let evicted = permit.evicted();
    let (tx, mut rx) = streaming::ring::<Frame>(RING_CAPACITY);
    let seek_instant = mode.timestamp_of(&query);
    let resumed_from = resume.clone();
    let topic_for_pump = topic.clone();

    tokio::spawn(async move {
        // The pump owns the scan. Selecting on the reader going away is what
        // stops this task from outliving the response it belongs to: kaas-lib
        // is cancel-safe, so dropping the future here releases the buffers and
        // the in-flight fetches without any teardown of our own.
        tokio::select! {
            biased;
            () = tx.closed() => {}
            // The process is going away. Ending the window rather than letting
            // the connection be severed is the difference between a client
            // that knows to reopen and one that cannot tell a deploy from a
            // broken network — and it is what lets the server finish draining
            // at all, since an SSE body otherwise never completes.
            () = shutdown.wait() => {
                tx.push(Frame::Phase(StreamPhase::Done));
            }
            // The same caller opened another stream while at their ceiling, so
            // this one — their oldest — makes room. Almost always a tab they
            // have already navigated away from, and behind a proxy that holds
            // the connection open it is the only signal that they have.
            () = evicted.wait() => {
                tx.push(Frame::Phase(StreamPhase::Done));
            }
            () = tokio::time::sleep(MAX_LIFETIME) => {
                tx.push(Frame::Phase(StreamPhase::Done));
            }
            () = pump(admin, topic_for_pump, plan, seek_instant, &tx, resumed_from) => {}
        }
        drop(permit);
    });

    let events = async_stream::stream! {
        // Announced before the first record so a reconnecting client can tell
        // "resumed" from "started over" without inspecting offsets.
        if let Some(from) = resume {
            yield Ok::<Event, Infallible>(Event::default().event("resumed").data(from));
        }

        let mut reported_drops = 0u64;
        while let Some(frame) = rx.recv().await {
            let dropped = rx.dropped();
            if dropped != reported_drops {
                reported_drops = dropped;
                yield Ok(encode("dropped", &Dropped { count: dropped }, None));
            }
            yield Ok(match frame {
                Frame::Rows(rows) => {
                    let id = rows.last().map(StreamRow::id);
                    encode("messages", &rows, id)
                }
                Frame::Progress(progress) => encode("progress", &progress, None),
                Frame::Phase(phase) => encode("phase", &PhaseEvent { phase }, None),
                Frame::Resolved(resolved) => encode("resolved", &resolved, None),
                Frame::Failed(error) => encode("error", &error, None),
            });
        }
    };

    let stream = Sse::new(events).keep_alive(
        // SSE comments, which browsers ignore and proxies count as traffic.
        // A hand-rolled heartbeat event would reach the client's parser and
        // have to be filtered out there instead.
        KeepAlive::new().interval(KEEP_ALIVE),
    );

    // Two headers aimed squarely at whatever is in front of this process.
    //
    // A proxy that buffers `text/event-stream` turns a live view into a
    // working-looking one that delivers nothing until some buffer fills — the
    // worst failure mode available, because every layer reports success. This
    // deployment has two proxies in the path, a Cloudflare tunnel into
    // code-server, and neither is ours to configure.
    //
    // `X-Accel-Buffering: no` is nginx's opt-out and is honoured by several
    // others. `no-transform` tells any cache in the chain not to recompress
    // the body, which is the other way a stream ends up buffered — kaas-ui's
    // own compression layer already declines SSE, but an edge that adds its
    // own is outside its reach.
    Ok((
        [
            (header::CACHE_CONTROL, "no-cache, no-transform"),
            (header::HeaderName::from_static("x-accel-buffering"), "no"),
        ],
        stream,
    ))
}

/// The `phase` event's body, so it is an object rather than a bare string.
#[derive(Debug, Serialize)]
struct PhaseEvent {
    phase: StreamPhase,
}

/// Serialise a frame, falling back to an error event rather than a panic.
fn encode<T: Serialize>(name: &str, body: &T, id: Option<String>) -> Event {
    let event = Event::default().event(name);
    let event = match id {
        Some(id) => event.id(id),
        None => event,
    };
    match serde_json::to_string(body) {
        Ok(json) => event.data(json),
        // Unreachable for these shapes, and still not worth a panic in the one
        // crate that is fed by whatever a producer felt like writing.
        Err(error) => Event::default().event("error").data(format!(
            r#"{{"message":"could not encode {name}: {error}"}}"#
        )),
    }
}

/// The client's `Last-Event-ID`, if it sent one.
fn last_event_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

/// Read the window and push frames until it ends or the reader leaves.
async fn pump(
    admin: std::sync::Arc<kafka_admin::Admin>,
    topic: String,
    plan: Plan,
    seek_instant: Option<i64>,
    tx: &streaming::Sender<Frame>,
    resume: Option<String>,
) {
    tx.push(Frame::Phase(StreamPhase::Seeking));

    // Before the read rather than after it, so a window that comes back empty
    // arrives with the reason already on screen. A broker that holds no
    // timestamp index answers a time seek with nothing, which is a valid
    // response and an unhelpful one — see `resolve`.
    if let Some(resolved) =
        super::resolve::resolve(&admin, &topic, plan.partitions(), seek_instant).await
    {
        tx.push(Frame::Resolved(Box::new(resolved)));
    }

    let cluster = admin.cluster().clone();

    match plan {
        // A backward walk has nothing to stream: it buffers its whole window
        // before returning. Everything arrives at once, or an error does.
        Plan::Backward { spec } => {
            match kafka_read::tail(&cluster, &spec).await {
                Ok(tails) => {
                    let mut rows: Vec<StreamRow> = Vec::new();
                    let mut malformed = 0usize;
                    for partition in &tails {
                        malformed += partition.malformed;
                        rows.extend(partition.records.iter().map(StreamRow::of));
                    }
                    // Newest first, which is what every backward mode renders.
                    rows.sort_by_key(|row| std::cmp::Reverse(super::sort_key(row)));
                    rows.truncate(spec.limit);

                    tx.push(Frame::Phase(StreamPhase::Streaming));
                    if malformed > 0 {
                        tx.push(Frame::Failed(Box::new(decode_note(&spec.topic, malformed))));
                    }
                    for chunk in rows.chunks(MAX_ROWS_PER_EVENT) {
                        tx.push(Frame::Rows(chunk.to_vec()));
                    }
                }
                Err(error) => {
                    tx.push(Frame::Failed(Box::new(ResourceError::new(
                        &spec.topic,
                        &error,
                    ))));
                }
            }
            tx.push(Frame::Phase(StreamPhase::Done));
        }

        Plan::Forward { spec, floor } => {
            let topic = spec.topic.clone();
            let reorder = reorder_window(&spec);
            let floor = resume_floor(floor, resume.as_deref(), spec.partitions.as_deref());

            let scan = match kafka_read::scan(&cluster, *spec).await {
                Ok(scan) => scan,
                Err(error) => {
                    tx.push(Frame::Failed(Box::new(ResourceError::new(&topic, &error))));
                    tx.push(Frame::Phase(StreamPhase::Done));
                    return;
                }
            };

            tx.push(Frame::Phase(StreamPhase::Streaming));
            forward(scan, tx, floor, reorder).await;
            tx.push(Frame::Phase(StreamPhase::Done));
        }
    }
}

/// Drive a forward scan, coalescing on the flush interval.
async fn forward(
    scan: impl Stream<Item = Result<ScanEvent, kafka_conn::Error>> + Send,
    tx: &streaming::Sender<Frame>,
    floor: Option<i64>,
    reorder: usize,
) {
    let mut scan = Box::pin(scan);
    let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut batch: Vec<StreamRow> = Vec::new();

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                flush(&mut batch, tx);
                if tx.is_closed() {
                    return;
                }
            }
            event = scan.next() => match event {
                Some(Ok(ScanEvent::Record(record))) => {
                    // `scan` clamps an out-of-range start into the log, so a
                    // request to read from an offset the topic no longer
                    // retains would otherwise answer from its earliest and
                    // look like it worked.
                    if floor.is_some_and(|floor| record.offset < floor) {
                        continue;
                    }
                    batch.push(StreamRow::of(&record));
                    if batch.len() >= MAX_ROWS_PER_EVENT {
                        flush(&mut batch, tx);
                    }
                }
                Some(Ok(ScanEvent::Malformed { partition, offset, last_offset, reason, .. })) => {
                    // A row, not an error. Surfacing these is the whole point
                    // of the tolerant decoder, and the scan continues past it.
                    batch.push(StreamRow::malformed(partition, offset, last_offset, reason));
                }
                Some(Ok(ScanEvent::Progress(progress))) => {
                    flush(&mut batch, tx);
                    tx.push(Frame::Progress(render_progress(&progress, reorder)));
                }
                Some(Ok(ScanEvent::Done(progress))) => {
                    flush(&mut batch, tx);
                    tx.push(Frame::Progress(render_progress(&progress, reorder)));
                    return;
                }
                Some(Ok(ScanEvent::PartitionComplete { .. })) => {}
                Some(Err(error)) => {
                    flush(&mut batch, tx);
                    tx.push(Frame::Failed(Box::new(ResourceError::new("scan", &error))));
                    return;
                }
                None => {
                    flush(&mut batch, tx);
                    return;
                }
            },
        }
    }
}

fn flush(batch: &mut Vec<StreamRow>, tx: &streaming::Sender<Frame>) {
    if batch.is_empty() {
        return;
    }
    tx.push(Frame::Rows(std::mem::take(batch)));
}

/// Where a resumed stream picks up, if it can pick up at all.
///
/// `Last-Event-ID` is one `{partition}-{offset}`, and a scan over several
/// partitions needs one cursor each. So a resume is honoured only for a stream
/// restricted to the partition the id names; any wider and the id is ignored,
/// the stream starts where its mode says, and the client sees the gap rather
/// than a silently wrong window.
fn resume_floor(
    floor: Option<i64>,
    resume: Option<&str>,
    partitions: Option<&[i32]>,
) -> Option<i64> {
    let Some((partition, offset)) = resume.and_then(parse_event_id) else {
        return floor;
    };
    if partitions != Some(&[partition]) {
        return floor;
    }
    // The client already has that offset, so the next one is the first it
    // still needs.
    let next = offset.checked_add(1)?;
    Some(floor.map_or(next, |floor| floor.max(next)))
}

fn parse_event_id(id: &str) -> Option<(i32, i64)> {
    let (partition, offset) = id.split_once('-')?;
    Some((partition.parse().ok()?, offset.parse().ok()?))
}

/// Roughly how far apart two partitions may be reordered.
///
/// The merge emits the oldest record it can see, so what bounds the reorder is
/// how much it can see at once: the buffer ceiling spread over the partitions
/// reading from it. A caveat to render beside the list, not a promise.
fn reorder_window(spec: &ScanSpec) -> usize {
    let partitions = spec.partitions.as_ref().map_or(1, |list| list.len().max(1));
    spec.max_buffered_records / partitions.max(1)
}

fn render_progress(progress: &ScanProgress, reorder: usize) -> StreamProgress {
    StreamProgress {
        fraction: progress.fraction(),
        records_emitted: progress.records_emitted,
        records_scanned: progress.records_scanned,
        malformed_batches: progress.malformed_batches,
        partitions_active: progress.partitions_active,
        ordering_degraded: progress.ordering_degraded,
        reorder_window: if progress.partitions_active > 1 {
            reorder
        } else {
            // One partition is exact log order. Reporting a window there would
            // caveat a guarantee that actually holds.
            0
        },
    }
}

fn decode_note(topic: &str, malformed: usize) -> ResourceError {
    ResourceError {
        resource: topic.to_owned(),
        kind: kaas_ui_core::ErrorKind::Decode,
        code: None,
        code_number: None,
        message: format!("{malformed} batch(es) in this window could not be decoded"),
        unsupported_api: None,
        retriable: false,
    }
}

impl ApiError {
    /// `429`, with which ceiling was hit and what to do about it.
    fn too_many_streams(refusal: Refusal) -> Self {
        Self::too_many_requests(refusal.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kafka_read::StartPosition;

    #[test]
    fn an_event_id_is_partition_dash_offset() {
        assert_eq!(parse_event_id("3-16733"), Some((3, 16_733)));
        assert_eq!(parse_event_id("0-0"), Some((0, 0)));
        assert_eq!(parse_event_id("nonsense"), None);
        assert_eq!(parse_event_id("3-"), None);
    }

    #[test]
    fn a_resume_is_honoured_only_for_the_partition_it_names() {
        // One id cannot restore a cursor per partition, and resuming the
        // partition it does name while restarting the others would drop
        // records without saying so.
        assert_eq!(
            resume_floor(None, Some("3-16733"), Some(&[3])),
            Some(16_734),
            "the client has 16733 already"
        );
        assert_eq!(
            resume_floor(None, Some("3-16733"), Some(&[3, 4])),
            None,
            "a wider stream must not pretend one id resumed it"
        );
        assert_eq!(
            resume_floor(None, Some("3-16733"), None),
            None,
            "nor must a stream over every partition"
        );
    }

    #[test]
    fn a_resume_never_moves_a_floor_backwards() {
        // Otherwise a stale id from before a seek would re-deliver records the
        // mode deliberately excluded.
        assert_eq!(
            resume_floor(Some(90_000), Some("0-100"), Some(&[0])),
            Some(90_000)
        );
    }

    #[test]
    fn the_reorder_window_is_the_buffer_spread_over_the_partitions() {
        let spec = ScanSpec::new("orders").with_partitions([0, 1, 2, 3]);
        assert_eq!(reorder_window(&spec), spec.max_buffered_records / 4);
    }

    #[test]
    fn a_single_partition_stream_reports_no_reorder_window() {
        // Within a partition the order is exact, always. A caveat there would
        // undersell a guarantee that holds.
        let progress = ScanProgress {
            records_emitted: 10,
            records_scanned: 10,
            malformed_batches: 0,
            offsets_consumed: 10,
            offsets_total: 100,
            partitions_active: 1,
            ordering_degraded: false,
        };
        assert_eq!(render_progress(&progress, 512).reorder_window, 0);
    }

    #[test]
    fn a_live_stream_has_no_fraction_to_report() {
        // `offsets_total` is unknown for a tail, and a progress bar that
        // invents one would fill up and stay full forever.
        let progress = ScanProgress {
            records_emitted: 10,
            records_scanned: 10,
            malformed_batches: 0,
            offsets_consumed: 10,
            offsets_total: 0,
            partitions_active: 3,
            ordering_degraded: false,
        };
        assert_eq!(render_progress(&progress, 128).fraction, None);
        assert_eq!(render_progress(&progress, 128).reorder_window, 128);
    }

    #[test]
    fn a_live_plan_reaches_the_pump_as_a_following_scan() {
        // The end-to-end version of the unit test in `seek`: what the handler
        // hands the library is what makes the view live.
        let (mode, plan) = Plan::build(
            "orders",
            &SeekQuery {
                mode: Some(SeekMode::Live),
                ..SeekQuery::default()
            },
        )
        .unwrap();
        assert!(mode.is_live());
        match plan {
            Plan::Forward { spec, .. } => {
                assert!(spec.follow);
                assert_eq!(spec.from, StartPosition::Latest);
            }
            other => panic!("expected a forward plan, got {other:?}"),
        }
    }
}
