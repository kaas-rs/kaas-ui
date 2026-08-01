# Phase 5 — consumer groups

*PLAN.md milestone M5.*

**Goal.** All four group kinds including `Unrecognized`, committed offsets via
`OffsetFetch`, lag against `ListOffsets(LATEST)`, and member assignments.

## Four kinds, four components

`GroupDescription` is an enum with four genuinely different variants. A classic
group has a generation and a negotiated assignor; a KIP-848 consumer group has a
group epoch, an assignment epoch and a *server-chosen* assignor; a share group
has its own epochs; an unrecognized group has a type string and nothing else.

Mirror the Rust enum with `#[serde(tag = "kind")]` and render four components.
Flattening to one all-optional TypeScript interface moves the same knowledge
somewhere the compiler cannot check it — and then every field access is a guess.

```ts
type Group =
  | { kind: "classic";      groupId: string; state: GroupState; protocolType: string; protocol: string; members: ClassicMember[] }
  | { kind: "consumer";     groupId: string; state: GroupState; groupEpoch: number; assignmentEpoch: number; assignor: string; members: ConsumerMember[] }
  | { kind: "share";        groupId: string; state: GroupState; groupEpoch: number; assignmentEpoch: number; assignor: string; members: ShareMember[] }
  | { kind: "unrecognized"; groupId: string; state: GroupState; groupType: string };
```

**`Unrecognized` is a success, not a failure.** The group exists, it is listed,
and the UI can say what it is. A UI that hard-fails on an undescribable group is
a UI that hard-fails on most real clusters — streams groups (KIP-1071) appear on
any Strimzi cluster running Kafka Streams, and `kafka-protocol` 0.17 has no
schema for api keys 88 or 89.

The `Undescribable` panel built in Phase 1 is what renders it.

## What the dev clusters give, and do not

Measured: `strimzi` has 8 groups, all `classic`. `kaas` has 4, all reporting
`group_type: ""` — brokers too old to report a type, which
`GroupListing::describable()` accepts and routes down the classic path.

**Neither cluster produces a consumer, share or streams group today.** So:

- the classic path is covered live, twice, on two implementations;
- the `""` path is covered live on `kaas`;
- the consumer, share and `Unrecognized` paths need **unit tests over
  constructed `GroupDescription` values**, plus a live check if a KIP-848
  consumer or a Streams app is deployed to `strimzi` for the purpose.

Deploying a Kafka Streams application to the `strimzi` namespace is the cheapest
way to get a real `Unrecognized` group — and the `kaas` namespace already has a
`streams-wordcount` fixture in the cluster repo that could be pointed at
Strimzi. Worth doing: `Unrecognized` is the variant most likely to be wrong and
least likely to be exercised by accident.

## Lag needs care

End offset minus committed offset per partition, where **"no commit yet", "empty
partition" and "zero lag" are three different states that must not all render as
`0`**:

| state | condition | renders as |
|---|---|---|
| no commit yet | `OffsetFetch` returns `-1` | `—` |
| empty partition | earliest == latest | `∅` |
| caught up | committed == latest, latest > earliest | `0` |
| lagging | latest − committed | the number |

`LagCell` from the design system is this table. Collapsing it is the single most
common bug in Kafka UIs, and it makes an idle consumer and a broken one look
identical.

Total lag for a group is the sum over partitions **that have a commit**, and the
UI says how many did not.

## Cost

A groups page on a cluster with 300 groups currently costs 300 `FindCoordinator`
round trips on a cold cache, then one `OffsetFetch` per group. Both are fixed by
[upstream asks](reference/upstream-asks.md) 2 and 3 — batched
`FindCoordinator` (KIP-699) and multi-group `OffsetFetch` (v8+) — which together
take the page from O(n) round trips to roughly O(1).

Until then: **fetch group details lazily, on row expansion**, and never fetch
offsets for the whole list. With 8 groups on the dev clusters this is invisible;
it will not stay invisible.

## Traps

- **Do not parse `__consumer_offsets`.** kaas-lib's non-goals are explicit:
  `OffsetFetch` is the interface, the internal format is not stable. Phase 6 may
  *display* the topic's contents; group views still go through the RPC.
- **`GroupState::parse` is string-based** and has an `Unknown` path. Render the
  raw string for states this build does not name, same as `ErrorCode::Unknown`.
- **A group's coordinator is not any broker.** kaas-lib routes it; do not
  short-circuit that by sending to a broker you already have a connection to.
- **Member assignments are `Vec<(String, Vec<i32>)>`** — topic to partitions,
  possibly overlapping across members mid-rebalance. Render the overlap rather
  than deduplicating it away; it is the signal that a rebalance is in progress.
- **Classic-group `assignment` is raw `Bytes`.** It is the consumer protocol's
  own encoding. Show it as hex or decode it in `kaas-ui-serde`, but do not
  pretend it is a string.

## Acceptance

```sh
cargo xtask live --config config.dev.yaml
```

- the 8 classic groups on `strimzi` render with members, client ids, hosts and
  per-partition assignment, matching `livetest probe`;
- the 4 groups on `kaas` render through the same component despite reporting an
  empty `group_type`;
- a fixture exercising all four kinds — three constructed, one live if a Streams
  app is deployed — renders four distinct components, **the streams group as
  undescribable rather than as an error**;
- the four lag states render as `—`, `∅`, `0` and a number respectively,
  asserted per state, not just "lag shows a number";
- expanding a group row triggers exactly one coordinator lookup and one
  `OffsetFetch`, verified on connection counters;
- collapsing the list does not leave a request in flight.

## Exit criteria

- [ ] four components, discriminated union, no all-optional interface
- [ ] `Unrecognized` renders as information, never as an error
- [ ] four lag states visibly distinct
- [ ] group details fetched lazily
- [ ] `__consumer_offsets` never parsed for group data
- [ ] upstream asks 2 and 3 filed with the 300-group number attached
