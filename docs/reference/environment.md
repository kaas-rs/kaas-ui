# The development environment

kaas-ui is developed from a pod **inside** the Kubernetes cluster that hosts
both target Kafka clusters. This is not incidental — it decides the whole
verification strategy.

## What that gives us, and what it takes away

**Docker is not available here.** kaas-lib's acceptance suite uses
`testcontainers` against `apache/kafka:4.3.1`; kaas-ui cannot copy that.

**Service DNS is directly dialable.** No port-forward, no proxy. A `cargo run`
in this workspace reaches both brokers.

```
$ getent hosts kaas.kaas.svc.cluster.local
10.43.190.97
$ getent hosts kafka-cluster-kafka-bootstrap.strimzi.svc.cluster.local
10.43.77.227
```

So kaas-ui trades a hermetic single-broker fixture for **two shared, long-lived,
three-node clusters running software we did not choose** — which is the harder
and more honest target, and the one PLAN.md §10 calls the dogfooding surface.
Every phase's acceptance command below runs against them.

## The two clusters

| | `kaas` | `strimzi` |
|---|---|---|
| namespace | `kaas` | `strimzi` |
| bootstrap (plain) | `kaas.kaas.svc.cluster.local:9092` | `kafka-cluster-kafka-bootstrap.strimzi.svc.cluster.local:9092` |
| what it is | the `kaas` broker, 3 replicas, under development | Apache Kafka **4.2.0** via Strimzi, 3 dual-role nodes |
| listeners | plain 9092, authed 9095, tls 9093 | plain 9092, tls 9093 |
| api keys advertised | **37** | **75** |
| topics | 21 | 14 |
| groups | 4, all reporting `group_type: ""` | 8, all `classic` |
| authorizer | yes — 24 ACLs | none configured |
| topic ids | **not reported** — `Fetch` stays on the name path | reported — `Fetch` runs v18 by id |

Both are `ClusterIP` only; there is no ingress in this cluster, so kaas-ui is
reached in development by running it here and using the code-server port
forwarding, not by publishing a hostname.

### Credentials that exist, for the phases that need them

Strimzi `KafkaUser`s: `alice` (scram-sha-512), `bob-mtls` (tls). Secrets
`kafka-cluster-cluster-ca-cert` (the CA the broker certs chain to) and
`kafka-cluster-clients-ca-cert`. kaas has the same two users plus
`pipeline-runner` and `throttled-user`.

Nothing in kaas-ui resolves credentials automatically. Same rule as kaas-lib's
`live-cluster` skill: a run that needs SASL is given the credentials
explicitly, so an unauthenticated run cannot silently pick up a secret it
happened to be able to read.

## Measured facts, not assumed ones

Everything in this section came out of a probe run against both clusters on
2026-08-01, using `kafka-admin` 0.1.0 from crates.io.

### The capability gap is the product's best test fixture

`kaas` advertises a strict **subset** of Strimzi's api keys — 38 keys differ,
none of them unique to `kaas`. Absent from `kaas` entirely:

`DescribeCluster` (60), `DescribeTopicPartitions` (75), `ConsumerGroupDescribe`
(69), `ShareGroupDescribe` (77), `DescribeProducers` (61), `DescribeTransactions`
(65), `ListTransactions` (66), `ListPartitionReassignments` (46),
`AlterPartitionReassignments` (45), `ElectLeaders` (43), `DescribeQuorum` (55),
`Describe`/`AlterUserScramCredentials` (50/51), `ListConfigResources` (74),
delegation tokens (38–41), every share-group api (76–92),
`OffsetForLeaderEpoch` (23), `AlterConfigs` (33), `UpdateFeatures` (57),
`UnregisterBroker` (64).

That maps onto UI features almost one for one: on `kaas` the transactions tab,
the quorum panel, the SCRAM list and the reassignment view have nothing behind
them, and on Strimzi they do. **Put the two side by side and the tab sets are a
conformance report** — PLAN.md §5's claim, and it is true today rather than
aspirationally.

### `DescribeCluster` is missing on kaas — Phase 0 must not depend on it

```
kaas    → describe_cluster: no usable version of DescribeCluster:
          broker offers None, we speak Some((0, 2))
strimzi → cluster_id=OA0xlxN0TqGSMHbW4vIUUA controller=Some(1) brokers=3
```

The fleet view therefore renders from `Cluster::snapshot()` alone in Phase 0
(brokers, controller id, cluster id, topic list — all present on both), and
Phase 1 *enriches* with `describe_cluster` where it exists. See
[upstream-asks.md](upstream-asks.md) item 0 for the alternative.

### Where the broker outruns the codec

Strimzi advertises 8 keys newer than `kafka-protocol` 0.17 can encode —
`ListOffsets` (the v11 tiered sentinel), `WriteTxnMarkers`, `ShareFetch`,
`ShareAcknowledge`, `AddRaftVoter`, `WriteShareGroupState`,
`ReadShareGroupStateSummary`, `DescribeShareGroupOffsets` — and 2 keys it cannot
name at all: **88 and 89**, `StreamsGroupHeartbeat` and `StreamsGroupDescribe`.

Against `kaas` both counts are zero. Running the pair exercises both branches.

### The read path, on real data

`kperf-bench` is 16 partitions and ~40M records on `kaas`, ~45M on Strimzi. It
is the tail fixture.

```
kaas    tail(kperf-bench, 20) -> 32 records, 16 fetches, 324765 bytes received
strimzi tail(kperf-bench, 20) -> 32 records, 16 fetches, 325180 bytes received
```

Two things to carry forward:

- **`TailSpec::limit` is a per-topic target spread across partitions with
  `div_ceil`.** 20 over 16 partitions is 2 each, so 32 come back. The HTTP
  `?limit=` must either say so or truncate after merging — decided in
  [Phase 3](../04-phase-3-messages.md).
- **~325 KB to reach the tail of a 40M-record topic** is the backward walk
  working. Phase 3's acceptance asserts on this via `ConnectionStats`, and the
  numbers above are the baseline.

## Reusing kaas-lib's `livetest`

kaas-lib ships a `livetest` binary and a `live-cluster` skill built for exactly
these two clusters. `livetest probe` emits a sorted, diffable `key = value`
inventory and mutates nothing.

That is kaas-ui's **oracle**: what the UI renders for a cluster must agree with
what `livetest probe` reports for it. Where a phase's acceptance says "matches
the probe", this is the mechanism:

```sh
cd ../kaas-lib
eval "$(.claude/skills/live-cluster/resolve-target.sh strimzi)"
cargo run -q -p livetest -- probe > /tmp/strimzi.txt
eval "$(.claude/skills/live-cluster/resolve-target.sh kaas)"
cargo run -q -p livetest -- probe > /tmp/kaas.txt
diff -u /tmp/strimzi.txt /tmp/kaas.txt
```

Only `probe` and `read` are safe to point at these clusters casually. `smoke`
and `sweep` mutate, and kaas-ui has no reason to run either.

## Target naming in kaas-ui config

kaas-ui's own `config.yaml` uses the same two ids throughout the phases:

```yaml
clusters:
  - id: kaas
    name: kaas (dev)
    bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
    labels: { env: dev, kind: kaas }
  - id: strimzi
    name: kafka-cluster (strimzi)
    bootstrap: ["kafka-cluster-kafka-bootstrap.strimzi.svc.cluster.local:9092"]
    labels: { env: dev, kind: strimzi }
  - id: dead
    name: nonexistent
    bootstrap: ["nowhere.invalid:9092"]
    labels: { env: dev, kind: none }
```

The third entry is not padding. Phase 0's acceptance is that it renders as an
unreachable card without delaying the other two, and it is the cheapest possible
regression test for lazy connection.

## Also running here

`kafbat-ui` is deployed in namespace `kafbat-ui`, pointed at both clusters, with
GitHub OAuth2. It is the thing kaas-ui is shaped like, available for
side-by-side comparison — and its auth config is a live demonstration of
PLAN.md §6's argument, since GitHub OAuth is exactly the provider-specific code
path Dex removes.
