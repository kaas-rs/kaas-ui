//! The statistics tab's route: an on-demand full-topic analysis, over SSE.
//!
//! The second consumer of the streaming infrastructure Phase 3 built, and a
//! simpler one than the message stream: an analysis emits a progress frame
//! every second or two and one terminal `result`, never coalesced record
//! batches — no flush interval, no rows-per-event cap, no reorder window, no
//! `Last-Event-ID` (an id per record cannot resume an aggregate).
//!
//! **Cancellation is closing the stream.** The pump selects on the reader
//! going away, exactly as the message stream does, so navigating off the tab
//! drops the scan within a poll. kaas-lib is cancel-safe by construction; no
//! `DELETE` verb exists and none is needed — which also keeps the
//! `no_mutating_route` invariant intact.
//!
//! **A partial result is flagged, never dressed as complete.** Three things
//! end a scan early — its lifetime ceiling, a mid-scan error, and process
//! shutdown — and each emits a `result` with `complete: false` and the
//! scanned fraction, because statistics that look complete and are wrong are
//! worse than an error. Until kaas-lib can survive one partition's failure
//! (upstream ask 12), an error costs the *rest* of the scan; the fold up to
//! that point is still real and still labelled.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::StreamExt;
use kaas_ui_core::ResourceError;
use kaas_ui_core::analysis::{AnalysisProgress, AnalysisStop, TopicAnalysisBuilder};
use kaas_ui_core::dto::StreamPhase;
use kafka_read::{ScanEvent, ScanSpec, StartPosition};
use serde::Deserialize;

use crate::streaming::{self, Principal};
use kaas_ui_auth::Kind;

use crate::{ApiError, ApiResult, AppState, Caller};

/// The longest a caller may ask an analysis to run, in minutes.
///
/// The message stream's lifetime exists to shed abandoned tabs; this one
/// exists to bound how long a scan may sit on the shared broker connection.
/// The caller picks their own ceiling with `?maxMinutes=` up to this one;
/// hitting either emits a result stopped by `timeCap` — see the module doc.
const MAX_MINUTES: u64 = 30;

/// The time cap when the caller does not name one.
const DEFAULT_MINUTES: u64 = 10;

/// The least time between two progress frames.
///
/// kaas-lib emits progress every thousand records, which on a firehose is
/// several frames a millisecond and on megabyte records a long silence
/// (upstream ask 14 is the honest fix). One frame a second is the cadence a
/// progress bar wants, whatever the record size does.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);

/// Room for frames the reader has not drained. An analysis emits a frame a
/// second, so a queue this deep only fills if the reader is gone — and the
/// pump notices that through `closed()`, not through the queue.
const RING_CAPACITY: usize = 64;

/// How often to send a comment so proxies see traffic mid-scan.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// What the analysis stream can say.
#[derive(Debug)]
enum Frame {
    Progress(Box<AnalysisProgress>),
    Result(Box<kaas_ui_core::analysis::TopicAnalysis>),
    Failed(Box<ResourceError>),
    Phase(StreamPhase),
}

/// The analysis query.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisQuery {
    /// `all` or `committed`. Changes the numbers on a transactional topic,
    /// so it is the caller's choice, defaulting to everything the log holds.
    pub visibility: Option<String>,
    /// Stop after this many records, and report the result as that sample.
    ///
    /// Absent is the whole retained topic. The UI defaults the field to the
    /// topic's current retained count — the same read, but a bar that can
    /// honestly reach its end on a topic being produced to.
    pub limit: Option<u64>,
    /// Stop after this many minutes, up to [`MAX_MINUTES`]. Defaults to
    /// [`DEFAULT_MINUTES`].
    pub max_minutes: Option<u64>,
}

/// `GET /api/environments/{env}/clusters/{id}/topics/{topic}/analysis`
///
/// Events are `progress` (throttled to one a second), `result` (terminal,
/// once), `error` (the same `ResourceError` shape as everywhere else) and
/// `phase` (`seeking` → `streaming` → `done`). Closing the response cancels
/// the scan; there is no other cancellation and none is needed.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/topics/{topic}/analysis",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
        ("visibility" = Option<String>, Query, description = "all | committed"),
        ("limit" = Option<u64>, Query, description = "Stop after this many records; absent is the whole topic"),
        ("maxMinutes" = Option<u64>, Query, description = "Stop after this many minutes, 1-30; defaults to 10"),
    ),
    responses(
        (status = 200, description = "An event stream: progress frames, then one result", content_type = "text/event-stream"),
        (status = 429, description = "An analysis is already running on this cluster, or too many streams are open"),
    ),
    tag = "analysis",
)]
pub async fn analysis(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, topic)): Path<(String, String, String)>,
    Query(query): Query<AnalysisQuery>,
    principal: Principal,
) -> ApiResult<impl IntoResponse> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    // An analysis reads every payload on the topic, so it spends the same
    // grant the message routes spend — the aggregate that leaves discloses
    // no payload, but the read happens.
    caller.require_topic(&id, &handle.labels, &topic)?;

    let visibility = match query.visibility.as_deref() {
        None | Some("all") => kafka_read::Visibility::All,
        Some("committed") => kafka_read::Visibility::CommittedOnly,
        Some(other) => {
            return Err(ApiError::bad_request(format!(
                "visibility must be \"all\" or \"committed\", not {other:?}"
            )));
        }
    };

    // Refused rather than clamped, both of them: silently scanning 30 minutes
    // for a request that asked for 90 makes the caller believe the cap they
    // set is the cap they got.
    let minutes = query.max_minutes.unwrap_or(DEFAULT_MINUTES);
    if minutes == 0 || minutes > MAX_MINUTES {
        return Err(ApiError::bad_request(format!(
            "maxMinutes must be between 1 and {MAX_MINUTES}"
        )));
    }
    let limit = match query.limit {
        Some(0) => {
            return Err(ApiError::bad_request(
                "an analysis of 0 records is a mistake, not a query",
            ));
        }
        other => other,
    };

    // Before the stream opens, which is when disclosure begins — the same
    // rule as the message stream.
    state.record_read(&caller.reading(&id, &topic, Kind::Analysis))?;

    // Two ceilings, both taken before anything expensive: the stream budget
    // everyone shares, and the one-per-cluster slot that keeps a second
    // full-topic read from queueing behind the first on the shared broker
    // connection.
    let stream_permit = state
        .streams()
        .acquire(&principal)
        .map_err(|refusal| ApiError::too_many_requests(refusal.to_string()))?;
    let analysis_permit = state.begin_analysis(&env, &id)?;

    tracing::info!(cluster = %id, %topic, principal = %principal.key, "starting a topic analysis");

    let shutdown = state.shutdown();
    let evicted = stream_permit.evicted();
    let (tx, mut rx) = streaming::ring::<Frame>(RING_CAPACITY);

    let mut spec = ScanSpec::new(topic.clone())
        .from(StartPosition::Earliest)
        .visibility(visibility);
    if let Some(cap) = limit {
        // The library ends the scan at the cap; the pump only has to say
        // that this is what happened.
        spec = spec.limit(usize::try_from(cap).unwrap_or(usize::MAX));
    }
    let deadline = Duration::from_secs(minutes.saturating_mul(60));

    tokio::spawn(async move {
        // The pump owns the scan; the permits die with the pump. The time cap
        // lives *inside* the pump — unlike the message stream's lifetime —
        // because expiring must yield a result over what was read, which only
        // the fold's owner can produce.
        tokio::select! {
            biased;
            () = tx.closed() => {}
            () = shutdown.wait() => {
                tx.push(Frame::Phase(StreamPhase::Done));
            }
            () = evicted.wait() => {
                tx.push(Frame::Phase(StreamPhase::Done));
            }
            () = pump(admin, spec, limit, deadline, &tx) => {}
        }
        drop(analysis_permit);
        drop(stream_permit);
    });

    let events = async_stream::stream! {
        while let Some(frame) = rx.recv().await {
            yield Ok::<Event, Infallible>(match frame {
                Frame::Progress(progress) => encode("progress", &progress),
                Frame::Result(result) => encode("result", &result),
                Frame::Failed(error) => encode("error", &error),
                Frame::Phase(phase) => encode("phase", &PhaseEvent { phase }),
            });
        }
    };

    let stream = Sse::new(events).keep_alive(KeepAlive::new().interval(KEEP_ALIVE));

    // The same two proxy-facing headers as the message stream, for the same
    // two proxies.
    Ok((
        [
            (header::CACHE_CONTROL, "no-cache, no-transform"),
            (header::HeaderName::from_static("x-accel-buffering"), "no"),
        ],
        stream,
    ))
}

/// The `phase` event's body, an object rather than a bare string.
#[derive(Debug, serde::Serialize)]
struct PhaseEvent {
    phase: StreamPhase,
}

/// Serialise a frame, falling back to an error event rather than a panic.
fn encode<T: serde::Serialize>(name: &str, body: &T) -> Event {
    let event = Event::default().event(name);
    match serde_json::to_string(body) {
        Ok(json) => event.data(json),
        Err(error) => Event::default().event("error").data(format!(
            r#"{{"message":"could not encode {name}: {error}"}}"#
        )),
    }
}

/// Epoch milliseconds, saturating rather than panicking on a clock before 1970.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Run the scan to its end, its record cap, its deadline, or its first error
/// — whichever comes first — and always leave a `result` and a `done` behind.
async fn pump(
    admin: std::sync::Arc<kafka_admin::Admin>,
    spec: ScanSpec,
    limit: Option<u64>,
    deadline: Duration,
    tx: &streaming::Sender<Frame>,
) {
    tx.push(Frame::Phase(StreamPhase::Seeking));
    let topic = spec.topic.clone();
    let started_at = now_ms();
    let started = tokio::time::Instant::now();
    let deadline = started + deadline;

    let scan = match kafka_read::scan(admin.cluster(), spec).await {
        Ok(scan) => scan,
        Err(error) => {
            // The scan could not be *planned* — topic gone, no metadata. This
            // is the case a bare error is for: there is no fold to report.
            tx.push(Frame::Failed(Box::new(ResourceError::new(&topic, &error))));
            tx.push(Frame::Phase(StreamPhase::Done));
            return;
        }
    };
    let mut scan = Box::pin(scan);
    tx.push(Frame::Phase(StreamPhase::Streaming));

    let mut builder = TopicAnalysisBuilder::new();
    let mut last_progress: Option<kafka_read::ScanProgress> = None;
    let mut last_pushed = tokio::time::Instant::now() - PROGRESS_INTERVAL;

    // What stopped the walk, and whatever failed along it.
    let outcome: (AnalysisStop, Vec<ResourceError>) = loop {
        tokio::select! {
            () = tokio::time::sleep_until(deadline) => {
                // The configured time cap. The fold is real for what was
                // read; the result says this is where it stopped rather
                // than presenting the numbers as the topic's.
                break (AnalysisStop::TimeCap, Vec::new());
            }
            event = scan.next() => match event {
                Some(Ok(ScanEvent::Record(record))) => {
                    builder.record(&record);
                }
                Some(Ok(ScanEvent::Malformed { partition, .. })) => {
                    // A counter here where the message stream shows a row: the
                    // batch is skipped, the scan continues, and the count is
                    // part of the result rather than a failure of it.
                    builder.malformed(partition);
                }
                Some(Ok(ScanEvent::Progress(progress))) => {
                    if last_pushed.elapsed() >= PROGRESS_INTERVAL {
                        last_pushed = tokio::time::Instant::now();
                        tx.push(Frame::Progress(Box::new(render_progress(
                            &progress, &builder, limit, started_at, started,
                        ))));
                    }
                    last_progress = Some(progress);
                }
                Some(Ok(ScanEvent::Done(progress))) => {
                    // The library ends the scan for the cap and for the log
                    // end alike; which one it was is told by whether the
                    // offset span was consumed. A cap equal to the topic's
                    // count is the boundary case, and it reads as `end` —
                    // which is the honest answer there.
                    let capped = limit.is_some_and(|cap| builder.records() >= cap)
                        && progress.offsets_consumed < progress.offsets_total;
                    last_progress = Some(progress);
                    break (
                        if capped { AnalysisStop::MessageCap } else { AnalysisStop::End },
                        Vec::new(),
                    );
                }
                Some(Ok(ScanEvent::PartitionComplete { .. })) => {}
                Some(Err(error)) => {
                    // Upstream ask 13 is the honest fix: today one partition's
                    // failure ends the whole stream, so the fold up to here is
                    // what there is. It leaves flagged partial, with the error
                    // named — not discarded, and not dressed as complete.
                    break (AnalysisStop::Error, vec![ResourceError::new(&topic, &error)]);
                }
                None => break (AnalysisStop::End, Vec::new()),
            },
        }
        if tx.is_closed() {
            return;
        }
    };

    let (stopped_by, errors) = outcome;
    let fraction = last_progress
        .as_ref()
        .and_then(kafka_read::ScanProgress::fraction);
    // A finished scan is the whole window whatever the arithmetic says —
    // `fraction()` saturates at u32 (upstream ask 13), and reporting 0.97 on
    // a complete result would look like data loss.
    let fraction = if stopped_by == AnalysisStop::End {
        Some(1.0)
    } else {
        fraction
    };
    tx.push(Frame::Result(Box::new(builder.render(
        started_at,
        now_ms(),
        stopped_by,
        fraction,
        errors,
    ))));
    tx.push(Frame::Phase(StreamPhase::Done));
}

fn render_progress(
    progress: &kafka_read::ScanProgress,
    builder: &TopicAnalysisBuilder,
    limit: Option<u64>,
    started_at: i64,
    started: tokio::time::Instant,
) -> AnalysisProgress {
    AnalysisProgress {
        started_at,
        fraction: fraction(progress, builder, limit),
        msgs_scanned: builder.records(),
        bytes_scanned: builder.bytes_scanned(),
        offsets_consumed: progress.offsets_consumed,
        offsets_total: progress.offsets_total,
        malformed_batches: progress.malformed_batches,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    }
}

/// How far along the scan is, against whichever finish line is nearer.
///
/// The same two-ratio rule the message stream uses (and upstream ask 12 asks
/// kaas-lib to adopt): a capped scan reaches its cap long before the end of
/// the topic, so the offset span alone leaves the bar parked at a fraction it
/// will never pass. With the UI defaulting the cap to the topic's current
/// count, this is also what lets the bar honestly reach its end on a topic
/// that is still being produced to.
fn fraction(
    progress: &kafka_read::ScanProgress,
    builder: &TopicAnalysisBuilder,
    limit: Option<u64>,
) -> Option<f64> {
    let span = progress.fraction();
    let Some(cap) = limit.filter(|cap| *cap > 0) else {
        return span;
    };
    // The `u32` narrowing keeps `f64::from` lossless where `as` would round.
    let scanned = u32::try_from(builder.records()).unwrap_or(u32::MAX);
    let ceiling = u32::try_from(cap).unwrap_or(u32::MAX);
    let by_cap = (f64::from(scanned) / f64::from(ceiling)).clamp(0.0, 1.0);
    Some(span.map_or(by_cap, |span| span.max(by_cap)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The design decision this route exists around: a result that did not
    /// read everything must say so on itself, not in a log.
    #[test]
    fn a_truncated_analysis_is_flagged_on_the_result() {
        let builder = TopicAnalysisBuilder::new();
        let result = builder.render(0, 1, AnalysisStop::TimeCap, Some(0.42), Vec::new());
        assert!(!result.complete);
        assert_eq!(result.stopped_by, AnalysisStop::TimeCap);
        assert_eq!(result.scanned_fraction, Some(0.42));
    }

    /// A configured cap is a sample, not a failure: the result names the cap
    /// as what stopped it, carries no error, and still refuses to claim the
    /// topic's numbers.
    #[test]
    fn a_capped_analysis_is_a_sample_not_a_failure() {
        let builder = TopicAnalysisBuilder::new();
        let result = builder.render(0, 1, AnalysisStop::MessageCap, Some(0.25), Vec::new());
        assert!(!result.complete);
        assert_eq!(result.stopped_by, AnalysisStop::MessageCap);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn the_progress_bar_tracks_the_nearer_finish_line() {
        let mut builder = TopicAnalysisBuilder::new();
        for offset in 0..80 {
            builder.record(&test_record(offset));
        }
        let progress = kafka_read::ScanProgress {
            records_emitted: 80,
            records_scanned: 80,
            malformed_batches: 0,
            offsets_consumed: 80,
            offsets_total: 8_000,
            partitions_active: 1,
            ordering_degraded: false,
        };
        // Uncapped: 1% of the span. Capped at 100: 80% of the cap.
        let by_span = fraction(&progress, &builder, None).unwrap_or(0.0);
        assert!(by_span < 0.02, "{by_span}");
        let by_cap = fraction(&progress, &builder, Some(100)).unwrap_or(0.0);
        assert!((0.79..=0.81).contains(&by_cap), "{by_cap}");
    }

    fn test_record(offset: i64) -> kafka_read::Record {
        kafka_read::Record {
            topic: "orders".to_owned(),
            partition: 0,
            offset,
            timestamp: 1_754_000_000_000,
            timestamp_type: kafka_read::TimestampType::Creation,
            key: None,
            value: None,
            headers: Vec::new(),
            producer_id: None,
            transactional: false,
            leader_epoch: None,
        }
    }
}
