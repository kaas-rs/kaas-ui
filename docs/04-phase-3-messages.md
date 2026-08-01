# Phase 3 — messages

*PLAN.md milestone M3.*

**Goal.** Tail first, then scan over SSE. Raw, string and JSON only — Avro and
Protobuf are Phase 6. This is the phase where kaas-ui stops being a metadata
browser.

It is also the phase that creates `crates/kaas-ui-serde`.

## Two endpoints, not one

```
GET /api/clusters/{id}/topics/{t}/messages/tail?limit=500       → JSON array
GET /api/clusters/{id}/topics/{t}/messages/scan?from=earliest…  → text/event-stream
```

**Tail is the default topic view.** One shot, bounded, and kaas-lib's backward
walk guarantees it reads a fraction of the partition — measured at ~325 KB to
reach the tail of a 40M-record topic. It is a plain JSON response and it goes in
the TanStack Query cache like everything else.

**Scan is for browsing and searching**, and it streams.

Building tail first is deliberate: it is the view people actually use, it needs
no streaming infrastructure, and it makes the Phase 3 acceptance measurable
before any SSE code exists.

### The `limit` question, settled

`TailSpec::limit` is a per-topic target spread across partitions with
`div_ceil`. On a 16-partition topic, `limit=20` returns 32 records — 2 per
partition.

kaas-ui's HTTP `limit` means **at most this many records, merged**. The handler
asks kaas-lib for the spread, merges the partitions by timestamp descending,
truncates to `limit`, and returns `partitionsSampled` alongside so the
truncation is visible. Anything else makes "show me the last 500" return 512 and
look like an off-by-a-lot.

## SSE

Events map 1:1 onto `ScanEvent`; the mapping table is in
[reference/http-contract.md](reference/http-contract.md).

Three properties to build in, not bolt on:

**Bounded channel.** A `tokio::sync::mpsc` with a fixed capacity between the
scan and the SSE writer. A slow client must slow the scan, not grow the heap.

**Streams do not go in the Query cache.** SSE feeds a capped ring buffer in a
dedicated hook with live/paused controls. TanStack Query is for
request/response; pushing a stream into it produces unbounded growth and
invalidation that nobody can reason about. This is PLAN.md §7's first
"thing to get right" and it is much harder to undo than to do.

**Cancellation is free, and must stay free.** On disconnect axum drops the
stream, which drops the scan future, which releases its buffers and closes or
completes its connections — kaas-lib is cancel-safe by construction (its rule
5). kaas-ui's job is not to introduce a `tokio::spawn` that outlives the
response and turns "user closed the tab" into a leaked scan.

That property is load-bearing with a dozen clusters and many users, and it is
the one thing in this phase most likely to be quietly broken by a refactor. The
acceptance test measures it.

## `crates/kaas-ui-serde`

kaas-lib hands over `Bytes`; everything above is ours.

Phase 3 scope is deliberately small: **raw, hex, UTF-8 string, JSON**. No
dependencies beyond `serde_json`. That covers most clusters, and it gets the
*shape* right — a `Deserializer` trait, a per-topic override, and a chip in the
UI showing what was chosen — before Phase 6 adds the hard formats behind the
same interface.

```rust
pub trait PayloadCodec {
    fn name(&self) -> &'static str;
    fn decode(&self, bytes: &Bytes, ctx: &TopicContext) -> Decoded;
}
```

Sniff order: explicit per-topic config → JSON if it parses → UTF-8 if it is
valid → hex. **Always show what was chosen and let the user override it.**
Auto-detection that cannot be corrected is worse than none.

`kaas-ui-serde` does not know about axum, and it may not panic. It is the crate
where a hostile payload arrives, and the workspace lint list exists for it.

## Filtering

Cheap predicates go through kaas-lib's `RecordFilter` — offset, timestamp,
partition, key prefix, headers — and run **before** deserialization. Phase 6's
JS predicate runs after. Never run the expensive one on a record the cheap one
could have dropped; the ordering is a correctness property of the design, not an
optimisation.

## Traps

- **Two malformed kinds, never conflated.** `ScanEvent::Malformed` is a batch
  that would not decode at the protocol level — render offsets and hex, keep
  going. A payload that is not valid JSON is an application-level failure on an
  otherwise fine record. Different causes, different fixes, different rows.
- **A truncated trailing batch is normal.** kaas-lib already discards it
  silently; do not add a UI that reports it.
- **Tombstones are `value: None`, not empty.** `Record::is_tombstone()`. An
  empty-string render loses the distinction that compaction turns on.
- **Timestamps have a type.** `TimestampType::{CreateTime, LogAppendTime}`.
  Show which; they answer different questions.
- **Headers are `Vec<(String, Option<Bytes>)>`** — ordered, duplicable, and the
  value is nullable. Not a map.
- **The message URL is the shareable artifact.** Topic, partitions, start
  position, filter and codec all live in zod-validated search params. This is
  the main thing people send each other from a Kafka UI.

## Acceptance

```sh
cargo xtask live --config config.dev.yaml
```

Against `kperf-bench` on both clusters — 16 partitions, ~40M records on `kaas`,
~45M on `strimzi`:

- **tail of 500 fetches under 5% of the partition's bytes**, asserted on
  kaas-lib's `ConnectionStats::since()` before and after, not estimated. The
  measured baseline for `limit=20` is ~325 KB and 16 fetches;
- the endpoint returns **exactly 500 records**, merged and truncated, with
  `partitionsSampled: 16` in the response;
- a scan from `earliest` on a 40M-record topic streams `progress` events with a
  moving `fraction()` and does not grow the server's RSS beyond the bounded
  channel;
- **closing the tab mid-scan returns connection count and RSS to baseline**
  within 5s — measured via `BrokerPool::live_connections()` and the process's
  own RSS, both before and after;
- a hand-corrupted batch renders as a malformed row **while the scan continues**
  — injected in a unit test over `decode_records`, since neither live cluster
  will corrupt a batch on request;
- the same tail works on `kaas` (Fetch v12, name-based) and `strimzi` (Fetch
  v18, topic-id-based) with no branch in kaas-ui;
- a JSON payload renders as JSON with the codec chip reading `json`, and
  overriding to `hex` re-renders without a refetch.

## Exit criteria

- [ ] tail under 5% of partition bytes, asserted on connection counters
- [ ] `limit` means what the API says it means
- [ ] scan streams, is bounded, and dies with its client
- [ ] no `tokio::spawn` that outlives a response
- [ ] malformed batch and malformed payload are visibly different rows
- [ ] message view URL fully round-trips through search params
- [ ] `kaas-ui-serde` has no axum dependency and no panic path
