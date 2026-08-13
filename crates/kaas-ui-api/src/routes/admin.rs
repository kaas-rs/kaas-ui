//! The read-only admin surface: ACLs, quotas, SCRAM users, reassignments and
//! transactions.
//!
//! Five screens over calls that already existed, and the phase where kaas-ui
//! goes past kafbat-ui rather than catching up. Every one of them is a read;
//! the mutating neighbours (`CreateAcls`, `AlterClientQuotas`,
//! `AlterUserScramCredentials`, `AlterPartitionReassignments`, `ElectLeaders`)
//! are unreachable from any route in this file, and the invariant grep is what
//! keeps that true rather than this paragraph.
//!
//! **All five are `Resource::ClusterConfig` + `Action::View`** — the grant that
//! already covers brokers, log dirs and configs, because these are facts about
//! a cluster rather than about a named topic or group. A new `Resource` variant
//! would have been the obvious move and is the wrong one: `Resource::every()`
//! is what a role saying `all` expands to, so adding one silently narrows every
//! deployed role that has it. The same argument retired `Resource::Schema` in
//! Phase 6.
//!
//! **A cluster that does not implement the api is not an error here.** The
//! capability projection is what hides the tab; a caller who asks anyway gets
//! kaas-lib's `UnsupportedApi`, which the error mapping already renders as the
//! panel naming both version ranges.

use axum::Json;
use axum::extract::{Path, Query, State};
use kaas_ui_core::admin::{
    Acl, ClientQuota, Producer, Reassignment, ScramCredential, ScramUser, Transaction,
};
use kaas_ui_core::envelope::Envelope;
use kafka_admin::{AclFilter, QuotaFilter};
use serde::Deserialize;

use kaas_ui_auth::{Action, Resource};

use crate::routes::split_list;
use crate::{ApiError, ApiResult, AppState, Caller, call};

/// `GET /api/environments/{env}/clusters/{id}/acls`
///
/// Every binding the authorizer holds, in one call.
///
/// The filter is `AclFilter::default()`, which is "show me all" — every field
/// `Any`, and it is *not* what `Default` would be if the enums defaulted to
/// their first variant. kaas-lib writes that default explicitly for this
/// reason, and the UI does not narrow it: 24 bindings is a page, and filtering
/// by principal is something a reader does to a table they can already see.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/acls",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
    ),
    responses((status = 200, description = "ACL bindings", body = Envelope<Acl>)),
    tag = "admin",
)]
pub async fn acls(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
) -> ApiResult<Json<Envelope<Acl>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(
        &id,
        &handle.labels,
        Resource::ClusterConfig,
        Action::View,
        None,
    )?;

    let bindings = call("describe_acls", admin.describe_acls(&AclFilter::default())).await?;

    // Sorted here rather than in the browser, for the reason every other list
    // is: the order is a property of the answer, and a table that reorders
    // itself when a binding is added somewhere else is a table nobody trusts.
    let mut acls: Vec<Acl> = bindings.iter().map(Acl::from).collect();
    acls.sort_by(|a, b| {
        (
            &a.resource_type,
            &a.resource_name,
            &a.principal,
            &a.operation,
        )
            .cmp(&(
                &b.resource_type,
                &b.resource_name,
                &b.principal,
                &b.operation,
            ))
    });

    Ok(Json(Envelope::new(acls)))
}

/// `GET /api/environments/{env}/clusters/{id}/quotas`
///
/// Every configured quota, across the three entity types.
///
/// One call per entity type rather than one unfiltered call: an empty
/// component list asks about *no* entity type, and the broker answers with
/// nothing. Partial failure is a result — a cluster that answers for users and
/// not for IPs renders the users and names the failure.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/quotas",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
    ),
    responses((status = 200, description = "Client quotas", body = Envelope<ClientQuota>)),
    tag = "admin",
)]
pub async fn quotas(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
) -> ApiResult<Json<Envelope<ClientQuota>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(
        &id,
        &handle.labels,
        Resource::ClusterConfig,
        Action::View,
        None,
    )?;

    let mut quotas: Vec<ClientQuota> = Vec::new();
    let mut errors = Vec::new();
    for entity_type in ["user", "client-id", "ip"] {
        let filter = QuotaFilter {
            components: vec![(entity_type.to_owned(), None)],
            strict: false,
        };
        match call(
            "describe_client_quotas",
            admin.describe_client_quotas(&filter),
        )
        .await
        {
            Ok(found) => quotas.extend(
                found
                    .iter()
                    .map(|(entity, values)| ClientQuota::of(entity, values)),
            ),
            Err(error) => errors.push(error.into_resource_error(entity_type)),
        }
    }

    // The same entity comes back under every component type it names, so
    // `user=alice, client-id=app` arrives from both the user query and the
    // client-id one. Deduplicated on the rendered entity, which is the
    // identity the reader sees.
    let mut seen = std::collections::BTreeSet::new();
    quotas.retain(|quota| seen.insert(entity_key(quota)));
    quotas.sort_by_key(entity_key);

    Ok(Json(Envelope::new(quotas).with_errors(errors)))
}

/// `user=alice, client-id=app` — the entity as one comparable string.
fn entity_key(quota: &ClientQuota) -> String {
    quota
        .entity
        .iter()
        .map(|component| {
            format!(
                "{}={}",
                component.entity_type,
                component.name.as_deref().unwrap_or("<default>")
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `GET /api/environments/{env}/clusters/{id}/scram-users`
///
/// Who can authenticate with SCRAM, and with what — **never how**. The broker
/// stores a salted hash and cannot return one; there is no field on the wire
/// that could carry a credential even if a handler wanted to.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/scram-users",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
    ),
    responses((status = 200, description = "SCRAM credentials, by user", body = Envelope<ScramUser>)),
    tag = "admin",
)]
pub async fn scram_users(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
) -> ApiResult<Json<Envelope<ScramUser>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(
        &id,
        &handle.labels,
        Resource::ClusterConfig,
        Action::View,
        None,
    )?;

    // `None` is every user. kaas-lib turns that into an empty user list rather
    // than a null one, because a null list makes some brokers answer nothing
    // at all and the client waits out its timeout — a hung describe where the
    // honest answer was an authorization failure.
    let described = call(
        "describe_scram_credentials",
        admin.describe_scram_credentials(None),
    )
    .await?;

    let mut envelope =
        Envelope::from_per_item(described, Clone::clone, |user, credentials| ScramUser {
            user: user.clone(),
            credentials: credentials.iter().map(ScramCredential::from).collect(),
        });
    envelope.items.sort_by(|a, b| a.user.cmp(&b.user));

    Ok(Json(envelope))
}

/// `GET /api/environments/{env}/clusters/{id}/reassignments`
///
/// What is moving right now. `None` asks about the whole cluster, which is the
/// question the screen exists to answer; an empty list is a healthy cluster and
/// renders as one rather than as an absence.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/reassignments",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
    ),
    responses((status = 200, description = "Reassignments in flight", body = Envelope<Reassignment>)),
    tag = "admin",
)]
pub async fn reassignments(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
) -> ApiResult<Json<Envelope<Reassignment>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(
        &id,
        &handle.labels,
        Resource::ClusterConfig,
        Action::View,
        None,
    )?;

    let moving = call(
        "list_partition_reassignments",
        admin.list_partition_reassignments(None),
    )
    .await?;

    let mut rows: Vec<Reassignment> = moving.iter().map(Reassignment::from).collect();
    rows.sort_by(|a, b| (&a.topic, a.partition).cmp(&(&b.topic, b.partition)));

    Ok(Json(Envelope::new(rows)))
}

/// The transaction list's one option.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionQuery {
    /// Describe the listed transactions, not just list them.
    ///
    /// Opt-in and second, the way `?metrics=true` is: `ListTransactions` is one
    /// call to every broker, and `DescribeTransactions` is a call per
    /// transactional id routed to its coordinator. A cluster with a thousand
    /// open transactions should paint a table and then fill it.
    #[serde(default)]
    pub details: bool,
    /// Only these states, comma-separated. The broker's vocabulary, passed
    /// through: this is a filter it applies, not one kaas-ui knows the values
    /// of.
    pub state: Option<String>,
}

/// `GET /api/environments/{env}/clusters/{id}/transactions`
///
/// The list, and with `?details=true` everything the describe adds: the start
/// timestamp, the timeout, the producer epoch and the partitions enrolled.
///
/// **The start timestamp, never a duration.** `open_for_ms` takes a `now`, and
/// whichever `now` a handler passes is wrong by the time the response is read
/// and wronger every second the page stays open — on the one column this screen
/// is sorted by. The browser ticks it, exactly as it ticks `snapshotAgeMs`.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/transactions",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
        ("details" = Option<bool>, Query, description = "Describe each listed transaction"),
        ("state" = Option<String>, Query, description = "Only these states, comma-separated"),
    ),
    responses((status = 200, description = "Transactions", body = Envelope<Transaction>)),
    tag = "admin",
)]
pub async fn transactions(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
    Query(query): Query<TransactionQuery>,
) -> ApiResult<Json<Envelope<Transaction>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(
        &id,
        &handle.labels,
        Resource::ClusterConfig,
        Action::View,
        None,
    )?;

    let states = query.state.as_deref().map(split_list).unwrap_or_default();
    let filters: Vec<&str> = states.iter().map(String::as_str).collect();
    let listings = call("list_transactions", admin.list_transactions(&filters)).await?;

    if !query.details {
        let mut rows: Vec<Transaction> = listings.iter().map(Transaction::from).collect();
        rows.sort_by(|a, b| a.transactional_id.cmp(&b.transactional_id));
        return Ok(Json(Envelope::new(rows)));
    }

    let ids: Vec<String> = listings
        .iter()
        .map(|listing| listing.transactional_id.clone())
        .collect();
    if ids.is_empty() {
        return Ok(Json(Envelope::new(Vec::new())));
    }

    // A transaction that committed between the list and the describe is an
    // error on that id and not on the response: twenty-eight described and two
    // gone is `200 OK` with twenty-eight items and two errors.
    let described = call("describe_transactions", admin.describe_transactions(ids)).await?;
    let mut envelope = Envelope::from_per_item(described, Clone::clone, |_, description| {
        Transaction::from(&description)
    });
    envelope
        .items
        .sort_by(|a, b| a.transactional_id.cmp(&b.transactional_id));

    Ok(Json(envelope))
}

/// `GET /api/environments/{env}/clusters/{id}/transactions/{txn}`
///
/// One transaction, described. The list already carries this under
/// `?details=true`; this is the addressable form, so a stuck transaction is a
/// link somebody can send.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/transactions/{txn}",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
        ("txn" = String, Path, description = "Transactional id"),
    ),
    responses((status = 200, description = "One transaction", body = Envelope<Transaction>)),
    tag = "admin",
)]
pub async fn transaction(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, txn)): Path<(String, String, String)>,
) -> ApiResult<Json<Envelope<Transaction>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(
        &id,
        &handle.labels,
        Resource::ClusterConfig,
        Action::View,
        None,
    )?;

    let described = call("describe_transactions", admin.describe_transactions([txn])).await?;
    Ok(Json(Envelope::from_per_item(
        described,
        Clone::clone,
        |_, description| Transaction::from(&description),
    )))
}

/// Which partitions to ask about.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProducerQuery {
    /// The topic.
    pub topic: Option<String>,
    /// Partitions, comma-separated. Omitted asks about every partition of the
    /// topic.
    pub partition: Option<String>,
}

/// `GET /api/environments/{env}/clusters/{id}/producers?topic=…&partition=…`
///
/// The producers writing to a topic's partitions — how the one holding a
/// transaction open is found.
///
/// Routed by kaas-lib to each partition's **leader**, because producer state is
/// leader state and a follower does not have it. Do not fan this out by hand.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/clusters/{id}/producers",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Query, description = "Topic name"),
        ("partition" = Option<String>, Query, description = "Partitions, comma-separated; every partition if omitted"),
    ),
    responses((status = 200, description = "Producer state per partition", body = Envelope<Producer>)),
    tag = "admin",
)]
pub async fn producers(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
    Query(query): Query<ProducerQuery>,
) -> ApiResult<Json<Envelope<Producer>>> {
    let (handle, admin) = state.connected(&env, &id, &caller)?;
    caller.require(
        &id,
        &handle.labels,
        Resource::ClusterConfig,
        Action::View,
        None,
    )?;

    let topic = query
        .topic
        .as_deref()
        .map(str::trim)
        .filter(|topic| !topic.is_empty())
        .ok_or_else(|| ApiError::bad_request("?topic= is required"))?;

    // From the snapshot, because the partition indexes of a topic are not
    // required to be `0..count` and this route must not invent one that does
    // not exist.
    let snapshot = admin.cluster().snapshot();
    let known: Vec<i32> = snapshot
        .topic(topic)
        .map(|info| {
            info.partitions
                .iter()
                .map(|partition| partition.partition)
                .collect()
        })
        .unwrap_or_default();

    let wanted: Vec<i32> = match query.partition.as_deref() {
        Some(raw) => {
            let asked: Vec<i32> = split_list(raw)
                .iter()
                .filter_map(|part| part.parse().ok())
                .collect();
            if asked.is_empty() {
                return Err(ApiError::bad_request(
                    "?partition= held no partition numbers",
                ));
            }
            asked
        }
        None => known,
    };

    if wanted.is_empty() {
        return Err(ApiError::bad_request(format!(
            "topic {topic:?} is not on this cluster"
        )));
    }

    let pairs: Vec<(String, i32)> = wanted
        .into_iter()
        .map(|partition| (topic.to_owned(), partition))
        .collect();
    let described = call("describe_producers", admin.describe_producers(pairs)).await?;

    // One row per producer rather than per partition: the question is "which
    // producer is stuck", and a partition with three of them is three rows.
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for ((topic, partition), outcome) in &described {
        match outcome {
            Ok(states) => rows.extend(
                states
                    .iter()
                    .map(|state| Producer::of(topic, *partition, state)),
            ),
            Err(error) => errors.push(kaas_ui_core::ResourceError::new(
                format!("{topic}-{partition}"),
                error,
            )),
        }
    }
    rows.sort_by_key(|row| (row.partition, row.producer_id));

    Ok(Json(Envelope::new(rows).with_errors(errors)))
}
