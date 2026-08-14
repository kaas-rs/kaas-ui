# The kaas-lib surface, mapped

Which library call backs which screen, and the four or five places the shape of
the library decides the shape of the UI. Read alongside kaas-lib's own
[API support matrix](https://kaas-rs.github.io/kaas-lib/compat/api-matrix.html).

## The crates

| crate | what kaas-ui uses it for |
|---|---|
| `kafka-conn` | `Error`, `ErrorCode`, `ApiKey`, `ApiVersions` — the error mapping and the capability projection |
| `kafka-meta` | `Cluster`, `ClusterConfig`, `MetadataSnapshot`, `BrokerPool` — the registry, the fleet view |
| `kafka-admin` | `Admin` and 31 read RPCs — everything else |
| `kafka-read` | `scan`, `tail` — the message views |

`kafka-admin` re-exports `Cluster`, `ClusterConfig`, `TopicInfo`, `ApiKey`,
`Error`, `ErrorCode`, `Result`, so most of `kaas-ui-core` needs one import.

## Connecting

```rust
use kafka_admin::{Admin, ClusterConfig};

let admin = Admin::connect_read_only(bootstrap, config).await?;
```

`connect_read_only` sets `ConnectionConfig::read_only`, and the gate is enforced
in `Connection::send` on `ApiKey::is_mutating` — not over the admin method
surface, so an admin method added upstream tomorrow is covered without anyone
remembering to cover it. **This is the security property. kaas-ui's only job is
not to undermine it.**

`ClusterConfig` carries `connection` (`ConnectionConfig`), `retry`,
`refresh_interval` (default 30s) and `max_staleness` (default 300s).
`ConnectionConfig` carries `client_id`, timeouts, `max_in_flight`,
`max_frame_bytes`, `tls: Option<Arc<TlsConfig>>` and `sasl:
Option<Arc<SaslConfig>>`.

Set `client_id` per cluster to something recognisable — it shows up in broker
request logs and quota attribution, and on a shared cluster that is how someone
else works out who is generating load.

TLS is `TlsConfig::system()` or `TlsConfig::with_ca_pem(...)`, plus
`with_client_certificate(chain_pem, key_pem)` and `with_server_name(...)`. The
SNI override is not optional in practice: the address dialled through a
Kubernetes Service is routinely not the name on the certificate.

## Screens to calls

| screen | call | returns |
|---|---|---|
| fleet card | `cluster.snapshot()` | `MetadataSnapshot` — brokers, topics, controller, cluster id, `age()` |
| cluster detail | `admin.describe_cluster()` | `ClusterDescription` — **absent on `kaas`** |
| broker log dirs | `admin.describe_log_dirs(node)` / `describe_all_log_dirs()` | `Vec<LogDir>` / `PerItem<i32, Vec<LogDir>>` |
| topic sizes | `admin.topic_sizes()` | `PerItem<String, TopicSize>` — logical and replicated bytes |
| topic list | `admin.list_topics()` | `Vec<String>` |
| topic detail | `admin.describe_topics([...])` | `PerItem<String, TopicInfo>` |
| configs | `admin.describe_configs([...])` / `_documented` | `PerItem<ConfigResource, Vec<ConfigEntry>>` |
| offsets | `admin.list_offsets(...)`, `admin.topic_offset_range(t)` | `PerItem<(String,i32), ListedOffset>`, `PerItem<i32, (Option<i64>, Option<i64>)>` |
| tail view | `kafka_read::tail(cluster, &TailSpec)` | `Vec<PartitionTail>` |
| scan view | `kafka_read::scan(cluster, ScanSpec)` | `Stream<Item = Result<ScanEvent>>` |
| group list | `admin.list_groups()` / `_filtered` | `Vec<GroupListing>` |
| group detail | `admin.describe_groups([...])` | `PerItem<String, GroupDescription>` |
| committed offsets | `admin.fetch_offsets(...)` | `PerItem<_, CommittedOffset>` |
| ACL viewer | `admin.describe_acls(&AclFilter)` | `Vec<AclBinding>` |
| quotas | `admin.describe_client_quotas(...)` | quota entities |
| SCRAM users | `admin.describe_scram_credentials(...)` | `ScramCredentialInfo` |
| reassignments | `admin.list_partition_reassignments(...)` | `OngoingReassignment` |
| transactions | `admin.list_transactions()`, `describe_transactions([...])`, `describe_producers(...)` | listings, `TransactionDescription`, `ProducerState` |

Everything mutating — `create_topics`, `delete_topics`, `alter_configs`,
`create_acls`, `reset_offsets`, `delete_groups`, `elect_leaders`,
`alter_partition_reassignments`, `upsert_scram_credentials`, … — exists on
`Admin` and is **never called**. The gate refuses them before a socket is
touched; kaas-ui simply has no route that reaches one.

## The five shapes that decide the UI

### 1. `PerItem` is `Vec<(K, Result<T, Error>)>`

```rust
pub type PerItem<K, T> = Vec<(K, Result<T, Error>)>;
pub fn oks<K, T>(items: &PerItem<K, T>) -> impl Iterator<Item = (&K, &T)>;
pub fn errs<K, T>(items: &PerItem<K, T>) -> impl Iterator<Item = (&K, &Error)>;
```

Describing 50 topics of which 2 do not exist returns 48 successes and 2 errors.
The HTTP envelope preserves that end to end. `oks` and `errs` are the split, and
the `Envelope::from_per_item` helper is the only place that split happens.

### 2. `MetadataSnapshot` is immutable and knows its age

```rust
snapshot.brokers()      -> &[BrokerInfo]     // node_id, host, port, rack
snapshot.topics()       -> &[TopicInfo]      // name, topic_id, internal, partitions, error
snapshot.controller_id()-> Option<i32>
snapshot.cluster_id()   -> Option<&str>
snapshot.age()          -> Duration
partition.under_replicated() -> bool
```

Behind an `ArcSwap`: readers never block, never wait on a refresh in progress,
and never see a half-updated cluster. `age()` is what makes "as of 4 seconds
ago" honest, and across twelve clusters that matters more than across one.

Every field the fleet view needs is here, on both target clusters. Phase 0 uses
nothing else.

### 3. The version table is **per connection**

```rust
let conn = admin.cluster().pool().any().await?;
for e in conn.versions().entries() {
    e.api_key;            // ApiKey, possibly Unknown(i16)
    e.broker;             // VersionRange the broker advertises
    e.ours;               // Option<VersionRange> — None = no schema in this build
    e.negotiated();       // Option<i16>
    e.broker_ahead();     // broker.max > ours.max
}
```

Deliberately per connection: brokers mid-rolling-upgrade genuinely disagree, and
a cluster-wide table would be wrong during exactly the window when being right
matters.

**Consequence for kaas-ui: there is no `cluster.capabilities()` to project
from, and kaas-ui must not invent one by picking a broker.** Doing so produces a
UI whose tabs flicker depending on which connection answered. See
[what is built](../11-built.md) for the interim rule and
[upstream-asks.md](upstream-asks.md) item 1 for the fix.

### 4. `Error` is a taxonomy, and `ErrorCode` carries `Unknown(i16)`

`Error::{Transport, ConnectionClosed, Timeout, Authentication, Authorization,
Broker, Decode, ReadOnly, UnsupportedApi, Unsupported, InvalidRequest}`, with
`retriable()`, `needs_metadata_refresh()`, `code()`.

`Decode` means **kaas-lib is wrong** — a version negotiated badly or a schema
drifted. Report it, never retry it; `retriable()` already returns false.

Mapping to HTTP is in [http-contract.md](http-contract.md).

### 5. Groups are four kinds, not one struct with optional fields

```rust
pub enum GroupDescription {
    Classic { group_id, state, protocol_type, protocol, members: Vec<ClassicGroupMember> },
    Consumer { group_id, state, group_epoch, assignment_epoch, assignor, members: Vec<ConsumerGroupMember> },
    Share { group_id, state, group_epoch, assignment_epoch, assignor, members: Vec<ShareGroupMember> },
    Unrecognized { group_id, group_type, state },
}
```

`Unrecognized` is a **successful** description of an undescribable group — the
group exists, it is listed, the UI can say what it is. Streams groups
(KIP-1071) land here because `kafka-protocol` 0.17 has no schema for api keys 88
and 89. Mirror this with `#[serde(tag = "kind")]` and render four components;
flattening to one all-optional interface moves the knowledge somewhere the
compiler cannot check it.

Note `GroupListing::describable()` accepts `""`, `"classic"`, `"consumer"` and
`"share"`. The `kaas` cluster reports `group_type: ""` for every group — brokers
too old to report a type — so it takes the classic path.

## The read path

```rust
let spec = TailSpec::new("orders", 500).partitions([0, 1]).filter(f);
let tails: Vec<PartitionTail> = kafka_read::tail(cluster, &spec).await?;

let spec = ScanSpec::new("orders").from(StartPosition::Earliest).limit(10_000);
let mut stream = Box::pin(kafka_read::scan(cluster, spec).await?);
```

`ScanEvent` is
`PartitionStarted | Record | Malformed | Progress | PartitionComplete | Done`,
and every one maps to a row or a control in the UI. `ScanProgress` carries
`records_emitted`, `records_scanned`, `malformed_batches`, `offsets_consumed`,
`offsets_total`, `partitions_active`, `partitions_planned`, `reorder_window`
and `fraction()`.

`reorder_window` is kaas-lib's own sizing as of 0.9 — the buffer ceiling over
the merge's real width, and `0` exactly when cross-partition timestamp order
held. kaas-ui reconstructed it from the plan until then, and the flag that
came with it had to be suppressed on the single-partition case that raised it
without any reorder to warn about.

`RecordFilter` matches raw bytes, which is why kaas-ui does not use it: its
payload filter is a substring of the *decoded* value and cannot be expressed
against bytes that are still Avro. What does go in the spec is the selection
that needs no payload at all — partitions, the start position, the limit — and
that still bounds what the broker sends.

Two measured behaviours to design against:

- **`limit` is a topic-wide target, and still over-fetches.** It was divided
  across every partition with `div_ceil` before 0.9 — `TailSpec::new(t, 20)`
  on a 16-partition topic returned 32 records, 2 each, and a topic with idle
  partitions returned a *fraction* of what was asked for. 0.9 measures the
  bounds first, drops partitions holding nothing from the divisor and hands an
  exhausted walk's share to one that can still yield; what remains is that a
  partition's last chunk is kept whole, so the count comes back at or above
  the limit and the HTTP layer truncates after merging. `PartitionTail` also
  reports `reached_log_start`, which is where `hasMore` comes from.
- **The backward walk really is cheap.** ~325 KB of network to tail a 40M-record
  topic, 16 fetches for 16 partitions. `Connection::stats_snapshot()` and
  `StatsSnapshot::since()` are how Phase 3 asserts that.

`decode_records` and `DecodeOptions` are available if kaas-ui ever needs to
decode a batch itself. It should not.

## What kaas-lib does not have, and will not for now

No producer, no consumer-group membership, no incremental fetch sessions. A
read-only kaas-ui wants none of them — PLAN.md §1's observation that "the
produce question disappears" is the single biggest simplification in the
project.
