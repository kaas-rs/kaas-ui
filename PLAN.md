# kaas-ui — plan

A **read-only** Kafka cluster UI in the shape of kafbat-ui, with an Axum backend
built on [`kaas-lib`](https://github.com/kaas-rs/kaas-lib) and a React frontend
on TanStack, Tailwind and shadcn/ui. Multi-cluster from day one: several `kaas`
instances and several Strimzi clusters, side by side. Authentication is OIDC
against Dex.

Written against kaas-lib as it stands (37 api keys, `Admin` + `scan`/`tail`),
with a section on the additions worth making to it — see §9.

---

## 0. The three constraints

**Read-only, by construction.** kaas-ui observes clusters. It never mutates one.
Not a config toggle that defaults to safe; the only mode.

**OIDC via Dex.** One protocol, one code path. Dex fronts GitHub, Google, Entra,
LDAP and SAML, so kaas-ui never learns about any of them. See §6.

**Multi-cluster.** Clusters differ in kind, not just in address: `kaas` is a
broker under development and will answer a different set of api keys from a
Strimzi-managed Apache Kafka. The UI must render each cluster for what it
actually is — without ever knowing what version anything is.

---

## 1. Read-only is the architecture, not a setting

`Admin::connect_read_only` refuses every mutating api key before opening a
socket, enforced on `ApiKey::is_mutating` inside `Connection::send` rather than
on the method surface. That is the whole security property, and it is already
built.

kaas-ui's job is to not undermine it:

- **One construction site.** `kaas-ui-core` contains exactly one call to
  `Admin::connect_read_only` and no call to `Admin::connect`. A grep-able
  invariant with a unit test asserting the plain constructor appears nowhere.
- **No mutating endpoint exists.** Not disabled, not 403 — absent from the
  router. There is no code path from HTTP to a mutating RPC to audit.
- **The gate is the second line, not the first.** If `Error::ReadOnly` ever
  reaches a user, kaas-ui has a bug: it built a request it should never have
  been able to build. Treat it as a 500 with a report link, not a 405.

### What this removes

Fourteen of the 37 api keys are mutating and drop out entirely: `OffsetCommit`,
`CreateTopics`, `DeleteTopics`, `DeleteRecords`, `CreateAcls`, `DeleteAcls`,
`CreatePartitions`, `DeleteGroups`, `ElectLeaders`, `IncrementalAlterConfigs`,
`AlterPartitionReassignments`, `OffsetDelete`, `AlterClientQuotas`,
`AlterUserScramCredentials`.

The twenty-odd that remain are more than enough, and several are things the JVM
tools barely surface — `DescribeProducers`, `DescribeTransactions`,
`ListTransactions`, `DescribeClientQuotas`, `DescribeUserScramCredentials`,
`ShareGroupDescribe`.

**The produce question disappears.** kaas-lib having no producer stops being a
gap to work around and becomes alignment with the product.

### What this changes about authorization

With no writes, permissions collapse to two axes, and the second is the one that
matters:

1. **Which clusters can you see?** A label selector per role.
2. **Can you see message payloads?** Topic contents are the sensitive surface in
   a read-only tool — payloads carry PII, tokens, order data. "Can browse
   metadata" and "can read message bodies" are different grants.

So audit logging is about *reads*: who opened which topic's messages, on which
cluster, when. In a read-only tool that is the log that matters, and it is the
one most likely to be skipped because nothing is being changed.

---

## 2. What kaas-lib owns — including every Kafka version

The ownership boundary is worth stating precisely, because getting it wrong is
how a UI backend slowly accumulates a second, worse copy of a client library.

### Version and implementation compatibility is entirely kaas-lib's

kaas-lib negotiates per api key per connection, clamps to the overlap, carries
`Unknown(i16)` on both the api-key and error-code enums, returns
`Error::UnsupportedApi` with both ranges when there is no overlap, degrades
undescribable groups to `GroupDescription::Unrecognized`, and falls back from
`DescribeTopicPartitions` to `Metadata` when the newer call is unreachable.

That is the complete surface of "supporting multiple Kafka versions", and none
of it is kaas-ui's business.

> **Invariant: no Kafka version number appears anywhere in kaas-ui.**
> No `if version >= 3.5`, no parsing of a broker version string, no per-version
> branch. Enforced by grep in CI, alongside the `Admin::connect` check.

If kaas-ui ever *needs* to know that some Kafka release added something, that is
the signal the knowledge belongs in kaas-lib. Push it down rather than branching
up here.

**This is also why `kaas` needs no special casing.** Negotiation is per api key,
not per broker version, so a partially-implemented broker is indistinguishable
from an old one and flows through the identical mechanism. kaas-ui does not know
which of its clusters is Apache Kafka and which is kaas, and should not.

### What is genuinely left for the UI

Exactly one thing: **deciding what absence looks like.** kaas-lib says
`ours: None` or `Error::UnsupportedApi` or `Unrecognized`; something has to
choose between a hidden tab, a greyed tab, and an explanatory panel. That is a
presentation decision and it cannot live in a client library.

So the capabilities endpoint in §5 is not kaas-ui doing negotiation. It is a
pure projection of a table kaas-lib already computed, and it should be one
function with no logic in it beyond mapping api keys to UI features.

### The four properties that shape the API

**Per-item results.** `describe_topics` returns `Vec<(String, Result<T, _>)>`.
The HTTP API preserves that shape end to end; collapsing it discards the
property on exactly the clusters that need a UI most.

**Typed errors.** `Error` separates `Transport` from `Authorization` from
`Decode` from `UnsupportedApi`. `Decode` means *kaas-lib is wrong* — report it,
don't retry it.

**A snapshot that knows its age.** `snapshot.age()` makes "as of 4 seconds ago"
honest, which matters more across twelve clusters than across one.

**Per-connection version tables.** See §5 and §9 — the per-connection part is
deliberate and has a consequence for a cluster-level UI.

---

## 3. Architecture

```
kaas-ui/
  crates/
    kaas-ui-core/      cluster registry, config, domain DTOs, capability projection
    kaas-ui-serde/     payload decoding: raw/string/json/avro/protobuf, SR client
    kaas-ui-api/       axum routers, DTOs, utoipa annotations
    kaas-ui-auth/      OIDC, sessions, RBAC, access audit
    kaas-ui-server/    the binary: config, wiring, embedded frontend
  web/                 vite + react app
  xtask/               ci, integration, docs — same shape as kaas-lib
```

Strictly layered, no cycles. `kaas-ui-serde` does not know about axum;
`kaas-ui-api` does not open sockets.

### Inherit the lint discipline

```toml
[workspace.lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
panic       = "deny"
indexing_slicing = "deny"
[workspace.lints.rust]
unsafe_code = "forbid"
```

This matters *more* here than in the library. kaas-lib's rule 2 says a malformed
record on one topic must not take down a server hosting other clusters —
kaas-ui **is** that server, and now it hosts a dozen. A panic in a payload
decoder is the failure the library's fuzz target exists to prevent,
reintroduced one layer up and blast-radiused across every cluster.

### Apply rule 1 one level up

kaas-lib is *our* upstream and it will move — §9 proposes moving it. Do not put
`kafka_admin::TopicInfo` in a `utoipa` schema. Define kaas-ui DTOs in
`kaas-ui-core` and convert at the boundary, for the same reason kaas-lib does
not expose `kafka-protocol` types: otherwise every library bump breaks the
generated TypeScript client.

---

## 4. The HTTP contract

Every data endpoint is a `GET`, because reading is what they do. Non-GET routes
exist for the auth flow and for the Dex proxy, which forwards whatever the
browser sends. Read-only is guaranteed by the single `connect_read_only`
construction site rather than by the shape of the router.

### Partial results are the default envelope

```json
{
  "items": [ { "name": "orders", "partitions": 6 } ],
  "errors": [
    { "resource": "shipments", "kind": "broker",
      "code": "UNKNOWN_TOPIC_OR_PARTITION", "codeNumber": 3, "message": "..." }
  ],
  "snapshotAgeMs": 4213
}
```

`200 OK` even when `errors` is non-empty — the call succeeded, some resources did
not. `codeNumber` is always present even when `code` is null, because against a
broker newer than the codec the number is the only searchable thing.

### Error mapping

| `Error` variant | Status | Frontend treatment |
| --- | --- | --- |
| `Transport`, `ConnectionClosed` | 502 | cluster card: unreachable, retry |
| `Timeout` | 504 | retry affordance |
| `Authentication` | 502 | **cluster** credentials — never confuse with the user's 401 |
| `Authorization` | 403 | "this cluster's principal lacks the ACL" |
| `Broker { code }` | 400 | render code and number |
| `Decode` | 500 | "this is a kaas-lib bug" + report link |
| `ReadOnly` | 500 | **our bug** — see §1 |
| `UnsupportedApi` | 501 | render *both* version ranges |
| `Unsupported`, `InvalidRequest` | 400 | query validation |

`UnsupportedApi` carrying broker range and our range is a diagnosis: `ours: None`
means bump the codec; a narrower broker range means the cluster is behind.
Across a kaas/Strimzi fleet you will see both, and the two numbers are what
distinguishes them.

### Two message endpoints, not one

```
GET /api/clusters/{id}/topics/{t}/messages/tail?limit=500      → JSON array
GET /api/clusters/{id}/topics/{t}/messages/scan?from=earliest… → text/event-stream
```

Tail is the default topic view: one shot, bounded, and the library's backward
walk guarantees it reads a fraction of the partition. Scan is for browsing and
searching, and streams.

SSE events map 1:1 onto `ScanEvent`:

| Event | Frontend |
| --- | --- |
| `record` | append to ring buffer |
| `progress` | progress bar; "approximately ordered" badge when the buffer forced an early emit |
| `malformed` | render a row with offset range and hex — **do not abort** |
| `end` | stop the spinner |

SSE rather than WebSocket: unidirectional (there is no write path to send back —
permanently, now), survives corporate proxies, free reconnect. Backpressure is a
bounded `mpsc`; on disconnect Axum drops the stream, which drops the scan, which
is safe because kaas-lib is cancel-safe by construction. That property is what
makes "user closed the tab mid-scan" a non-event rather than a leaked
connection — and with a dozen clusters and many users it is load-bearing.

**Two distinct malformed kinds, never conflated.** `ScanEvent::Malformed` is a
batch that would not decode at the protocol level. A payload that is not valid
Avro is an application-level failure on an otherwise fine record. Different
causes, different fixes, different rows.

---

## 5. Multi-cluster

The design centre of the product, not a feature bolted on.

### Configuration

```yaml
clusters:
  - id: kaas-dev
    name: kaas (dev)
    bootstrap: ["localhost:9092"]
    labels: { env: dev, kind: kaas }

  - id: orders-prod
    name: Orders (prod)
    bootstrap: ["orders-kafka-bootstrap.kafka.svc:9093"]
    labels: { env: prod, kind: strimzi }
    tls:
      ca_file: /etc/kaas-ui/certs/orders/ca.crt      # cluster-ca-cert Secret
      client_cert_file: /etc/kaas-ui/certs/orders/user.crt
      client_key_file: /etc/kaas-ui/certs/orders/user.key
      sni: orders-kafka-bootstrap
    schema_registry:
      url: http://apicurio.kafka.svc:8080/apis/ccompat/v6
```

Files, not inline PEM — Strimzi delivers the cluster CA and KafkaUser
credentials as mounted Secrets. kaas-lib's `TlsConfig` already covers custom CA,
client certs and the SNI override, and the SNI override is not optional in
practice: the address you dial through a Kubernetes Service is routinely not the
name on the certificate.

Config via `figment` (YAML + env overlay). Watch the file and reload the
registry so adding a cluster does not need a restart.

### The registry

```rust
pub struct ClusterHandle {
    pub id: ClusterId,
    pub admin: Admin,                  // always connect_read_only
    pub labels: BTreeMap<String, String>,
    pub caps: ArcSwap<Capabilities>,
    pub health: ArcSwap<ClusterHealth>,
}
```

**Connect lazily and isolate failures.** One unreachable cluster must not block
startup, hang `/health`, or slow a page that does not touch it. It renders as a
card saying "unreachable" with its `Transport` error attached. Eager connection
in `main` is the easy mistake and it makes a twelve-cluster deployment
unbootable whenever one cluster is down.

### Capabilities: projection, not negotiation

```
GET /api/clusters/{id}/capabilities
```

```json
{
  "features": {
    "consumerGroups": "available",
    "shareGroups":    { "state": "unsupported", "broker": null, "ours": [0,1] },
    "quotas":         "available",
    "describeQuorum": { "state": "unsupported", "broker": [0,2], "ours": null }
  }
}
```

Per §2, this contains no version logic — it maps kaas-lib's already-negotiated
table onto UI features. The frontend renders tabs from it, so a kaas cluster
that does not answer `DescribeClientQuotas` shows no quotas tab rather than a tab
that errors on click. Routes still exist and render an explanatory panel, so a
shared URL degrades into an explanation rather than a dead end.

**One wrinkle worth knowing about.** kaas-lib's version table is *per
connection*, deliberately: brokers mid-rolling-upgrade genuinely disagree, and a
cluster-wide table would be wrong during exactly the window when being right
matters. So there is no `cluster.capabilities()` to project from, and kaas-ui
must not invent one by picking a broker arbitrarily — that produces a UI whose
tabs flicker depending on which connection answered. §9 item 1 is the fix.

Side effect worth having: put kaas and Strimzi side by side and the differing
tab sets are a visual conformance report.

### The fleet view

The landing page is a fleet dashboard, not a cluster picker: one card per cluster
with reachability, broker count, topic count, offline and under-replicated
partition counts, snapshot age. Grouped by label (`env`, `kind`). Cheap —
`DescribeCluster` plus `Metadata` per cluster, fanned out concurrently with a
per-cluster timeout so one slow cluster does not delay the page.

Cross-cluster topic search follows: "which of my twelve clusters has a topic
matching `orders-*`" has no good answer today.

---

## 6. Authentication and authorization

### Dex is the only provider

kaas-ui speaks generic OIDC and nothing else. Dex sits in front and terminates
GitHub, Google, Entra, LDAP, SAML or anything else, presenting all of them as one
issuer with a `groups` claim.

The payoff is that kaas-ui contains no provider-specific code at all. GitHub in
particular is *not* an OIDC provider — OAuth Apps issue opaque tokens with no
`id_token`, no discovery document and no groups claim, so direct support would
have meant a second code path with its own REST calls to `/user` and
`/user/orgs`. Dex's GitHub connector does that work and emits `org` and
`org:team` group strings, which is exactly the shape the role mapping below
wants. Adding a second identity source later becomes a Dex config change rather
than a kaas-ui release.

```yaml
auth:
  issuer: https://dex.example.com
  client_id: kaas-ui
  client_secret_file: /etc/kaas-ui/secrets/oidc-client-secret
  redirect_url: https://kaas-ui.example.com/auth/callback
  scopes: [openid, profile, email, groups]
```

Implementation is the `openidconnect` crate against the discovery document, with
PKCE, `state` and `nonce` mandatory and full `id_token` signature verification.
Sessions via `tower-sessions` with an encrypted cookie store —
`SameSite=Lax`, `Secure`, `HttpOnly` — and a server-side store only if forced
logout is needed. RP-initiated logout, which Dex supports.

Document the Dex requirement prominently in the README, since "we only support
OIDC" reads as a limitation until you say that Dex makes it a superset.

### Authorization

Read-only makes this small. Roles map group claims to two grants:

```yaml
roles:
  - name: everyone
    subjects: ["acme-corp"]                # Dex GitHub connector: org membership
    clusters: { env: dev }                 # label selector
    grants: [metadata, messages]

  - name: prod-oncall
    subjects: ["acme-corp:platform"]       # org:team
    clusters: { env: prod }
    grants: [metadata]                     # metadata only — no payloads

  - name: prod-support
    subjects: ["acme-corp:support"]
    clusters: { env: prod }
    grants: [metadata, messages]
    topics: ["public-*"]                   # payload access scoped by pattern
```

`metadata` versus `messages` is the meaningful boundary: browsing topic
configuration is not the same act as reading customer data out of a payload.

Hand-roll this. A matrix over two actions does not need `casbin-rs`, and the
policy file above is more legible than a Casbin model.

Cluster visibility is enforced in the registry lookup, not in the router — a user
without access to a cluster gets 404, not 403, so cluster ids are not enumerable.

### Access audit

Append-only log of `(timestamp, subject, cluster, topic, action, offsets)` for
every message read — SQLite via `sqlx`, or structured JSON on stdout for
shipping elsewhere. This is the log that matters in a read-only tool.

---

## 7. Payload decoding and frontend

### `kaas-ui-serde`

kaas-lib hands over `Bytes`; everything above is ours.

- **Raw / hex / string / JSON** — no dependencies, covers most clusters
- **Avro** — `apache-avro` + `schema_registry_converter`
- **Protobuf** — `prost-reflect`, dynamic decode from a `FileDescriptorSet`
- **JSON Schema** — `jsonschema` for display-time validation
- **`__consumer_offsets`** — decode for display only; group views still go
  through `OffsetFetch`, per kaas-lib's non-goals

Registry is per cluster — a Strimzi cluster with Apicurio and a kaas instance
with none coexist fine. Sniff the Confluent magic byte, fall back to per-topic
config, then raw; always show what was chosen and let the user override it.
Auto-detection that cannot be corrected is worse than none.

**Filtering in two tiers.** Cheap predicates through kaas-lib's `RecordFilter`
(offset, timestamp, partition, key prefix, headers) run before deserialization.
A user JS predicate over the decoded value runs after, in `rquickjs` with a hard
memory cap and an interrupt handler. Never run the JS predicate on a record a
cheap filter could have dropped.

### Frontend

React 19, TypeScript, Vite, Tailwind, shadcn/ui, plus TanStack Query (all request
state), Table v8 + Virtual (every grid), Router (typed, zod-validated search
params — the message-view URL is the shareable artifact), Monaco (schema and
filter editing), and Orval generating the client from utoipa's spec.

Four things to get right:

**Streams do not go in the Query cache.** SSE feeds a capped ring buffer in a
dedicated hook with live/paused controls. Query is for request/response; pushing
a stream into it produces unbounded growth and confusing invalidation.

**Group kinds are a discriminated union.** Four variants with genuinely different
fields — a classic group has a generation and a negotiated assignor; a KIP-848
consumer group has a group epoch, an assignment epoch and a server-chosen
assignor. Mirror the Rust enum via `#[serde(tag = "kind")]` and render four
components. Flattening to one all-optional interface moves the same knowledge
somewhere the compiler cannot check it.

**Cluster identity is always visible.** With twelve clusters in one UI, "which
cluster am I looking at" must be answerable without reading the URL. Persistent
colour-coded cluster chip in the header, hard visual distinction for `env: prod`.

**Degradation has components, not error toasts.** An `UnknownCode` chip for
`ErrorCode::Unknown(i16)`, an `Undescribable` panel for
`GroupDescription::Unrecognized`, an `UnsupportedApi` panel showing both version
ranges. These are the entire UI-side surface of §2's ownership boundary — build
them in M1, not when a 4.4 cluster appears.

---

## 8. Milestones

Each has an acceptance command, per kaas-lib's convention. Integration tests run
against `apache/kafka:4.3.1`, against `kaas`, and — where feasible — against a
Strimzi cluster in kind.

**M0 — skeleton.** Workspace, config, registry, `/health`, embedded frontend,
distroless image. The single `connect_read_only` call site plus its test.
*Acceptance:* boots with three clusters, one nonexistent; fleet view renders
within 2s; the dead cluster shows as unreachable with its `Transport` error; CI
grep finds no `Admin::connect` and no Kafka version literal in the workspace.

**M1 — fleet and cluster views.** `DescribeCluster`, `DescribeLogDirs`,
`DescribeConfigs`, capabilities endpoint, degradation components.
*Acceptance:* three-broker fixture renders per-broker log dirs and configs; kaas
and Apache fixtures side by side render different tab sets; an injected
`Unknown(30000)` renders as a number rather than a generic failure.

**M2 — topics.** List, detail, partitions, configs, replica placement,
under-replicated highlighting.
*Acceptance:* describing 50 topics of which 2 do not exist returns 48 items and
2 errors with a 200; the frontend renders 48 rows and 2 chips.

**M3 — messages.** Tail, then scan over SSE. Raw/string/JSON only.
*Acceptance:* tail of 500 on a 100k-record partition fetches under 5% of its
bytes (assert on kaas-lib's connection counters); a hand-corrupted batch renders
as a malformed row while the scan continues; closing the tab mid-scan returns
connection count and RSS to baseline.

**M4 — auth.** OIDC against Dex, sessions, role mapping, cluster visibility,
`metadata` vs `messages` grants, access audit.
*Acceptance:* login works against a Dex instance with a static-password connector
in CI; a user in no matching role sees an empty fleet; a `metadata`-only user
gets 403 on the messages endpoint and no message tab; every message read appears
in the audit log.

**M5 — consumer groups.** All four kinds including `Unrecognized`. Committed
offsets via `OffsetFetch`, lag against `ListOffsets(LATEST)`, member assignments.
*Acceptance:* a fixture with classic, consumer, share and streams groups renders
all four, the streams group as undescribable rather than as an error.

Lag needs care: end offset minus committed offset per partition, where "no commit
yet", "empty partition" and "zero lag" are three different states that must not
all render as `0`.

**M6 — schema registry.** Per-cluster registry client, Avro/Protobuf/JSON Schema
decode, schema browser with version history, per-topic serializer overrides.

**M7 — the read-only admin surface.** ACL viewer, client quotas, SCRAM user list,
reassignment status via `ListPartitionReassignments`, and the transaction
inspector (`DescribeTransactions`, `ListTransactions`, `DescribeProducers`).

This is where kaas-ui **exceeds** kafbat-ui rather than catching up: kaas-lib
already supports all of it and the JVM tools largely do not surface it. Cheap,
because it is all read.

**M8 — cross-cluster.** Topic search across the fleet, cluster comparison view,
capability diff.

---

## 9. Proposed kaas-lib work

Expansion is on the table, so here is what the UI wants, ordered by
value-to-effort. All read-side — nothing here needs a producer.

**1. Cluster-level capability aggregation.** The per-connection version table is
correct and should stay that way, but a UI needs a cluster-level answer and must
not fabricate one by picking a broker. The right shape preserves disagreement
rather than collapsing it:

```rust
// on Cluster — needs pool access, so it cannot live in kaas-ui
pub enum ApiSupport {
    Agreed(Option<VersionRange>),
    Disagreed(BTreeMap<BrokerId, Option<VersionRange>>),  // rolling upgrade
}
pub fn capabilities(&self) -> BTreeMap<ApiKey, ApiSupport>;
```

`Disagreed` is the interesting variant: rendering "3 of 5 brokers support this,
upgrade in progress" is honest and useful, and it is invisible to every other
Kafka tool. This is item 1 because capabilities drive the entire UI and there is
currently no correct way to compute them.

**2. Batched `FindCoordinator` (KIP-699).** Your own docs flag it: "worth
revisiting for a UI rendering hundreds of groups at once." A groups page on a
cluster with 300 groups currently costs 300 round trips on a cold cache. Biggest
UI-visible performance item, contained change to `kafka-meta`.

**3. Multi-group `OffsetFetch`.** v8+ takes several groups per request. With (2),
the groups list goes from O(n) round trips to roughly O(1). Same page, same
motivation.

**4. `DescribeTopicPartitions` cursor pagination.** A cluster with thousands of
topics should not need one enormous `Metadata` response to render page one.
Check whether the cursor is already exposed; if not, expose it.

**5. SASL OAUTHBEARER.** Strimzi with Keycloak is a mainstream deployment and
cannot connect today. Also unlocks Confluent Cloud OAuth and, with a token
provider hook, MSK IAM later. Largest item — token acquisition, refresh, and the
interaction with KIP-368 re-authentication — and the one most likely to be a
hard blocker for a real user.

Note the pleasing symmetry if this lands: Dex terminates OIDC for *users*,
OAUTHBEARER terminates OAuth for *clusters*, and the same Keycloak can serve
both.

**6. `DescribeQuorum` (51).** Your api-matrix already calls it "the one in this
group a UI might plausibly want." A KRaft quorum panel — voters, leader, lag — is
directly useful for both kaas and Strimzi KRaft clusters.

**7. Pool and connection introspection.** kaas-ui wants "connected to 2 of 3
brokers" plus per-broker connection age and byte counters. The counters exist in
`stats.rs`; the pool's state is not exposed. Pairs naturally with (1), since both
need the same pool access.

**8. `ListConfigResources` (70).** Enumerate config resources rather than
guessing at what exists.

**9. Upstream contributions to `kafka-protocol`.** `StreamsGroupDescribe`
(KIP-1071) and `ListOffsets` v11 are blocked on schemas rather than on kaas-lib.
Contributing upstream is the option your own docs rank above every workaround,
and streams groups appear on any Strimzi cluster running Kafka Streams.

Not needed: producer, group membership, incremental fetch sessions. A read-only
kaas-ui never wants any of them, so M12–M19 can be sequenced entirely on their
own merits.

---

## 10. Packaging, dogfooding, risks

**Packaging.** rustls with `ring`, no C toolchain, no librdkafka — a genuinely
static musl binary, embedded frontend via `rust-embed`, distroless image. Expect
~25 MB and ~15 MB RSS at idle against the JVM original's several hundred. That
win exists *because* kaas-lib is pure Rust; put the numbers in the README.

**Distribution.** The image publishes to `ghcr.io/kaas-rs/kaas-ui`, alongside
`ghcr.io/kaas-rs/charts` where the kaas broker's chart already lives —
cosign-signed, `latest` moving only on a semver tag. CI and releases run on
`arc-runner-set-ui`, a repo-scoped ARC scale set on the same k3s cluster the UI
talks to, so the acceptance suite reaches both brokers over service DNS instead
of over a container fixture. Deployment is GitOps: an `apps/kaas-ui/` directory
in the cluster repo, picked up by the ArgoCD ApplicationSet that already
discovers `apps/*`. See `docs/10-release-and-deployment.md`.

**Dogfooding.** Point kaas-ui at `kaas` alongside Strimzi and the UI becomes a
conformance surface: a wrongly-shaped response renders as a wrong page, and the
capability diff between the two is visible on one screen. The trap kaas-lib
already names still applies — kaas-ui must not become a place where broker and
client share code or assumptions. It depends on kaas-lib only and talks to kaas
over the wire like any other cluster.

**Risks.**

*OAUTHBEARER is the likely hard blocker.* SCRAM, PLAIN and mTLS cover kaas and
most Strimzi deployments — mTLS is Strimzi's default — but Strimzi-plus-Keycloak
is common enough that §9 item 5 may become urgent rather than optional.

*Capability correctness before capability rendering.* Building tab-rendering on a
capability table computed from one arbitrary broker will look fine on every
single-broker fixture and misbehave on the first rolling upgrade. Either do §9
item 1 first or render capabilities from an explicitly-named broker and say so in
the UI until it lands.

*Fan-out cost grows with the fleet.* Twelve clusters polling in the background is
twelve metadata refreshes plus whatever the open page requests. Per-cluster
concurrency limits and "pause background refresh for clusters not on screen" are
worth building before the fleet gets large, not after.

*Read-only is a product decision to hold.* The first request after launch will be
"can it just delete this one topic". The value is that no code path exists and no
mutation audit is needed — one write endpoint forfeits that permanently.
