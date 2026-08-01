//! The message browser's one-shot tail.

use axum::Json;
use axum::extract::{Path, Query, State};
use kaas_ui_core::dto::Message;
use kaas_ui_core::envelope::Envelope;
use kafka_read::TailSpec;
use serde::Deserialize;

use crate::routes::split_list;
use crate::{ApiError, ApiResult, AppState};

/// The most records one request will return.
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
    Path((id, topic)): Path<(String, String)>,
    Query(query): Query<TailQuery>,
) -> ApiResult<Json<Envelope<Message>>> {
    let (_, admin) = state.connected(&id)?;

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
        spec = spec.with_partitions(partitions);
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
