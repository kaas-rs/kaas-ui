# The HTTP contract

Shared by every phase. PLAN.md §4 sets the shape; this fills in the parts a
router needs.

## Every data endpoint is a GET

Because reading is what they do — not because a check forbids the alternative.
The auth flow adds `POST /auth/logout`, and the Dex proxy at `/dex/*` forwards
whatever method the browser sends. What guarantees read-only is the single
`Admin::connect_read_only` construction site: a handler reached by any verb has
no client that can write.

`/auth/login` is a `GET` and has to stay one: `SameSite=Lax` sends the pending
cookie on a top-level navigation and not on a `fetch`. Logout is the mirror
image and is a `POST` for the same reason. `login_is_a_navigation` in
`cargo xtask ci` holds both in place.

## The envelope

Partial results are the default, not a special case, because kaas-lib returns
`Vec<(ResourceId, Result<T, Error>)>` and collapsing that discards the property
on precisely the clusters that most need a UI.

```json
{
  "items": [ { "name": "orders", "partitions": 6 } ],
  "errors": [
    {
      "resource": "shipments",
      "kind": "broker",
      "code": "UNKNOWN_TOPIC_OR_PARTITION",
      "codeNumber": 3,
      "message": "..."
    }
  ],
  "snapshotAgeMs": 4213
}
```

`200 OK` even when `errors` is non-empty: the call succeeded, some resources did
not.

`codeNumber` is always present even when `code` is `null`. Against a broker
newer than the codec, `ErrorCode::Unknown(i16)` is all there is, and the number
is the only searchable thing. This is not hypothetical — Strimzi here runs
Kafka 4.2 against Kafka 4.0 schemas.

In Rust:

```rust
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Envelope<T> {
    pub items: Vec<T>,
    pub errors: Vec<ResourceError>,
    pub snapshot_age_ms: Option<u64>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceError {
    pub resource: String,
    pub kind: ErrorKind,      // transport | timeout | auth | broker | decode | unsupported | invalid
    pub code: Option<String>,
    pub code_number: Option<i16>,
    pub message: String,
}
```

`Envelope<T>` is built from a `PerItem<K, T>` by one function in
`kaas-ui-core`. There should be exactly one, and no handler should construct an
envelope by hand.

## Error mapping

| `kafka_conn::Error` | Status | Frontend treatment |
|---|---|---|
| `Transport`, `ConnectionClosed` | 502 | cluster card: unreachable, retry |
| `Timeout` | 504 | retry affordance |
| `Authentication` | 502 | **the cluster's** credentials failed — never rendered as the user's 401 |
| `Authorization` | 403 | "this cluster's principal lacks the ACL" |
| `Broker { code }` | 400 | render code *and* number |
| `Decode` | 500 | "this is a kaas-lib bug" + report link |
| `ReadOnly` | 500 | **our** bug — see PLAN.md §1 |
| `UnsupportedApi` | 501 | render *both* version ranges |
| `Unsupported`, `InvalidRequest` | 400 | query validation |

Two of these are load-bearing and easy to get wrong:

**`Authentication` is 502, not 401.** A 401 means *the person using kaas-ui* is
not logged in. A cluster whose SASL credentials were rejected is a server-side
configuration fault and must never log the user out.

**`ReadOnly` reaching a client is a bug, not a 405.** The gate is the second
line of defence. If it fires, kaas-ui built a request it should have been
incapable of building — there is no mutating endpoint in the router. Log it at
`error!` with a stack of context and return 500.

`UnsupportedApi` carries `broker: Option<(i16,i16)>` and `ours:
Option<(i16,i16)>`, and the pair is a diagnosis rather than a failure:

- `ours: None` → bump the codec, the key has no schema in this build
  (keys 88/89 on Strimzi today);
- `broker: None` → the cluster does not implement it at all
  (`DescribeCluster` on `kaas` today);
- both present but disjoint → the cluster is behind.

The UI renders all three differently. That is the whole point of the variant
carrying two ranges.

## Endpoints

Phase in brackets. `…` is `/api/environments/{env}`, elided so the table fits.

**Everything is addressed environment-first.** A cluster id alone addresses
nothing: two environments may each hold a `kafka`, and the lookup that decides
whether a caller may see either takes both halves. A schema registry is a
*peer* of a cluster inside its environment rather than a feature of one, so it
has its own subtree — its id is scoped, and an environment answers only to a
caller who can already see something in it, which is what keeps registry ids
off the enumerable surface without routing them through a cluster.

```
GET  /health                                                    [0] liveness, never blocks on a cluster
GET  /api/openapi.json                                          [0] the document describing everything below
GET  /api/environments                                          [0] the fleet: every environment, and everything in one
GET  /api/environments/{env}                                    [0] one of them, same shape as an element above
GET  /api/environments/{env}/clusters                           [0] one entry per cluster in it
GET  /api/environments/{env}/clusters/{id}                      [1] cluster detail
GET  …/clusters/{id}/capabilities                               [1] the capability projection
GET  …/clusters/{id}/brokers                                    [1]
GET  …/clusters/{id}/brokers/{node}/log-dirs                    [1]
GET  …/clusters/{id}/configs?resource=broker:1                  [1]

GET  …/clusters/{id}/topics                                     [2] list
GET  …/clusters/{id}/topics/{topic}                             [2] detail + partitions
GET  …/clusters/{id}/topics/{topic}/configs                     [2]
GET  …/clusters/{id}/topics/{topic}/offsets?spec=latest         [2]

GET  …/clusters/{id}/topics/{topic}/messages/tail?limit=500     [3] one shot, JSON
GET  …/clusters/{id}/topics/{topic}/messages?mode=…             [3] one bounded page
GET  …/clusters/{id}/topics/{topic}/messages/stream?mode=…      [3] text/event-stream
GET  …/clusters/{id}/topics/{topic}/messages/{part}/{offset}    [3] one record, whole

GET  /auth/login?connector=…                                    [4] optional, skips the chooser
GET  /auth/callback                                             [4]
POST /auth/logout                                               [4]
GET  /api/me                                                    [4]

GET  …/clusters/{id}/groups                                     [5]
GET  …/clusters/{id}/groups/{group}                             [5]
GET  …/clusters/{id}/groups/{group}/offsets                     [5] committed + lag

GET  /api/environments/{env}/schema-registries/{reg}/subjects    [6] the registry, and its subjects
GET  …/schema-registries/{reg}/subjects/{subject}/versions       [6] every version, with its text

GET  …/clusters/{id}/acls                                       [7]
GET  …/clusters/{id}/quotas                                     [7]
GET  …/clusters/{id}/scram-users                                [7]
GET  …/clusters/{id}/reassignments                              [7]
GET  …/clusters/{id}/transactions                               [7]
GET  …/clusters/{id}/transactions/{txn}                         [7]
GET  …/clusters/{id}/producers?topic=…&partition=…              [7]

GET  /api/search/topics?q=orders-*                              [8] across the fleet
GET  /api/compare?a={id}&b={id}                                 [8] capability + config diff
```

## Cluster visibility is a 404, not a 403

Enforced in the registry lookup, not in the router. A user without access to a
cluster gets `404 Not Found`, so cluster ids are not enumerable by probing. The
lookup function takes the caller's identity from Phase 4 onward; before Phase 4
it takes no identity and every cluster is visible.

This means there is exactly one `fn cluster(&self, id, who) -> Option<&ClusterHandle>`
and no handler ever indexes the registry map directly.

## SSE

`messages/stream` is the only streaming route. Events do **not** map 1:1 onto
`kafka_read::ScanEvent`, and the two places they differ are deliberate:

| SSE event | payload | frontend |
|---|---|---|
| `messages` | an array of rows, `id:` = `{partition}-{offset}` of the last | push to the ring buffer |
| `progress` | fraction, counters, reorder window | progress bar; "approximately ordered" caveat |
| `phase` | `seeking` \| `streaming` \| `done` | spinner, then the terminal row |
| `resolved` | what a time seek landed on, per partition | the "resolved to nothing" notice |
| `dropped` | a running count | a banner, never suppressed |
| `error` | a `ResourceError` | rendered with both version ranges intact |

**Records are batched, not one per event.** One event per record saturates the
connection and the browser's parser long before the list is the bottleneck.
Rows accumulate for 100 ms and leave together, so ten thousand records a second
is ten events a second.

**Malformed batches ride inside `messages`, not in `error`.** They are a *row
type* — `{"kind":"malformed", …}` alongside `{"kind":"record", …}` — because
the scan continued past them and the topic is fine either side. Folding them
into `error` throws away the one thing the reader needs, which is where in the
topic the damage is.

`error` carries the ordinary `ResourceError`, so an `UnsupportedApi` arrives
with both ranges and the three diagnoses above stay distinguishable.

Backward modes (`newest`, `toOffset`, `toTime`) have **no partial results**:
`kafka_read::tail` returns a `Vec`, so they emit `phase: seeking`, then the
whole window at once. A progress bar there would be a lie.

**Two headers exist for whatever is in front of the process**, and both are
load-bearing rather than defensive:

```
Cache-Control:      no-cache, no-transform
X-Accel-Buffering:  no
```

A proxy that buffers `text/event-stream` produces the worst failure mode
available — every layer reports success, the request stays open, and nothing
arrives. It was measured here: through code-server alone the stream delivered
4.4 KB in five seconds; through the Cloudflare tunnel in front of it, the
browser received nothing at all until these headers were set. `no-transform`
covers the other route to the same place, an edge that recompresses the body;
kaas-ui's own compression layer already declines SSE, but somebody else's does
not.

Backpressure is a bounded ring that **drops its oldest entry and counts it**
rather than awaiting. Awaiting would push back through the writer into the
fetch loop, so one slow browser would slow the scan for the cluster; and a live
tail whose reader has fallen behind wants the newest records, not a stale
prefix. On disconnect the response is dropped, the pump's `tx.closed()`
resolves, and the scan goes with it — kaas-lib is cancel-safe by construction,
so "user closed the tab mid-scan" is a non-event. Phase 3's acceptance asserts
on it by abandoning five streams and watching the slots come back.

SSE rather than WebSocket because the channel is permanently unidirectional —
there is no write path to send anything back, now or ever — and because it
survives corporate proxies and reconnects for free.

Backpressure is a bounded `mpsc`. On disconnect axum drops the stream, which
drops the scan; kaas-lib is cancel-safe by construction, so "user closed the tab
mid-scan" is a non-event rather than a leaked connection. With a dozen clusters
and many users that property is load-bearing, and Phase 3's acceptance asserts
on it.

**Two distinct malformed kinds, never conflated.** `ScanEvent::Malformed` is a
batch that would not decode at the protocol level. A payload that is not valid
Avro is an application-level failure on an otherwise fine record. Different
causes, different fixes, different rows — and in the wire shape that is a
`malformed` **row** for the first and a `record` row whose `value.note.kind` is
`decodeError` for the second. The record is fine; its value is not.

`?filter=` is a **literal substring of the decoded value**, matched after the
payload is decoded and capped at 256 characters — longer is a `400` rather than
a comparison the server repeats per record. It cannot go into the scan spec,
which kaas-lib matches against raw bytes, so `limit` is a budget for records
**read**: a page of three rows may have read five hundred records, and
`hasMore` reports whether the *read* filled its budget rather than whether the
page did. Paging off the row count instead would stop at the first window a
selective filter emptied.

## The schema browser is rooted at a registry

`/api/environments/{env}/schema-registries/{registry}`, which it was not: it
used to hang off `/api/clusters/{id}/schemas`, because a registry id on its own
would have been a second enumerable namespace beside cluster ids, and "which
clusters use this registry" is a list that can name a cluster the caller may
not see.

Nesting settles that without the indirection. A registry id is scoped to an
environment; an environment answers only to a caller who can already see a
cluster in it; and the registry lookup additionally requires that they can see
a cluster *referencing this registry*. So the URL names the thing it returns
instead of routing through a cluster that merely knows about it, and it still
cannot be probed — the two properties that used to be in tension.

Neither route requires a **connected** cluster, or any cluster at all on the
request path. A registry serves an environment and knows nothing about brokers,
so subjects stay browsable while every cluster beside it is down.

Three things that are not errors here, because each is an ordinary state:

- a registry that cannot be reached → `200`, the card carrying `unreachable`
  and its error, and an empty list;
- a registry answering the wrong API → `200`, the card carrying
  `misconfigured` and a message naming `/apis/ccompat/v7`;
- a subject the registry lists but will not hand over → `200`, the version
  absent and named in `errors`.

Every other absence is one `404` with one message: no such environment, no
registry of that id in it, and "not yours" are deliberately indistinguishable.
Telling them apart is precisely what a prober wants.

## Payloads say how they were read

`Payload` carries the codec that produced its text, and — where a registry was
involved — a `schema` naming the id, the format, the **registry id**, the
subject and the version. A schema id means nothing without the registry it is
an id in, which is why the registry is on the payload rather than implied by
the cluster.

`note` is why a payload is not what was asked for, and its `kind` is one of
`decodeError`, `registryUnavailable`, `registryAbsent`,
`registryMisconfigured`, `overrideRefused` or `nonConforming`. They are kept
apart because they want different things done about them: an outage heals on
its own, a `url` pointing at the wrong API does not.

`raw` is the original bytes as hex, present exactly when the text is a
*decoded* rendering. It is what makes `?keyCodec=hex` a browser-side render
rather than a refetch — and the reason the override is free downward and
refused upward, since nothing can invent a schema id for a payload that does
not carry one.
