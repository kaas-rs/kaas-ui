//! The seven ways to ask for a window of a topic.
//!
//! One table drives the server and the frontend alike. Both need to know that
//! `toOffset` reads backwards and `oldest` reads forwards, and the moment that
//! knowledge is spelled out twice they disagree — usually as a list that sorts
//! the wrong way round for exactly one mode.
//!
//! Two library calls back all seven. [`kafka_read::scan`] reads forward and
//! streams; [`kafka_read::tail`] walks backward and returns a `Vec`. Which one
//! a mode uses is not a detail: a backward mode has **no partial results**, so
//! it shows a seeking state and then the whole window at once, while a forward
//! mode fills the list as it goes.

use bytes::Bytes;
use kafka_read::{RecordFilter, ScanSpec, StartPosition, TailAnchor, TailSpec, Visibility};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{ApiError, ApiResult};

/// The most records any bounded read will return.
///
/// Mandatory on the backward modes rather than merely advisable: `tail`
/// buffers its whole window before returning anything, so an unbounded
/// backward read is a request to hold a topic in memory.
pub const MAX_LIMIT: usize = 10_000;

/// What a bounded read returns when the caller does not say.
pub const DEFAULT_LIMIT: usize = 500;

/// The memory ceiling for a live tail, in decoded records.
///
/// Lower than the library's default because a tail is open for a long time and
/// the reader only ever sees the newest end of it. Buffering ten thousand
/// records to render thirty rows is the wrong trade for a stream that never
/// finishes. Lowering it widens the cross-partition reorder window, which is
/// why the stream reports that window rather than implying a total order.
pub const LIVE_MAX_BUFFERED: usize = 2_000;

/// How long a live tail's fetch may wait for records.
///
/// Well below the library's 500 ms default, and the reason is head-of-line
/// blocking rather than latency. kaas-lib keeps **one connection per broker**,
/// shared by every caller, and Kafka answers a connection's requests **in
/// order** — so a long-polling `Fetch` sitting at the head of that queue
/// delays every `ListOffsets` and `Metadata` behind it by up to its own wait.
///
/// With several live views open the delays add: six streams at 500 ms made an
/// `/offsets` call that normally takes 2 ms take three to four seconds, and a
/// process that had accumulated abandoned streams took nine.
///
/// Lowering it divides that constant. It does not remove it — the honest fix
/// is a connection that streaming reads do not share, which is kaas-lib's to
/// give. Filed as upstream ask 11.
pub const LIVE_MAX_WAIT_MS: i32 = 100;

/// How a window of a topic was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SeekMode {
    /// Follow the log as records arrive. The only mode that does not end.
    Live,
    /// The most recent records, then stop.
    Newest,
    /// From the start of what the topic still retains.
    Oldest,
    /// Forward from an explicit offset.
    FromOffset,
    /// Backward from an explicit offset, which is included.
    ToOffset,
    /// Forward from an instant.
    SinceTime,
    /// Backward from an instant, which is included.
    ToTime,
}

impl SeekMode {
    /// Whether the stream stays open after the window is read.
    pub fn is_live(self) -> bool {
        matches!(self, Self::Live)
    }

    /// Whether this mode walks backwards.
    ///
    /// The axis that matters, and the one the UI derives its scroll behaviour
    /// from. Backward modes prepend and have no partial results; forward modes
    /// append and stream.
    pub fn is_backward(self) -> bool {
        matches!(self, Self::Newest | Self::ToOffset | Self::ToTime)
    }

    /// The instant this mode seeks to, where it seeks to one at all.
    ///
    /// The other five modes have no timestamp to resolve, and asking a broker
    /// about one would cost a round trip to learn nothing.
    pub fn timestamp_of(self, query: &SeekQuery) -> Option<i64> {
        match self {
            Self::SinceTime | Self::ToTime => query.timestamp,
            _ => None,
        }
    }

    /// Which parameter this mode requires, if any.
    fn requires(self) -> Option<&'static str> {
        match self {
            Self::FromOffset | Self::ToOffset => Some("offset"),
            Self::SinceTime | Self::ToTime => Some("timestamp"),
            _ => None,
        }
    }
}

/// The query every message read shares.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeekQuery {
    /// Which of the seven.
    pub mode: Option<SeekMode>,
    /// For `fromOffset` and `toOffset`.
    pub offset: Option<i64>,
    /// For `sinceTime` and `toTime`, in epoch milliseconds.
    ///
    /// Milliseconds and not ISO 8601 because that is what `ListOffsets` takes.
    /// One conversion, in the picker, and nothing between it and the broker
    /// reinterprets a timezone.
    pub timestamp: Option<i64>,
    /// Restrict to these partitions, comma-separated.
    pub partitions: Option<String>,
    /// `all` or `committed`.
    pub visibility: Option<String>,
    /// Substring match on the decoded value.
    pub filter: Option<String>,
    /// How many records. Ignored by `live`.
    pub limit: Option<usize>,
    /// How to read keys, overriding the per-topic configuration.
    ///
    /// The chip in the message list, travelling as a query parameter so the
    /// URL stays the shareable artifact. It can always fall *back* — hex and
    /// string need no schema, so they work with the registry down — and it
    /// cannot invent a schema id to move up.
    pub key_codec: Option<kaas_ui_serde::Codec>,
    /// How to read values, overriding the per-topic configuration.
    pub value_codec: Option<kaas_ui_serde::Codec>,
    /// A JavaScript expression over the decoded value.
    ///
    /// The **second** tier of filtering, and never the first: `filter`,
    /// `partitions`, `offset` and the timestamps above are cheap and go into
    /// the scan spec, where kaas-lib applies them before a record is ever
    /// deserialised. This one runs on the decoded value, after them, in a
    /// sandbox with a memory cap and an interrupt handler.
    pub predicate: Option<String>,
}

impl SeekQuery {
    /// Compile the user predicate, if this request carries one.
    ///
    /// A `Result`, so an expression that does not compile is a `400` naming
    /// the syntax error rather than a filter that silently matches nothing.
    pub fn compile_predicate(&self) -> crate::ApiResult<Option<kaas_ui_serde::Predicate>> {
        let Some(source) = self
            .predicate
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return Ok(None);
        };
        kaas_ui_serde::Predicate::compile(source)
            .map(Some)
            .map_err(|error| ApiError::bad_request(error.to_string()))
    }

    /// The codec chips, as this request set them.
    #[must_use]
    pub fn codecs(&self) -> kaas_ui_core::decode::CodecOverride {
        kaas_ui_core::decode::CodecOverride {
            key: self.key_codec,
            value: self.value_codec,
        }
    }
}

/// A validated read, in the shape the library takes.
#[derive(Debug)]
pub enum Plan {
    /// A forward read. Streams, and may follow the log.
    Forward {
        /// The spec to hand [`kafka_read::scan`].
        spec: Box<ScanSpec>,
        /// Records below this are dropped.
        ///
        /// A guard rather than a filter in the usual case: `scan` clamps an
        /// out-of-range start into the log, so asking to read from an offset
        /// the topic no longer retains silently answers from its earliest.
        /// That is right for browsing and wrong for "did my record land at
        /// 900001", so the requested floor is remembered and enforced.
        floor: Option<i64>,
    },
    /// A backward read. Returns everything at once or nothing.
    Backward {
        /// The spec to hand [`kafka_read::tail`].
        spec: Box<TailSpec>,
    },
}

impl Plan {
    /// The partitions this read covers, or `None` for all of them.
    pub fn partitions(&self) -> Option<&[i32]> {
        match self {
            Self::Forward { spec, .. } => spec.partitions.as_deref(),
            Self::Backward { spec } => spec.partitions.as_deref(),
        }
    }

    /// Validate a query into a plan, or say exactly what is missing.
    pub fn build(topic: &str, query: &SeekQuery) -> ApiResult<(SeekMode, Self)> {
        // `newest`, not `live`. A caller that names no mode is asking for a
        // look at the topic, and answering with the one mode that never ends
        // hands them an open long poll — which, on a shared broker connection,
        // is a cost to every other reader of that cluster. The web app's
        // `DEFAULT_SEEK_MODE` says the same thing on its own side.
        let mode = query.mode.unwrap_or(SeekMode::Newest);

        if let Some(needed) = mode.requires() {
            let present = match needed {
                "offset" => query.offset.is_some(),
                _ => query.timestamp.is_some(),
            };
            if !present {
                return Err(ApiError::bad_request(format!(
                    "mode={} requires ?{needed}=",
                    serde_json::to_string(&mode)
                        .unwrap_or_default()
                        .trim_matches('"')
                )));
            }
        }
        if let Some(offset) = query.offset
            && offset < 0
        {
            return Err(ApiError::bad_request("?offset= must not be negative"));
        }

        let partitions = parse_partitions(query.partitions.as_deref())?;
        let visibility = parse_visibility(query.visibility.as_deref())?;
        let filter = query
            .filter
            .as_deref()
            .map(str::trim)
            .filter(|needle| !needle.is_empty())
            .map(|needle| RecordFilter::ValueContains(Bytes::from(needle.to_owned())));

        let limit = limit_for(mode, query.limit)?;

        let plan = if mode.is_backward() {
            let mut spec = TailSpec::new(topic.to_owned(), limit.unwrap_or(DEFAULT_LIMIT));
            spec = spec.visibility(visibility);
            spec = spec.ending_at(match mode {
                SeekMode::ToOffset => TailAnchor::Offset(query.offset.unwrap_or_default()),
                SeekMode::ToTime => TailAnchor::Timestamp(query.timestamp.unwrap_or_default()),
                _ => TailAnchor::LogEnd,
            });
            if let Some(partitions) = partitions {
                spec = spec.partitions(partitions);
            }
            if let Some(filter) = filter {
                spec = spec.filter(filter);
            }
            Self::Backward {
                spec: Box::new(spec),
            }
        } else {
            let from = match mode {
                SeekMode::Live => StartPosition::Latest,
                SeekMode::Oldest => StartPosition::Earliest,
                SeekMode::FromOffset => StartPosition::Offset(query.offset.unwrap_or_default()),
                SeekMode::SinceTime => {
                    StartPosition::Timestamp(query.timestamp.unwrap_or_default())
                }
                _ => StartPosition::Latest,
            };

            let mut spec = ScanSpec::new(topic.to_owned())
                .from(from)
                .visibility(visibility);
            if mode.is_live() {
                // A live view is a tail, not a browse: without this the scan
                // plans against the log end it is already standing on and
                // finishes at once, which looks exactly like a working live
                // view of a quiet topic.
                spec = spec.following();
                spec.max_buffered_records = LIVE_MAX_BUFFERED;
                spec.max_wait_ms = LIVE_MAX_WAIT_MS;
            }
            if let Some(limit) = limit {
                spec = spec.limit(limit);
            }
            if let Some(partitions) = partitions {
                spec = spec.partitions(partitions);
            }
            if let Some(filter) = filter {
                spec = spec.filter(filter);
            }

            Self::Forward {
                spec: Box::new(spec),
                floor: match mode {
                    SeekMode::FromOffset => query.offset,
                    _ => None,
                },
            }
        };

        Ok((mode, plan))
    }
}

/// The record ceiling for a mode, or `None` where there is none.
fn limit_for(mode: SeekMode, asked: Option<usize>) -> ApiResult<Option<usize>> {
    if let Some(limit) = asked
        && (limit == 0 || limit > MAX_LIMIT)
    {
        return Err(ApiError::bad_request(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }
    if mode.is_live() {
        // Not "unlimited by omission" — a live tail has no window to bound,
        // and honouring a limit would turn it into a snapshot that stops.
        return Ok(None);
    }
    Ok(Some(asked.unwrap_or(DEFAULT_LIMIT)))
}

fn parse_partitions(raw: Option<&str>) -> ApiResult<Option<Vec<i32>>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let partitions: Vec<i32> = super::super::split_list(raw)
        .iter()
        .map(|part| {
            part.parse::<i32>()
                .map_err(|_| ApiError::bad_request(format!("partition {part:?} is not a number")))
        })
        .collect::<ApiResult<Vec<i32>>>()?;
    if partitions.is_empty() {
        return Err(ApiError::bad_request("?partitions= was empty"));
    }
    Ok(Some(partitions))
}

fn parse_visibility(raw: Option<&str>) -> ApiResult<Visibility> {
    match raw {
        None | Some("all") => Ok(Visibility::All),
        Some("committed") => Ok(Visibility::CommittedOnly),
        Some(other) => Err(ApiError::bad_request(format!(
            "visibility must be \"all\" or \"committed\", not {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(mode: SeekMode) -> SeekQuery {
        SeekQuery {
            mode: Some(mode),
            ..SeekQuery::default()
        }
    }

    #[test]
    fn the_backward_modes_are_exactly_the_three_that_walk_back() {
        // The axis the UI derives scroll behaviour from. A mode landing on the
        // wrong side of it drags the viewport on every arriving record.
        assert!(SeekMode::Newest.is_backward());
        assert!(SeekMode::ToOffset.is_backward());
        assert!(SeekMode::ToTime.is_backward());
        assert!(!SeekMode::Live.is_backward());
        assert!(!SeekMode::Oldest.is_backward());
        assert!(!SeekMode::FromOffset.is_backward());
        assert!(!SeekMode::SinceTime.is_backward());
    }

    #[test]
    fn a_seek_mode_without_its_parameter_is_rejected_by_name() {
        for (mode, needed) in [
            (SeekMode::FromOffset, "offset"),
            (SeekMode::ToOffset, "offset"),
            (SeekMode::SinceTime, "timestamp"),
            (SeekMode::ToTime, "timestamp"),
        ] {
            let error = Plan::build("orders", &query(mode)).unwrap_err();
            assert_eq!(error.status(), axum::http::StatusCode::BAD_REQUEST);
            let rendered = format!("{:?}", error);
            assert!(
                rendered.contains(needed),
                "{mode:?} must name the parameter it wanted, got {rendered}"
            );
        }
    }

    #[test]
    fn the_modes_that_need_nothing_build_from_an_empty_query() {
        for mode in [SeekMode::Live, SeekMode::Newest, SeekMode::Oldest] {
            assert!(Plan::build("orders", &query(mode)).is_ok(), "{mode:?}");
        }
    }

    #[test]
    fn a_live_stream_follows_the_log_and_holds_less_of_it() {
        let (_, plan) = Plan::build("orders", &query(SeekMode::Live)).unwrap();
        match plan {
            Plan::Forward { spec, floor } => {
                assert!(
                    spec.follow,
                    "a live view that does not follow shows nothing"
                );
                assert_eq!(spec.from, StartPosition::Latest);
                assert_eq!(spec.limit, None, "a tail has no window to bound");
                assert_eq!(spec.max_buffered_records, LIVE_MAX_BUFFERED);
                assert_eq!(
                    spec.max_wait_ms, LIVE_MAX_WAIT_MS,
                    "a long poll holds the shared connection against everything behind it"
                );
                assert_eq!(floor, None);
            }
            other => panic!("live must be a forward plan, got {other:?}"),
        }
    }

    #[test]
    fn a_bounded_forward_mode_does_not_follow() {
        let (_, plan) = Plan::build("orders", &query(SeekMode::Oldest)).unwrap();
        match plan {
            Plan::Forward { spec, .. } => {
                assert!(!spec.follow, "a snapshot that never ends is not a snapshot");
                assert_eq!(spec.limit, Some(DEFAULT_LIMIT));
            }
            other => panic!("expected a forward plan, got {other:?}"),
        }
    }

    #[test]
    fn to_offset_anchors_the_backward_walk_at_the_offset_itself() {
        let plan = Plan::build(
            "orders",
            &SeekQuery {
                mode: Some(SeekMode::ToOffset),
                offset: Some(16_733),
                ..SeekQuery::default()
            },
        )
        .unwrap()
        .1;
        match plan {
            Plan::Backward { spec } => {
                assert_eq!(spec.anchor, TailAnchor::Offset(16_733));
                assert_eq!(spec.limit, DEFAULT_LIMIT);
            }
            other => panic!("toOffset must be a backward plan, got {other:?}"),
        }
    }

    #[test]
    fn to_time_anchors_at_the_instant() {
        let plan = Plan::build(
            "orders",
            &SeekQuery {
                mode: Some(SeekMode::ToTime),
                timestamp: Some(1_754_040_945_671),
                ..SeekQuery::default()
            },
        )
        .unwrap()
        .1;
        match plan {
            Plan::Backward { spec } => {
                assert_eq!(spec.anchor, TailAnchor::Timestamp(1_754_040_945_671));
            }
            other => panic!("toTime must be a backward plan, got {other:?}"),
        }
    }

    #[test]
    fn newest_walks_back_from_the_log_end() {
        let plan = Plan::build("orders", &query(SeekMode::Newest)).unwrap().1;
        match plan {
            Plan::Backward { spec } => assert_eq!(spec.anchor, TailAnchor::LogEnd),
            other => panic!("expected a backward plan, got {other:?}"),
        }
    }

    #[test]
    fn from_offset_remembers_the_floor_it_asked_for() {
        // `scan` clamps an out-of-range start into the log, so without the
        // floor "read from 900001" on a topic that starts at 12000 answers
        // from 12000 and looks like it worked.
        let plan = Plan::build(
            "orders",
            &SeekQuery {
                mode: Some(SeekMode::FromOffset),
                offset: Some(900_001),
                ..SeekQuery::default()
            },
        )
        .unwrap()
        .1;
        match plan {
            Plan::Forward { floor, .. } => assert_eq!(floor, Some(900_001)),
            other => panic!("expected a forward plan, got {other:?}"),
        }
    }

    #[test]
    fn a_limit_beyond_the_ceiling_is_refused_rather_than_clamped() {
        // Silently returning 10,000 for a request of 50,000 makes the caller
        // believe they have the whole window.
        let error = Plan::build(
            "orders",
            &SeekQuery {
                mode: Some(SeekMode::Newest),
                limit: Some(MAX_LIMIT + 1),
                ..SeekQuery::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.status(), axum::http::StatusCode::BAD_REQUEST);

        let zero = Plan::build(
            "orders",
            &SeekQuery {
                mode: Some(SeekMode::Newest),
                limit: Some(0),
                ..SeekQuery::default()
            },
        );
        assert!(
            zero.is_err(),
            "a window of nothing is a mistake, not a query"
        );
    }

    #[test]
    fn visibility_is_one_of_two_words() {
        assert_eq!(parse_visibility(None).unwrap(), Visibility::All);
        assert_eq!(parse_visibility(Some("all")).unwrap(), Visibility::All);
        assert_eq!(
            parse_visibility(Some("committed")).unwrap(),
            Visibility::CommittedOnly
        );
        assert!(parse_visibility(Some("uncommitted")).is_err());
    }

    #[test]
    fn a_filter_of_whitespace_is_no_filter() {
        // Otherwise clearing the box leaves a filter that matches everything
        // and costs a comparison per record.
        let plan = Plan::build(
            "orders",
            &SeekQuery {
                mode: Some(SeekMode::Oldest),
                filter: Some("   ".to_owned()),
                ..SeekQuery::default()
            },
        )
        .unwrap()
        .1;
        match plan {
            Plan::Forward { spec, .. } => assert!(spec.filter.is_none()),
            other => panic!("expected a forward plan, got {other:?}"),
        }
    }

    #[test]
    fn partitions_are_parsed_and_an_empty_list_is_a_mistake() {
        assert_eq!(
            parse_partitions(Some("0,2,5")).unwrap(),
            Some(vec![0, 2, 5])
        );
        assert!(parse_partitions(Some("0,x")).is_err());
        assert!(parse_partitions(Some(",,")).is_err());
        assert_eq!(parse_partitions(None).unwrap(), None);
    }
}
