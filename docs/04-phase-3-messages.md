# Phase 3 — messages

*PLAN.md milestone M3.*

**Goal.** Tail first, then scan over SSE. Raw, string and JSON only — Avro and
Protobuf are Phase 6. This is the phase where kaas-ui stops being a metadata
browser.

> **Built, and it grew.** The plan below is what was designed; the routes are
> four rather than two, the SSE events do not map 1:1 onto `ScanEvent`, and
> `crates/kaas-ui-serde` was not created. Everything that changed and why is in
> [Decisions this phase changed](#decisions-this-phase-changed) at the foot of
> this file — read that alongside the reasoning here rather than instead of it.

## Two endpoints, not one

*Four, as built. The two below are the design; `messages` and
`messages/{partition}/{offset}` were added for reasons the plan did not
anticipate, and the streaming one is `messages/stream`.*

```
GET /api/clusters/{id}/topics/{t}/messages/tail?limit=500        → JSON array
GET /api/clusters/{id}/topics/{t}/messages/stream?mode=live…     → text/event-stream
GET /api/clusters/{id}/topics/{t}/messages?mode=newest&limit=…   → one bounded page
GET /api/clusters/{id}/topics/{t}/messages/{partition}/{offset}  → one record, whole
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

*As built they do **not** map 1:1: records are batched on a 100 ms interval, and
a malformed batch is a row inside `messages` rather than an event of its own.
The table in [reference/http-contract.md](reference/http-contract.md) is the
contract; the reasoning for both is below.*

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

*Not created. Payload rendering is `Payload::of` in `kaas-ui-core::dto`, which
is this section minus the JSON step and minus the per-topic override — see the
decisions below. The design here stands for Phase 6, which is where the crate
earns its boundary.*

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

50 assertions against `kaas`, `strimzi` and a deliberately dead third cluster.
The message ones, and what each is actually guarding:

- **`toOffset` includes its anchor and nothing above it**, and `fromOffset`
  starts at the offset asked for rather than at the base of the batch
  containing it. Both are off-by-one traps that leave a plausible-looking
  window, which is why they are asserted on both ends;
- **a single record is fetched by partition and offset**, and an offset past
  the end is `404` rather than the last record — `scan` clamps a start position
  into the log, so the wrong answer here is a payload from a different row;
- **a time seek reports what it resolved to.** `kaas` resolves to nothing and
  says so; Strimzi resolves precisely. Both are correct, and a window that is
  empty for the first reason must not look like one that is empty for the
  second;
- **the stream is an uncompressed `text/event-stream`**, and a backward window
  goes `seeking → streaming → done`;
- **every `messages` event carries its last row's `{partition}-{offset}`** as
  the SSE id;
- **a sixth stream from one named caller is served and their oldest is
  closed**, a different caller is untouched, and callers a proxy makes
  indistinguishable are not rationed against each other;
- **a shutdown with streams open drains in milliseconds**, not in the
  termination grace period.

Two from the original plan are not asserted here, and neither was quietly
dropped:

- **the byte-budget assertion on the tail** is kaas-lib's, and runs in its
  `cargo xtask integration` against a container. kaas-ui has no Docker, and
  re-measuring it here would test the library rather than this layer;
- **a hand-corrupted batch** has no fixture: neither live cluster will corrupt
  a batch on request, and kaas-lib already covers the decoder path in
  `a_corrupt_batch_yields_malformed_and_the_scan_continues`. What kaas-ui
  guards is that a malformed row *survives the layer* — the row type, its
  rendering and the raw-hex detail exist and are unit-tested, but the
  end-to-end path is unproven against real damage.

## Exit criteria

- [x] **`limit` means what the API says it means** — the tail merges across
  partitions and truncates, and `cargo xtask live` shows 20 returned from 32
  fetched on a 16-partition topic, which is the `div_ceil` spread made visible
- [x] **the stream dies with its client** — asserted by abandoning streams and
  watching the governor's slots come back
- [x] **the stream is bounded** — `max_buffered_records` for the library's
  buffer, a drop-oldest ring for the hand-off, and a dropped count that is
  reported rather than swallowed
- [x] **message view URL fully round-trips through search params** — zod-validated,
  every control writes to the URL and nothing mirrors it in `useState`
- [x] **a shutdown does not wait for streams that never end** — not in the
  original list, and it should have been
- [~] **tail under 5% of partition bytes** — true, and asserted in *kaas-lib's*
  integration suite rather than here. kaas-ui has no Docker, and re-measuring it
  would test the library rather than this layer
- [~] **malformed batch and malformed payload are visibly different rows** —
  both row types exist, render differently and are unit-tested; the end-to-end
  path is unproven because neither live cluster will corrupt a batch on request
- [ ] **no `tokio::spawn` that outlives a response** — amended rather than met;
  see below
- [ ] **`kaas-ui-serde` has no axum dependency and no panic path** — not built.
  A decision, not an omission: see below

Not covered by any of the above, and worth naming rather than leaving implied:
**the frontend has never been verified in a browser under load.** The render
budget in the spec — ten thousand records a second, React commits at roughly
seven a second, none over 16 ms — needs the React Profiler and a real load
generator. The design is built for it (`getSnapshot` returns a stable reference,
the transport never touches React state, row height is fixed) and none of that
is *measured*.

## Decisions this phase changed

**The stream endpoint is `messages/stream`, and there are four routes, not
two.** `messages` (one bounded page) exists for "load more" past the end of a
window, and `messages/{partition}/{offset}` exists because no listing route
ever sends a whole payload — a topic at 1 KB × 10k/s is 10 MB/s the browser
would parse and never draw. The list shows a 256-character preview; the rest is
fetched for the one record someone selected, cached with `staleTime: Infinity`
because a record at an offset is immutable.

**"No `tokio::spawn` that outlives a response" became "no spawn that *can*
outlive one".** The pump has to be a task rather than an inline stream: the
whole point of the bounded ring is that a slow reader loses old records instead
of stalling the fetch loop, and only a separately-scheduled producer can do
that. What preserves the original property is that the task selects on
`tx.closed()`, so dropping the response drops the scan within a poll. The
acceptance run abandons five streams and watches the slots come back, which is
the property the original wording was reaching for.

**The `scan` events do not map 1:1 onto SSE events.** Records are batched on a
100 ms interval — one event per record saturates the browser's parser long
before the list does — and malformed batches ride inside `messages` as a row
kind rather than in `error`. Both are recorded in
[reference/http-contract.md](reference/http-contract.md).

**Two changes were needed in kaas-lib, not one.** The anchored tail was
expected. What was not: `scan` from `StartPosition::Latest` finished in seven
milliseconds having emitted nothing, because a partition starting at its own
log end is marked finished — which looks exactly like a working live view of an
idle topic. `ScanSpec::following` fixes it downstairs, where version and
implementation knowledge belongs. A third fix came out of the same session:
`scan` from an offset emitted records *before* it, because a fetch begins at
the batch containing the offset and only the backward walk was filtering.

**A shutdown has to end the streams, not wait for them.** Found by running,
after the process refused to exit on SIGTERM. `with_graceful_shutdown` stops
accepting and waits for in-flight connections, and an SSE response is an
unbounded body — it completes when the stream does, and a live tail's stream
completes when the client leaves or its lifetime expires. A shutdown is
neither, so the drain waited on a response that would never finish and the
process had to be killed. In Kubernetes that is the full
`terminationGracePeriodSeconds` on every rollout, with every open stream
severed by SIGKILL and no `phase: done` to tell the client a deploy happened
rather than a network fault.

The fix is a latch every open stream watches: on SIGTERM the pump emits
`phase: done`, drops the scan, and the body completes. 30+ seconds became
~50 ms, and `cargo xtask live` asserts it with three streams open. A ten-second
drain deadline backs it up, because the choice is not "wait longer or lose
data" — it is "exit tidily now, or be SIGKILLed at the deadline having waited
anyway".

**A proxy in front will buffer the stream unless told not to.** `Cache-Control:
no-transform` and `X-Accel-Buffering: no`, and they are not optional: through
the Cloudflare tunnel that fronts this cluster the browser received *nothing*
without them, while the same stream through code-server alone delivered 4.4 KB
in five seconds. Every layer reported success — the request simply stayed open
and empty, which is the hardest kind of failure to attribute.

**Time seeks are reported, not interpreted.** `kaas` holds no timestamp index
and answers a time seek with no offset at all — a legitimate response, and
indistinguishable from "nothing was written since". Rather than guess, the
stream carries a `resolved` block naming what each partition said, and the UI
renders it beside the empty window. See
[reference/environment.md](reference/environment.md).

**`kaas-ui-serde` still does not exist.** Payload rendering is `Payload::of` in
`kaas-ui-core::dto` — UTF-8 where the bytes are text, hex where they are not,
with the encoding said out loud. That is the sniff order this phase called for
minus the JSON step and minus the per-topic override, and the crate is created
by the phase that fills it rather than up front.
