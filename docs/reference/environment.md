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
| listeners | plain 9092, authed 9095, tls 9093 | plain 9092, tls 9093, **oauthbearer 9094** |
| api keys advertised | **40** (was 37) | **75** |
| topics | 13 | 17, of which 2 internal |
| groups | 2, all reporting `group_type: ""` | 16, all `classic` |
| authorizer | yes — 31 ACLs (was 24) | yes — it answers `DescribeAcls`, where this file recorded `SECURITY_DISABLED(54)` until Phase 7 asked it |
| SCRAM credentials | `alice`, `throttled-user` | `alice` |
| client quotas | none configured | none configured |
| topic ids | **not reported** — `Fetch` stays on the name path | reported — `Fetch` runs v18 by id |

**The api-key count moved too, and it was the row this file called stable.**
`kaas` advertised 37 keys when this was written and advertises 40 now — it is a
broker under development, and the number going up is the whole point of it. Read
it as "far fewer than Strimzi's 75", which is the fact the capability work
rests on, rather than as a constant.

**The topic and group rows move; the rest does not.** Canaries connect and
leave, benchmarks create and delete. Those two numbers were 21/14 topics and
4/8 groups when this file was first written, and re-reading them is one
`livetest probe` — so treat them as "roughly this many, both non-empty" rather
than as constants. No assertion in `cargo xtask live` pins them, deliberately:
a test that fails because someone's canary restarted is a test that gets
disabled. The api-key counts are the stable facts, and they are the ones the
capability work rests on.

**The ACL count is not one of them, and neither is the authorizer row.** Both
were re-measured when Phase 7 built the screen that reads them, and both had
moved: 24 bindings had become 31, and Strimzi — recorded here as having no
authorizer at all — answers the call. `cargo xtask live` asserts the *shape* of
a binding and that the list is non-empty, never the count: a test that fails
because somebody granted a principal a topic is a test that gets disabled.

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

### The OAUTHBEARER listener, and what kaas-ui needs to use it

Strimzi's `internal` listener on **9094** is `SASL_SSL` with OAUTHBEARER
validated against an Entra tenant's JKWS, and `authorization: simple` behind
it — `StandardAuthorizer`, default-deny. It is a different service from the
one everything else here dials:

```
kafka-cluster-kafka-internal-bootstrap.strimzi.svc.cluster.local:9094   ← OAUTHBEARER
kafka-cluster-kafka-bootstrap.strimzi.svc.cluster.local:9092            ← plaintext, anonymous
```

Note the `-internal-` segment. `kafka-cluster-kafka-bootstrap` serves
9091/9092/9093 and nothing on 9094, so getting the service name wrong reads as
"connection refused" rather than as an auth problem.

```yaml
kafka_clusters:
  - id: strimzi
    bootstrap: ["kafka-cluster-kafka-internal-bootstrap.strimzi.svc.cluster.local:9094"]
    tls:
      # `kafka-cluster-cluster-ca-cert`, key `ca.crt` — the *cluster* CA, not
      # the clients CA. Hostname verification stays on: the listener
      # advertises names Strimzi puts in the broker cert SANs.
      ca_file: /etc/kaas-ui/kafka-ca/ca.crt
    sasl:
      mechanism: oauthbearer
      token_endpoint: https://login.microsoftonline.com/<tenant>/oauth2/v2.0/token
      client_id: <client-id>
      # No client_secret: it comes from $KAAS_UI_CLIENT_SECRET_DEV_STRIMZI
      scope: <client-id>/.default
```

Four things about that block are load-bearing, and each has cost somebody an
afternoon:

- **No `client_secret` at all**, and that is the general rule rather than an
  OAuth quirk. Every credential kaas-ui takes — a SASL `password`, an OAuth
  `client_secret`, a registry's `password` or `bearer_token` — reads the same
  way: write the key and it wins, omit it and it comes from
  `KAAS_UI_<CREDENTIAL>_<ENVIRONMENT>_<ID>`, which the deployment fills from a
  `secretKeyRef`. So `dev`/`strimzi` reads `KAAS_UI_CLIENT_SECRET_DEV_STRIMZI`
  here and would read `KAAS_UI_PASSWORD_DEV_STRIMZI` for SASL. The config is a
  ConfigMap and the secret is a Secret, so the deployed file sets neither.
  Overriding from the file is for a local run. Two things whose ids flatten to
  one variable are refused at startup rather than left sharing a credential —
  including a cluster and a registry with the same id, since the name comes
  from the `(environment, id)` they share. It is read **once, at startup**, so
  a rotation needs a pod restart: the config poller watches the config file,
  and a Secret rewritten underneath a running process goes unnoticed.
- **`/oauth2/v2.0/token`, not v1.** The v1 endpoint issues
  `iss: https://sts.windows.net/{tid}/`, which fails a broker pinned to the v2
  issuer. The symptom is a SASL failure whose reason is only in the *broker's*
  log.
- **`scope: <client-id>/.default`, bare.** `api://<client-id>/.default` fails
  with `AADSTS500011` unless the app registration has an Application ID URI.
- **The principal is not the client id.** With no `oauth.username.claim` set,
  Strimzi takes the principal from the token's `sub`, which for an Entra
  client-credentials token is the service principal's **object** id. That is
  what the `KafkaUser` must be named for, and why the CR in
  `apps/strimzi/kafka-users/` is a uuid rather than `kaas-ui`.

Measured, 2026-08-09, against this listener: 3 brokers, 17 topics — exactly
what the broker holds — 16 capability features available, 28 groups described
and an Avro record read. All of those need an ACL under default-deny, so they
are the proof that the token authenticated as the principal the ACLs name
rather than as anything more permissive. A wrong client secret renders as an
unreachable card carrying the issuer's own `AADSTS7000215`, with the secret
nowhere in the message.

The full CLI walkthrough this was built against, including how to mint a token
by hand, is `docs/strimzi-oauth-cli.md` in the cluster repo. **That file holds a
live client secret; nothing in kaas-ui does, and nothing in kaas-ui should.**

### …and the same block with no secret at all

The deployment against `kaas`'s OAUTHBEARER listener (9096) authenticates by
**workload identity** instead, which is one key different:

```yaml
sasl:
  mechanism: oauthbearer
  token_endpoint: https://login.microsoftonline.com/<tenant>/oauth2/v2.0/token
  client_id: <client-id>
  client_assertion_file: /var/run/spiffe/svid/azure-token
  scope: <client-id>/.default
```

The file is a SPIFFE JWT-SVID written by a `spiffe-helper` sidecar, minted for
`spiffe://spiffe.smeding.cloud/ns/<namespace>/sa/<service-account>` and
addressed to `api://AzureADTokenExchange`; the Entra app registration carries a
federated credential naming that issuer and that subject, and hands back an
access token in exchange. So there is no `KAAS_UI_CLIENT_SECRET_*` for such a
cluster, no Secret, and nothing to rotate — the credential lives for minutes
and cannot be used off the pod that was issued it.

Four consequences, in the order they bite:

- **`client_assertion_file` switches the exchange over.** A `client_secret`
  beside it is refused at startup naming both keys, and the environment is not
  consulted at all — a leftover variable must not resurrect the other flow.
- **The assertion is re-read on every fetch, not held.** SPIRE's JWT-SVIDs
  default to a five-minute TTL here; the access token they buy lasts an hour.
- **A missing file is retriable, not fatal.** The sidecar writes it, and it is
  allowed to be seconds behind us — which is also why nothing checks the path
  at startup.
- **The principal is still the token's `sub`**, the service principal's object
  id, exactly as with a secret. The way the token was bought is invisible to
  the broker.

`SPIFFE ID` is namespace-and-service-account shaped, so **moving kaas-ui to
another namespace or renaming its ServiceAccount invalidates the federated
credential** — the symptom is `AADSTS700213: No matching federated identity
record found`, quoted back on the fleet card.

## Measured facts, not assumed ones

Everything in this section came out of a probe run against both clusters,
first on 2026-08-01 with `kafka-admin` 0.1.0 and re-read on 2026-08-03 with
kaas-lib's `livetest probe` — which is the oracle: what the UI renders must
agree with what it reports.

### The capability gap is the product's best test fixture

`kaas` advertises a strict **subset** of Strimzi's api keys — 35 keys differ,
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
the quorum panel and the reassignment view have nothing behind them, and on
Strimzi they do. The SCRAM list is the exception this list gets wrong — `kaas`
answers `DescribeUserScramCredentials` today, and Phase 7's admin page shows
that tab on both clusters. **Put the two side by side and the tab sets are a
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

**On the capabilities page this is expected output, not a defect.** 88 and 89
render as `Unknown` with `ours: null` and `negotiated: null`; the 8 render with
`brokerAhead: true` and negotiate down to our max. Nothing degrades — none of
the 8 backs a feature kaas-ui uses, and `strimzi` reports every feature
`available`.

Three things mislead if you go looking, all of them kaas-lib's business rather
than ours:

- **88/89 are a hole, not a truncation.** `kafka-protocol` 0.17's `ApiKey` enum
  runs `ReadShareGroupStateSummary = 87` straight to
  `DescribeShareGroupOffsets = 90`. It knows three keys *above* the two it is
  missing, so "the codec stops at key N" does not explain it, and a bump that
  raises the ceiling elsewhere need not fill this.
- **`InitProducerId` reports `ours: [0,6]` against a broker's `[0,5]`** — our
  ceiling *above* the broker's, which reads like a bug and is not. `ours` is
  `ApiKey::valid_versions()`, which takes the wider of the request's and the
  response's range; the request stops at v5. It negotiates 5 and sends 5,
  because kaas-lib's wire path uses `negotiate_with` and the typed ranges.
- **`ours` is never a kaas-ui or kaas-lib decision.** It is
  `ApiKey::valid_versions()` verbatim — a fact about the codec crate. The
  remedy for every row above is one `kafka-protocol` bump upstream, and
  **0.17.0 is the latest published**, so there is nothing to bump to yet.

Documented upstream in
[Version negotiation](https://kaas-rs.github.io/kaas-lib/architecture/version-negotiation.html)
and [The upstream schema gap](https://kaas-rs.github.io/kaas-lib/compat/upstream-gap.html).

#### Known issue: both unnameable keys render as bare `Unknown`

This one *is* ours. `ApiKeyEntry.name` in `kaas-ui-core/src/capabilities.rs`
documents itself as *"the key's name, or `Unknown(n)` where this build has
none"*, but assigns `entry.api_key.name()`, which returns the bare string
`"Unknown"`. So 88 and 89 arrive as two identical `Unknown` rows,
distinguishable only by the `key` column, and the field does not do what its
own comment says.

kaas-lib's `Display` impl already writes `Unknown(89)` and falls through to
`name()` for every named key, so `entry.api_key.to_string()` is the whole fix
and changes none of the other 73 rows. Not urgent — no information is lost,
`key` carries the number — but it is a doc-versus-behaviour mismatch on a
surface whose entire job is to be legible.

### The read path, on real data

`kperf-bench` is 16 partitions and ~40M records on `kaas`, ~45M on Strimzi. It
is the tail fixture.

```
kaas    tail(kperf-bench, 20) -> 32 records, 16 fetches, 324765 bytes received
strimzi tail(kperf-bench, 20) -> 32 records, 16 fetches, 325180 bytes received
```

Two things to carry forward:

- **`TailSpec::limit` is a per-topic target that over-fetches.** Measured with
  kafka-read 0.6, which divided it across every partition with `div_ceil`: 20
  over 16 partitions is 2 each, so 32 come back. 0.9 spreads it over the
  partitions that hold something instead, and over-fetches by keeping a
  partition's last chunk whole. Either way the HTTP `?limit=` must say so or
  truncate after merging — decided in [what is built](../11-built.md).
- **~325 KB to reach the tail of a 40M-record topic** is the backward walk
  working. Phase 3's acceptance asserts on this via `ConnectionStats`, and the
  numbers above are the baseline.

### `kaas` answers a timestamp seek with nothing

Found by running the two time modes side by side, and the sharpest degradation
fixture in the environment so far because **both clusters are behaving
correctly**.

`ListOffsets` answers "the first offset at or after this instant". Asked about
an instant well inside retention:

```
strimzi  sinceTime(t) -> p0 = 999993 @ t, p1 = 1123840 @ t+1     (resolved)
kaas     sinceTime(t) -> p0 = none,       p1 = none              (unresolved)
```

`kaas` advertises `ListOffsets` v1–7, so it speaks the request — v1 is where
timestamp lookup arrives. It simply has no timestamp index to answer from, and
"no offset at or after" is a legitimate response. It is also **the same
response as "nothing has been written since then"**, and nothing in the
protocol distinguishes them.

Neither kaas-lib nor kaas-ui can tell those apart, and neither should try:
inferring that a broker "must" have an index is precisely the version knowledge
that CLAUDE.md rule 2 keeps out of the UI. So the answer is *reported* rather
than interpreted — the stream and the page both carry a `resolved` block naming
what each partition said, and the UI renders "this cluster resolved 14:30 to no
offset on any of its 16 partitions" beside the empty window.

The consequence for anyone reading `kaas`: **seek by offset, not by time.** The
offset modes are exact on both clusters.

### "Segment count" is not obtainable, and is not worth obtaining

kafbat-ui's topic overview shows a "Segment Count", and kaas-ui deliberately
does not match it. `DescribeLogDirs` — the only call either UI has here —
reports no segment *files* at all: it returns one entry per **replica copy per
log directory**, carrying `size_bytes` and `offset_lag`. kafbat's number is
that entry count, which is the replica count under a borrowed name; on both
development clusters it reads exactly `partitions × replication factor` unless
a directory move is in flight.

kaas-ui carries the same number as `logDirEntryCount`, named for what it
counts, on the topic overview card. A true segment-file count would need a
protocol surface Kafka does not offer remote clients, so it is not filed as an
upstream ask either — there is nothing upstream could call.

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

## The schema registry

**Apicurio Registry 3.2.4**, in namespace `apicurio`, at
`http://apicurio-registry.apicurio.svc.cluster.local:8080`. Both the Confluent
compatibility API and Apicurio's native one are served:

| | |
|---|---|
| `…/apis/ccompat/v7` | what kaas-ui speaks. `GET /subjects` answers a JSON array |
| `…/apis/registry/v3` | Apicurio's own. **Not supported**, and pointing `url` at it is a configuration error |

One registry for the whole `dev` environment, referenced by both live clusters,
which is the fixture the "shared, not owned" design is asserted against: the
same schema id resolves to the same schema on both sides because it is the
same registry answering.

### The canary is the Avro fixture

`kaas-producer-canary` runs against **each** cluster and produces
Confluent-framed Avro to `kaas-canary-v1`, registering
`kaas-canary-v1-value` — schema id **1** — through the ccompat endpoint:

```
--bootstrap kaas.kaas.svc.cluster.local:9092
--topic kaas-canary-v1
--schema-registry http://apicurio-registry.apicurio.svc.cluster.local:8080/apis/ccompat/v7
```

`rs.kaas.canary.Heartbeat` has five fields, and `sequence` is one of them —
which is what settles where the payload filter runs, since an Avro record
carries its field *names* nowhere in its bytes. A filter that finds `sequence`
in a window of this topic could only have run after the decode. Its headers (`content-type`,
`canary-run`, `canary-version`) are ordinary **unframed** payloads on a
registry-backed record, which is the live fixture for "absence of framing is
not a failure".

There is no Protobuf and no JSON Schema topic here, and no subject with a
reference. Those three paths are asserted against a stub registry in
`crates/kaas-ui-serde/tests/registry.rs` — see `docs/11-built.md`, "What is
still unproven".

## Also running here

`kafbat-ui` is deployed in namespace `kafbat-ui`, pointed at both clusters, with
GitHub OAuth2. It is the thing kaas-ui is shaped like, available for
side-by-side comparison — and its auth config is a live demonstration of
PLAN.md §6's argument, since GitHub OAuth is exactly the provider-specific code
path Dex removes.
