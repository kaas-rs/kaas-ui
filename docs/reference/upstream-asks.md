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
| 5 | SASL OAUTHBEARER | — | the likely hard blocker |
| 6 | `DescribeQuorum` (51) | Phase 7 | nice to have |
| 7 | pool and connection introspection | Phase 1/3 | medium |
| 8 | `ListConfigResources` (70) | Phase 1 | low |
| 9 | upstream `kafka-protocol` contributions | Phase 5 | low, long lead |
| 10 | `topic_offset_range` will not compile in an axum handler | Phase 2 | **worked around** |
| 11 | streaming reads share a connection with everything else | Phase 3 | **high — a live view degrades the whole cluster's UI** |

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

## 5. SASL OAUTHBEARER

**The largest item and the one most likely to be a hard blocker for a real
user.** Strimzi with Keycloak is a mainstream deployment and cannot connect
today. Also unlocks Confluent Cloud OAuth and, with a token provider hook, MSK
IAM later.

Token acquisition, refresh, and the interaction with KIP-368 re-authentication
are all in scope, which is why it is large.

Not urgent for *this* deployment: both dev clusters offer plaintext and TLS
listeners, and `kaas` additionally has a SASL listener on 9095 with
SCRAM-SHA-512 users, all of which kaas-lib already speaks. It becomes urgent the
moment someone points kaas-ui at a Keycloak-fronted Strimzi.

Note the symmetry if it lands: Dex terminates OIDC for *users*, OAUTHBEARER
terminates OAuth for *clusters*, and the same Keycloak can serve both.

## 6. `DescribeQuorum` (51)

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

## Not needed

Producer, group membership, incremental fetch sessions. A read-only kaas-ui
never wants any of them.

This is worth stating positively rather than as an omission: **kaas-lib having
no producer stops being a gap to work around and becomes alignment with the
product.**
