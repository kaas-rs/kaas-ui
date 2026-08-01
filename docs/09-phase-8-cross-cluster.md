# Phase 8 — cross-cluster

*PLAN.md milestone M8.*

**Goal.** Topic search across the fleet, cluster comparison, capability diff.

The payoff for multi-cluster being the design centre rather than a feature.
Every screen before this one answers a question about *a* cluster; these answer
questions about *the fleet*, and none of them has a good answer today with any
tool.

## Cross-cluster topic search

```
GET /api/search/topics?q=orders-*&labels=env:prod
```

"Which of my twelve clusters has a topic matching `orders-*`" currently requires
twelve tabs. It is the single most-wanted thing a multi-cluster UI can do and it
costs almost nothing: every cluster already holds a metadata snapshot, so the
search is over memory, not over the network.

Results group by topic name, not by cluster, so a topic present on five clusters
is one row with five badges — which immediately shows partition-count and
replication-factor drift between environments.

## Cluster comparison

```
GET /api/compare?a=kaas&b=strimzi
```

Three diffs on one screen:

**Capability diff.** The Phase 1 projection for both clusters, side by side. On
the dev pair this is 38 differing api keys and it is a conformance report:
`kaas` advertises 37 keys, Strimzi 75, and the difference is exactly the list of
things `kaas` has not implemented yet.

**Config diff.** Broker and topic configs where a topic exists on both. This is
how "it works in dev and not in prod" gets answered, and it is the most
operationally useful of the three.

**Topic diff.** Present on A only, B only, or both with differing partition
count or replication factor.

## The fleet dashboard, finished

Phase 0 built cards. This phase adds what only makes sense once there are
several:

- **cross-cluster totals** — brokers, topics, partitions, under-replicated,
  offline, summed and grouped by label;
- **the outlier row** — clusters differing from their label-group peers, which
  is where a misconfiguration shows up first;
- **per-cluster background-refresh control.** PLAN.md §10 flags fan-out cost as
  a real risk: twelve clusters polling is twelve metadata refreshes plus
  whatever the open page requests. "Pause background refresh for clusters not on
  screen" is worth building **before** the fleet gets large, not after — and
  this is the last phase where it can still be retrofitted cheaply.

## Traps

- **Fan-out needs a concurrency limit and a per-cluster timeout**, both. Twelve
  clusters, one of them slow, must not make the search take as long as the
  slowest. Partial results are the envelope's whole purpose: return the ten that
  answered and the two that did not.
- **Search must not trigger metadata refreshes.** It reads snapshots. A search
  that refreshes twelve clusters is a search that costs more than the twelve
  tabs it replaced.
- **Comparison must not assume both clusters answer the same api keys.** That is
  the entire point of the screen; the diff of a present and an absent feature is
  a *row*, not an error.
- **Label selectors are the grouping vocabulary everywhere** — fleet view,
  roles, search filters. One implementation, in `kaas-ui-core`, used by all
  three.

## Acceptance

```sh
cargo xtask live --config config.dev.yaml
```

- `?q=kperf*` returns matches on both `kaas` and `strimzi` grouped by topic
  name, showing that `kperf-bench` is 16 partitions on both and
  `kperf-bench-throttled` exists only on `kaas`;
- search across a fleet including the `dead` cluster returns results from the
  two live ones plus one entry in `errors`, with `200 OK`;
- search response time with `dead` present is within 200ms of without it;
- the capability diff renders **38 differing api keys** between `kaas` and
  `strimzi`, matching `comm` over two `livetest probe` reports;
- the config diff for `kperf-bench` shows the replication-factor difference (1
  on `kaas`, 3 on `strimzi`) as a row;
- with a cluster's card off screen, its background refresh interval backs off,
  verified on `MetadataSnapshot::age()` and connection counters;
- no search triggers a metadata refresh — asserted on `fetched_at` being
  unchanged across a search.

## Exit criteria

- [ ] cross-cluster search reads snapshots only, never refreshes
- [ ] fan-out bounded by concurrency limit and per-cluster timeout
- [ ] partial fleet results are a 200 with an errors array
- [ ] capability diff reproduces the `livetest probe` diff
- [ ] background refresh backs off for off-screen clusters
- [ ] one label-selector implementation, used by fleet, roles and search

---

## After Phase 8

PLAN.md §10's last risk is the one to hold: *read-only is a product decision.*

> The first request after launch will be "can it just delete this one topic".
> The value is that no code path exists and no mutation audit is needed — one
> write endpoint forfeits that permanently.

The four CI greps are what make that a property rather than an intention. They
should outlive every phase in this plan.
