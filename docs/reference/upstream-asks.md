# Proposed kaas-lib work

PLAN.md §9 ordered these by value-to-effort against the library as it stood.
This version sequences them against the **phases that need them** and adds one
item that only appeared once the library was pointed at a real `kaas` broker.

All read-side. Nothing here needs a producer, so kaas-lib's own roadmap can be
sequenced entirely on its own merits.

| # | item | blocks | urgency |
|---|---|---|---|
| 0 | `describe_cluster` falls back to `Metadata` | Phase 0/1 | **worked around** |
| 1 | cluster-level capability aggregation | Phase 1 | **high — correctness** |
| 2 | batched `FindCoordinator` (KIP-699) | Phase 5 | high at scale |
| 3 | multi-group `OffsetFetch` | Phase 5 | pairs with 2 |
| 4 | `DescribeTopicPartitions` cursor pagination | Phase 2 | medium |
| 5 | SASL OAUTHBEARER | — | **landed in 0.6.1, in use** |
| 6 | `DescribeQuorum` (55) | Phase 7 | nice to have |
| 7 | pool and connection introspection | Phase 1/3 | medium |
| 8 | `ListConfigResources` (70) | Phase 1 | low |
| 9 | upstream `kafka-protocol` contributions | Phase 5 | low, long lead |
| 10 | `topic_offset_range` will not compile in an axum handler | Phase 2 | **worked around** |
| 11 | streaming reads share a connection with everything else | Phase 3 | **high — a live view degrades the whole cluster's UI** |
| 12 | `ScanProgress::fraction` ignores `spec.limit` | Phase 3 | **worked around** |

---

## 0. `describe_cluster` should fall back to `Metadata`

**New — found by running, not reading.**

```
kaas → describe_cluster: no usable version of DescribeCluster:
       broker offers None, we speak Some((0, 2))
```

`kaas` does not implement api key 60. `ClusterDescription` is
`{ cluster_id, controller_id, brokers: [{ node_id, host, port, rack, is_fenced }] }`,
and `Metadata` supplies **every field except `is_fenced`**.

kaas-lib already does exactly this shape of fallback for `describe_topics`
(`DescribeTopicPartitions` → `Metadata`) and PLAN.md §2 is explicit that
absorbing implementation differences is the library's job, not the UI's. So the
fallback belongs there, with `is_fenced` becoming `Option<bool>` — `None`
meaning "not reported by this path", which is honest, rather than `false`, which
is a guess.

**Worked around** in kaas-ui by rendering the fleet from
`Cluster::snapshot()` alone (see [what is built](../11-built.md)), so this
is not blocking. It is filed because the workaround is a UI making a
compatibility decision, which is the thing the ownership boundary exists to
prevent.

## 1. Cluster-level capability aggregation

**The highest-value item, and the only one that is a correctness problem rather
than a performance or coverage one.**

The per-connection version table is correct and should stay that way. But a UI
needs a cluster-level answer and must not fabricate one by picking a broker. The
right shape preserves disagreement rather than collapsing it:

```rust
// on Cluster — needs pool access, so it cannot live in kaas-ui
pub enum ApiSupport {
    Agreed(Option<VersionRange>),
    Disagreed(BTreeMap<BrokerId, Option<VersionRange>>),  // rolling upgrade
}
pub fn capabilities(&self) -> BTreeMap<ApiKey, ApiSupport>;
```

`Disagreed` is the interesting variant. Rendering "3 of 5 brokers support this,
upgrade in progress" is honest, useful, and invisible to every other Kafka tool.

This is item 1 because capabilities drive the entire UI and there is currently no
correct way to compute them. Until it lands, kaas-ui reads the table from an
explicitly named broker and **says so in the UI** — the `source` field in the
capabilities response.

The failure mode if this is skipped is nasty: a capability table computed from
one arbitrary broker looks perfect on every single-broker fixture and misbehaves
on the first rolling upgrade. Both dev clusters are three-node, so kaas-ui can
at least *detect* disagreement by polling more than one connection — but
detecting it is not the same as modelling it.

## 2. Batched `FindCoordinator` (KIP-699)

kaas-lib's own docs flag it: "worth revisiting for a UI rendering hundreds of
groups at once."

A groups page on a cluster with 300 groups costs **300 round trips** on a cold
cache. Biggest UI-visible performance item, and a contained change to
`kafka-meta`.

## 3. Multi-group `OffsetFetch`

v8+ takes several groups per request. With (2), the groups list goes from O(n)
round trips to roughly O(1). Same page, same motivation, and the two should land
together — either alone leaves the other as the bottleneck.

Note `OffsetFetch` is one of the version-shaped requests: the response schema
reaches v10 while the request stops at v9, and the `groups` field only exists
from v8. kaas-lib already handles this via `negotiated_for::<R>()`; the batching
change must not regress it.

## 4. `DescribeTopicPartitions` cursor pagination

A cluster with thousands of topics should not need one enormous `Metadata`
response to render page one. Check whether the cursor is already exposed; if
not, expose it.

Note this only helps on clusters that implement api key 75 — `kaas` does not, so
the `Metadata` path stays the fallback and stays unpaginated.

## 5. SASL OAUTHBEARER — **landed**

Called the largest item here and the one most likely to be a hard blocker.
kaas-lib 0.6.1 shipped it, and kaas-ui speaks it as of 0.10.3: `SaslConfig::oauth_bearer`
takes a `TokenProvider`, and the `oidc` feature brings `OidcTokenProvider`, a
`client_credentials` exchange that caches the token, refreshes it ahead of
expiry and single-flights the refresh across connections.

Everything the ask asked for is in it. Token acquisition, refresh and the
KIP-368 interaction are all handled below kaas-ui: the provider is asked again
on every re-authentication, which is a timer kaas-lib owns, so the thing that
had to be got right — a token captured once and presented hours later — cannot
be got wrong from up here.

What kaas-ui had to add was configuration and one lifetime decision: the
provider is built **once per cluster handle**, not per connect, because that is
what keeps every connection to a cluster on one token and one fetch. The rest
is a config block — see `sasl: {mechanism: oauthbearer, …}` in
[environment.md](environment.md).

Two things are deliberately still absent, and neither blocks anything:

- **A pre-fetched token.** `SaslConfig::oauth_bearer_token` exists and is right
  for a CLI; kaas-ui runs for weeks, so the fetching provider is the only mode
  worth carrying. A `token_file:` variant is a small addition if a sidecar ever
  wants to own refresh.
- **A private CA for the *issuer*.** `OidcConfig::with_tls` is there for a
  Keycloak behind an internal CA. Entra is public, so the system trust store
  is right today, and a second TLS block nobody needs is surface nobody
  should have to read.

The symmetry the original note predicted holds: Dex terminates OIDC for
*users*, OAUTHBEARER terminates OAuth for *clusters*, and one issuer can serve
both.

## 6. `DescribeQuorum` (55)

kaas-lib's api-matrix already calls it "the one in this group a UI might
plausibly want". A KRaft quorum panel — voters, leader, lag — is directly useful
for both `kaas` and Strimzi KRaft clusters.

Strimzi advertises it here; `kaas` does not. So the panel would render on one of
the two dev clusters, which is the normal situation for this project.

## 7. Pool and connection introspection

kaas-ui wants "connected to 2 of 3 brokers" plus per-broker connection age and
byte counters. `ConnectionStats` and `StatsSnapshot::since()` exist and are
already load-bearing for Phase 3's acceptance test — but they are per
`Connection`, reachable only via `pool().any()`, so there is no way to enumerate
the pool.

`BrokerPool` exposes `live_connections()` and `live_node_connections()` as
counts. What is missing is the per-broker breakdown.

Pairs naturally with (1), since both need the same pool access.

## 8. `ListConfigResources` (70)

Enumerate config resources rather than guessing at what exists. Advertised by
Strimzi, absent on `kaas`.

## 9. Upstream contributions to `kafka-protocol`

`StreamsGroupDescribe` (KIP-1071) and `ListOffsets` v11 are blocked on schemas
rather than on kaas-lib. Api keys **88 and 89 are advertised by the Strimzi
cluster here and cannot be named by this build** — the gap is live, not
theoretical.

Contributing upstream is the option kaas-lib's own docs rank above every
workaround, and streams groups appear on any Strimzi cluster running Kafka
Streams. Long lead time, so worth starting early even though nothing blocks on
it: `GroupDescription::Unrecognized` degrades correctly in the meantime, which is
the whole reason that variant exists.

## 10. `topic_offset_range` is unusable from an axum handler

Found while building Phase 2, and it is a compiler-level blocker rather than a
missing feature.

`Admin::topic_offset_range(&self, topic: &str)` builds its partition list with
`partitions.iter().map(|p| (topic.to_owned(), *p))` — a closure of shape
`fn(&i32) -> (String, i32)` held across an `await`. Awaiting that future inside
an axum handler fails to compile:

```
error: implementation of `FnOnce` is not general enough
  --> crates/kaas-ui-api/src/lib.rs
   | .route("/clusters/{id}/topics/{topic}", get(topics::detail))
   = note: closure with signature `fn(&'0 i32) -> (String, i32)` must implement
           `FnOnce<(&'1 i32,)>`, for any two lifetimes `'0` and `'1`...
```

The error surfaces at the `get(handler)` call site, names none of the caller's
code, and `Box::pin` does not fix it — which makes it expensive to diagnose
from the consumer side.

**Workaround in kaas-ui today:** build the `Vec<(String, i32)>` *before* the
first await and call `list_offsets` twice, which is what the topic detail and
group offset handlers do. That is also the cheaper call — `topic_offset_range`
refreshes metadata first, which a list view must not do per row.

**The fix upstream** is to collect the partition list into an owned `Vec`
before the await rather than mapping lazily across it. One line, and it makes
the helper usable from the only kind of caller a UI has.

## 11. Streaming reads need a connection they do not share

**Found by running, and it is the sharpest performance problem in the project
so far.** A live message view makes every other view of that cluster slow.

`BrokerPool` keeps **one connection per broker**, shared by every caller, and
Kafka answers a connection's requests **in order**. A live tail long-polls with
`ScanSpec::max_wait_ms`, so its `Fetch` sits at the head of that queue and
delays every `ListOffsets`, `Metadata` and `DescribeTopics` behind it. The
delays add up per open stream.

Measured against the Strimzi cluster, on a call that is otherwise 2 ms:

```
no streams open                     2 ms
6 live streams, max_wait_ms = 500   3000-4000 ms
6 live streams, max_wait_ms = 100    940-1200 ms
a process with ~18 abandoned polls  7500-9000 ms
```

`max_in_flight` does not help — its own documentation says so: *"the broker
processes a connection's requests in order either way, so this is about
pipelining, not parallelism."*

kaas-ui has mitigated what it can by lowering `max_wait_ms` for live scans,
which divides the constant and does not remove it. The ceiling makes that
plain: the stream governor allows 50 concurrent streams, and at 100 ms each
that is still five seconds of queueing for an unrelated request.

**The fix belongs downstairs**, because it is about how the pool is shaped:

```rust
// on BrokerPool — a caller that intends to hold a request open for a long
// time should be able to say so and get a connection nobody else waits behind.
pub async fn get_dedicated(&self, node_id: i32) -> Result<Connection>;
```

Or, equivalently, `scan` opening its own connections when following. Either
way the property wanted is that **a long poll never shares a socket with a
request that expects to be fast**.

This also interacts with the UI's own ceilings: until it lands, "how many live
views may be open at once" is a question about someone else's latency rather
than about memory, which is not a trade-off the stream governor can reason
about.

---

## 12. `ScanProgress::fraction` ignores the limit it was given

**Found by running.** The one that shipped a wrong number to a user rather than
a slow one.

`fraction()` divides `offsets_consumed` by `offsets_total`, and `plan()` sets
`offsets_total` from `end_offset = latest` — the whole retained span of every
partition in the scan. `spec.limit` never enters the calculation, but it is
what actually ends the scan. So every limited scan reports a fraction against
a finish line it was never going to reach:

```
kaas-canary-v1 on the kaas cluster, ScanSpec::new(topic).limit(500)

  offsets_total    = 9181       the whole topic
  records emitted  = 500        where the scan actually stopped
  fraction()       = 0.0545     and stays there, complete
```

Rendered as a progress bar this parks at 5%, and on a topic being produced to
it *sinks* over time as the denominator grows — which is how it was noticed.

The scan ends at whichever finish line arrives first, so the fraction should
be the nearer of the two:

```rust
// on ScanProgress, which would need the limit to be carried onto it
let by_span  = offsets_consumed / offsets_total;
let by_limit = records_emitted / limit;
Some(by_span.max(by_limit))
```

Both terms are needed. An unfiltered read reaches its limit long before the
end of the topic; a `RecordFilter` that drops most of what it reads runs out of
topic while its emitted count crawls. Taking the larger tracks whichever end
the scan is heading for, and both reach `1.0` exactly when the scan stops.

**kaas-ui computes this itself** in `routes/messages/stream.rs::fraction`,
because it has both numbers at the call site and the bar was visibly wrong. It
is written here because the wrongness is not kaas-ui's: any consumer that sets
`ScanSpec::limit` and reads `fraction()` gets the same bad number, and the fix
belongs where the limit and the span are both already known.

Related, and much smaller: `progress_every` is hardcoded to `1_000` decoded
records and is not reachable from `ScanSpec`. A window smaller than that emits
no `Progress` event at all — only the `Done` one — so kaas-ui's default
500-record window has a bar with exactly one frame. It reads in ~40 ms, so
nothing is visibly wrong today, but the cadence should be the caller's to pick.
The other end of the same hardcoding: on megabyte records a thousand of them
is a long silence, and on a firehose it is several events a millisecond. The
analysis route throttles to one frame a second on its own side, which fixes
the flood and cannot fix the silence — a record count is simply the wrong unit
for a progress bar, and elapsed time is the right one.

---

## 13. A partition-level failure must not fail the whole scan

**The ask the statistics tab is most exposed to.** `scan.rs` — any error out
of `refill()` latches `done` and yields `Some(Err(error))`, ending the stream:

```rust
Err(error) => {
    self.done = true;
    return Some(Err(error));
}
```

One partition's leader election, one `NotLeaderOrFollower`, one transient
disconnect discards an entire analysis. On a large topic that is potentially
an hour of reading thrown away because one partition of sixteen hiccuped at
95%, and the retry starts from `Earliest` again.

This is kaas-ui's rule 5 — *partial failure is a result* — pushed down one
level, exactly as rule 4 is kaas-lib's rule 1 pushed up one:

- a `ScanEvent::PartitionFailed { partition, error }` variant, after which the
  scan **continues with its remaining partitions**
- `Done` reporting which partitions failed, so a caller can label the result
- `Err` on the stream reserved for what it genuinely means today: the scan
  could not be *planned* — topic gone, no metadata, no leader for anything

Every consumer benefits, but the analysis is the one that structurally needs
it: it is the longest-running read in the product, so it is the most exposed
to a transient fault, and it is the only one whose output is an aggregate a
missing partition silently corrupts. A browse that lost a partition still
shows records; a `totalMsgs` that quietly omits partition 7 is *wrong*, not
partial. kaas-ui ships around it today by flagging the whole result
`complete: false` with the error named — honest, and strictly worse than 15
good partition rows and one error row, which is what the rest of the API
already does with `PerItem`.

---

## 14. `ScanProgress::fraction` saturates at `u32`

`scan.rs`:

```rust
let consumed = u32::try_from(self.offsets_consumed.max(0)).unwrap_or(u32::MAX);
let total = u32::try_from(self.offsets_total.max(1)).unwrap_or(u32::MAX);
Some((f64::from(consumed) / f64::from(total)).clamp(0.0, 1.0))
```

Past ~4.29e9 offsets `total` saturates while `consumed` is still real, so the
fraction **overstates** progress; once `consumed` saturates too it pins at
`1.0` for the rest of the scan. Both are offset ranges rather than record
counts, so a long-retention topic gets there well before it holds that many
records.

The `u32` is clearly there to keep `f64::from` lossless under the workspace
lints, which is the right instinct — the fix is arithmetic that stays exact
without the ceiling: scale the ratio in `i64`/`i128` before converting, or
convert with one explicit, commented allow. Today it distorts a progress bar;
with ask 13's `Done` accounting it would distort a number people read.

---

## 15. Surface skipped control batches and aborted records

`batch.rs` already counts `control_batches_skipped` and
`aborted_records_skipped` on the decode result, and `scan.rs` reads them —
but only to decide whether to advance the cursor. They never reach
`ScanProgress`, so they are computed and thrown away.

For the analysis they are the explanation of an otherwise alarming number. On
a transactional topic `totalMsgs` is legitimately far below the offset range,
and without these counters the UI cannot tell the reader whether that gap is
compaction, transaction markers, or aborted records — three very different
facts about their topic. Two `u64`s on `ScanProgress` turn "your numbers don't
add up" into an explanation.

---

## Not needed

Producer, group membership, incremental fetch sessions. A read-only kaas-ui
never wants any of them.

This is worth stating positively rather than as an omission: **kaas-lib having
no producer stops being a gap to work around and becomes alignment with the
product.**

Also not asked for, so it is not proposed again: **a payload-free "stats only"
scan** for the analysis. `RecordBatchDecoder` decodes a whole batch into a
`Vec<Record>` — decoding is batch-level — and kaas-lib explicitly refuses to
vendor the record loop rather than keep a second implementation of a wire
schema in step with upstream. A payload-free path would need exactly that. And
the cost it would avoid is smaller than it looks: `Record::key`/`value` are
`Bytes`, refcounted slices of the fetch buffer rather than copies, and
`payload_len()` is already key-plus-value for free. The remaining per-record
cost is the `topic: String` clone and the headers `Vec`, which is not worth a
second decoder.
