# Phase 0 — skeleton

*PLAN.md milestone M0.*

**Goal.** A binary that boots with three configured clusters, connects to none
of them eagerly, serves a fleet dashboard from the embedded frontend, and
renders the dead one as unreachable without making anyone wait for it.

Everything after this phase is filling in screens. This phase is where the
invariants get built so that filling in screens cannot break them.

## What gets built

### `crates/kaas-ui-core`

```
src/
  lib.rs
  config.rs      figment: YAML + env overlay
  registry.rs    ClusterHandle, lazy connect — the ONLY connect_read_only call site
  health.rs      per-cluster health state machine
  dto.rs         owned DTOs + From<kafka_*> conversions
  envelope.rs    Envelope<T>, ResourceError, PerItem -> Envelope
  error.rs       kafka_conn::Error -> (StatusCode, ResourceError)
```

**`config.rs`** — the shape in PLAN.md §5, loaded with `figment` from a YAML
file plus an env overlay. TLS and schema-registry blocks are parsed and
validated now even though nothing reads them until later phases; a config file
that silently ignores a `tls:` block is worse than one that rejects it.

Certificates are **file paths, not inline PEM**, because Strimzi delivers the
cluster CA and KafkaUser credentials as mounted Secrets, and because a PEM
inlined in YAML loses its newlines in the first careless edit.

**`registry.rs`** — the heart of the phase.

```rust
pub struct ClusterHandle {
    pub id: ClusterId,
    pub name: String,
    pub labels: BTreeMap<String, String>,
    admin: ArcSwapOption<Admin>,       // None until first successful connect
    pub health: ArcSwap<ClusterHealth>,
}

pub struct Registry { clusters: BTreeMap<ClusterId, Arc<ClusterHandle>> }

impl Registry {
    pub fn get(&self, id: &str) -> Option<&Arc<ClusterHandle>>;   // 404, not 403
    pub fn all(&self) -> impl Iterator<Item = &Arc<ClusterHandle>>;
}
```

Three rules, each of which is a bug if broken:

1. **Connect lazily.** `main` must not connect to anything. One unreachable
   cluster must not block startup, hang `/health`, or slow a page that does not
   touch it. Eager connection is the easy mistake and it makes a
   twelve-cluster deployment unbootable whenever one cluster is down.
2. **Isolate failures.** A connect attempt that fails records a
   `ClusterHealth::Unreachable { error, since, attempts }` and schedules a
   retry with backoff. It never propagates out of the handle.
3. **One construction site.** Exactly one `Admin::connect_read_only` in the
   workspace, here. No `Admin::connect` anywhere. Enforced by both a unit test
   and a CI grep, because the grep alone is defeated by a rename and the test
   alone is defeated by a second call site nobody tested.

**`dto.rs`** — owned types, converted at the boundary. `kaas_ui_core::dto::Broker`,
not `kafka_meta::BrokerInfo`. Rule 1 one level up.

### `crates/kaas-ui-api`

```
src/
  lib.rs
  routes/
    health.rs      GET /health
    clusters.rs    GET /api/clusters
  openapi.rs       utoipa spec assembly
```

`GET /api/clusters` fans out over the registry **concurrently, with a
per-cluster timeout**, and returns one card per configured cluster whether or
not it is reachable. It reads `Cluster::snapshot()` and nothing else — see
below.

`GET /health` returns 200 as long as the process is alive. It must not consult a
cluster: a liveness probe that fails because a Kafka cluster is down is a
liveness probe that restarts a healthy process.

### `crates/kaas-ui-server`

The binary. Config load, tracing init, registry construction, router assembly,
`rust-embed` static serving with an SPA fallback to `index.html`, graceful
shutdown.

### `web/`

Vite + React 19 + TypeScript + Tailwind 4 + TanStack Query + TanStack Router.
One route: the fleet dashboard. One component that matters: the cluster card.

Cards are grouped by label (`env`, then `kind`) and show: reachability, broker
count, topic count, offline and under-replicated partition counts, snapshot age.
Unreachable cards show the `Transport` error text and a retry button.

**The design tokens land here, in full, before the first component.**
[reference/design-system.md](reference/design-system.md) is the mdbook `rust`
palette — the same colours as the kaas-lib book — expressed as a Tailwind 4
`@theme` block. Writing it once now is an hour; retrofitting semantic tokens
over components that hardcoded hexes is a week, and the intermediate state looks
like two products.

**Cluster identity is visible from the first commit.** `ClusterChip` in the
header, colour derived deterministically from the cluster id, with `env: prod`
overriding to the danger tone. With twelve clusters in one UI, "which cluster am
I looking at" must be answerable without reading the URL — and retrofitting that
later means touching every component.

### `xtask/`

`ci`, `live`, `link`, `unlink`, `docs`. See
[00-foundations.md](00-foundations.md).

### Packaging and CI

Three-stage build — frontend, then a static musl binary with rustls/ring, then
`FROM gcr.io/distroless/static`. Record the image size and idle RSS in the
README; PLAN.md §10 promises ~25 MB and ~15 MB, and the number is only worth
claiming if it is measured.

CI runs on `arc-runner-set-ui`, a **new** ARC scale set that has to be added to
the cluster repo first — the existing two are repo-scoped to `kaas` and
`kaas-lib` and cannot be reused. Full details, including the ARC image's missing
C toolchain and the fork-PR runner guard, are in
[10-release-and-deployment.md](10-release-and-deployment.md).

The GitOps manifests are written in this phase but **not committed**: the
ApplicationSet in the cluster repo auto-discovers `apps/*`, so committing them
before an image exists in GHCR deploys an `ImagePullBackOff`.

## The decision this phase turns on

**The fleet view reads `Cluster::snapshot()` and nothing else.**

PLAN.md §5 describes the fleet card as "`DescribeCluster` plus `Metadata` per
cluster". Against the actual target clusters that does not work:

```
kaas → describe_cluster: no usable version of DescribeCluster:
       broker offers None, we speak Some((0, 2))
```

`kaas` does not implement api key 60. The snapshot, by contrast, carries
brokers, controller id, cluster id, every topic and every partition's
replicas/ISR/offline set — everything a fleet card renders — and it works on
both clusters today.

So Phase 0 uses the snapshot alone, and [Phase 1](02-phase-1-fleet-and-capabilities.md)
adds `describe_cluster` as *enrichment* that degrades visibly when absent. This
is not a workaround: it is PLAN.md §2's rule applied honestly. The only thing
`describe_cluster` adds over the snapshot is `is_fenced` per broker, and one
optional field is not worth making the landing page depend on an api key half
the fleet does not answer.

See [reference/upstream-asks.md](reference/upstream-asks.md) item 0 for the
kaas-lib change that would remove the choice.

## Traps

- **`refresh_interval` defaults to 30s per cluster.** Twelve clusters is twelve
  background metadata refreshes forever, whether or not anyone is looking. Set
  it per cluster from config now, and add "pause background refresh for
  clusters not on screen" before the fleet gets large rather than after
  (PLAN.md §10).
- **`Cluster::connect` fetches the first snapshot before returning**, so the
  first `connect_read_only` for an unreachable cluster blocks for the connect
  timeout. That is the whole reason connection is lazy and off the request path.
- **Config reload must swap the registry, not mutate it.** Adding a cluster
  should not disturb the connections of the eleven that did not change. `notify`
  watches the file; the reload builds a new `BTreeMap` reusing existing
  `Arc<ClusterHandle>`s by id.
- **Do not put `kafka_meta::TopicInfo` in a utoipa schema.** The generated
  TypeScript client would then break on every kaas-lib bump.

## Acceptance

```sh
cargo xtask ci
```

- fmt, clippy (with the workspace deny list), unit tests all pass;
- grep finds no `Admin::connect(`, exactly one `Admin::connect_read_only(`;
- grep finds no Kafka version literal anywhere in the workspace;
- grep finds no `xtask link` fence in `Cargo.toml`;
- a unit test asserts the single call site independently of the grep.

```sh
cargo xtask live --config config.dev.yaml
```

With the three clusters from
[reference/environment.md](reference/environment.md) — `kaas`, `strimzi`, and
`dead` pointing at `nowhere.invalid:9092`:

- the process is serving within 2s of start, having connected to nothing;
- `GET /health` returns 200 in under 10ms while `dead` is still failing;
- `GET /api/clusters` returns three entries **within 2s**, of which two report
  broker and topic counts matching `livetest probe` for that cluster and one
  reports `unreachable` with its `Transport` error attached;
- the response time for `GET /api/clusters` with `dead` present is within 200ms
  of the response time with it removed — the dead cluster costs a timeout on a
  background task, not on the request;
- the frontend renders three cards, grouped, with the dead one visually
  distinct and offering a retry.

## Exit criteria

- [ ] `cargo xtask ci` green, including all four greps and the call-site test
- [ ] `cargo xtask live` green against `kaas` + `strimzi` + `dead`
- [ ] fleet page renders in a browser from the embedded assets, not from Vite dev
- [ ] distroless image builds; size and idle RSS recorded in the README
- [ ] config reload adds and removes a cluster without dropping connections to others
- [ ] `docs/` updated where this phase changed a decision
