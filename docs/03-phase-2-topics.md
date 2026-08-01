# Phase 2 — topics

*PLAN.md milestone M2.*

**Goal.** Topic list, topic detail, partitions, configs, replica placement, and
under-replicated highlighting. The first screen with enough rows to need
virtualisation and enough failure modes to need the envelope.

## What gets built

**List.** `GET /api/clusters/{id}/topics`. Backed by `admin.list_topics()` for
names and the metadata snapshot for partition counts, with `admin.topic_sizes()`
as an optional enrichment column — it is a `DescribeLogDirs` fan-out and should
not be on the critical path for rendering the list.

Server-side filtering and sorting. Do **not** load 5000 topics into the browser
and filter there; the fleet has clusters where that is a real number.

**Detail.** `GET /api/clusters/{id}/topics/{topic}` → `describe_topics([...])`,
which internally prefers `DescribeTopicPartitions` and falls back to `Metadata`
where the newer call is unreachable. `kaas` does not implement api key 75, so
**both branches are exercised by the two dev clusters** without any special
casing here — which is exactly PLAN.md §2's claim that a partially-implemented
broker is indistinguishable from an old one.

Per partition: leader, leader epoch, replicas, ISR, offline replicas,
`under_replicated()`, earliest/latest offsets from `topic_offset_range`.

**Replica placement.** A small grid: partitions down, brokers across, cell
shaded by leader / follower / out-of-sync / offline. This is the view that makes
a bad reassignment obvious at a glance and is cheap to build from data already
fetched.

**Configs.** `describe_configs_documented(ConfigResource::topic(name))`, with
`ConfigSource::is_explicit()` distinguishing an override from an inherited
default, and the documentation string in a tooltip.

## Traps

- **`topic_offset_range` calls `refresh_topics` first.** Calling it per row on a
  list of 500 topics is 500 metadata refreshes. Fetch offsets on the detail page
  only, or batch through `list_offsets` with an explicit partition list.
- **`__consumer_offsets` and other internal topics.** `TopicInfo::internal` is
  the flag. Hide them behind a toggle, and never parse the contents — kaas-lib's
  non-goals are explicit that `OffsetFetch` is the interface.
- **`kaas` reports no topic ids.** `TopicId::is_zero()` is true for every topic
  there. Do not render an empty UUID column as though the data were missing;
  omit the column when the cluster reports none.
- **Under-replicated is not the same as offline.** `replicas.len() != isr.len()`
  is under-replicated; a non-empty `offline_replicas` is worse. Two different
  colours, not one.
- **Partition counts belong to the snapshot, which has an age.** Show it. A
  topic that gained partitions 20 seconds ago should not look like a stale
  render bug.

## Acceptance

```sh
cargo xtask live --config config.dev.yaml
```

The headline test from PLAN.md §8, run against both clusters:

- describing **50 topics of which 2 do not exist** returns `200 OK` with 48
  items and 2 errors in the envelope; the frontend renders 48 rows and 2 error
  chips, and nothing about the page suggests a failed request;
- topic detail for `kperf-bench` renders 16 partitions with replicas and ISR on
  both clusters, and the placement grid matches `livetest probe`;
- the same page works on `kaas` (via the `Metadata` fallback) and on `strimzi`
  (via `DescribeTopicPartitions`) with **no branch in kaas-ui** — verified by
  the version-literal grep still passing;
- `kperf-bench` on `strimzi` (replication factor 3) shows all replicas in ISR;
  the single-replica topics show no under-replication warning;
- a topic list of 21 (`kaas`) and 14 (`strimzi`) renders with counts matching
  `livetest probe`;
- sorting and filtering happen server-side — verified by asserting the response
  row count changes with the query, not just the rendering.

## Exit criteria

- [ ] 48-of-50 partial result renders as data, not as an error
- [ ] topic detail identical in shape across both clusters, no version branch
- [ ] placement grid distinguishes leader / follower / out-of-sync / offline
- [ ] internal topics hidden by default, never parsed
- [ ] list is virtualised and filtered server-side
- [ ] `cargo xtask ci` green
