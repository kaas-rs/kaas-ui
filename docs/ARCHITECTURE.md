# Architecture

kaas-ui is one binary: an Axum HTTP server with the React frontend embedded in
it. It serves a **fleet** of Kafka clusters, the schema registries they decode
against, and the Kafka Connect clusters beside them. Its only Kafka dependency
is [kaas-lib](https://github.com/kaas-rs/kaas-lib), from crates.io.

```
 browser ──HTTP──▶ kaas-ui-server ─────────────────────────────────────────────
                    │  /health   /auth/*   /dex/* (proxy)   /api/*   assets
                    │
                    ▼
                 AppState ── Policy (roles) ── Provider (OIDC) ── Audit (stdout)
                    │
                    ▼
                 Registry  (ArcSwap, swapped whole on config reload)
                    ├── ClusterHandle   ──kaas-lib Admin──▶ Kafka brokers
                    ├── RegistryHandle  ──HTTP (ccompat)──▶ schema registry
                    └── ConnectHandle   ──HTTP (REST)─────▶ Connect workers
```

Security is covered in [RBAC.md](RBAC.md). The HTTP envelope, error mapping and
endpoint table are in [reference/http-contract.md](reference/http-contract.md).

## The fleet model

Configuration is a list of **environments**. Each one holds its Kafka clusters,
its schema registries, its Connect clusters, and inventory that kaas-ui lists
but does not connect to:

```yaml
environments:
  - id: dev
    schema_registries: [{ id: apicurio, url: … }]
    kafka_clusters:
      - id: kaas
        bootstrap: [kaas.kaas.svc.cluster.local:9095]
        schema_registry: apicurio        # a reference, not membership
    connect_clusters:
      - id: connect
        url: http://…:8083
        kafka_cluster: kaas              # where the workers keep their state
    resources: [{ id: mqtt, kind: mqtt_broker, … }]
```

Three properties follow from it:

- **Everything is addressed `(environment, id)`.** An id is unique only within
  its environment, so `kafka` in `dev` and `kafka` in `prod` are two clusters.
  The URL space mirrors the config:
  `/api/environments/{env}/clusters/{id}/…`,
  `/api/environments/{env}/schema-registries/{registry}/…`,
  `/api/environments/{env}/connect-clusters/{connect}/…`.
- **Nesting is membership.** A cluster belongs to `dev` because it is declared
  there. Its `env` label comes from that nesting, so a role's
  `cluster_labels: {env: prod}` cannot disagree with the config.
- **References are checked at startup.** A `schema_registry:` or
  `kafka_cluster:` reference must name something in the same environment.
  Unknown keys are rejected, and so is a config in the old pre-nesting shape.

A schema registry serves an environment, not a cluster. Two clusters that
reference `apicurio` hold the same `Arc<RegistryHandle>`, so they share one set
of decoders and one id→schema cache.

## Crates

The dependency order, lowest first: `kaas-ui-auth` and `kaas-ui-serde`, then
`kaas-ui-connect`, then `kaas-ui-core`, then `kaas-ui-api`, then
`kaas-ui-server`.

| crate | what it owns | depends on |
|---|---|---|
| `kaas-ui-auth` | Identity (`Principal`), roles (`Policy` → `Access`), the write gate (`Grant`), the audit log and the OIDC exchange. It names no identity provider. | nothing in the workspace |
| `kaas-ui-serde` | Payload decoding: Confluent framing, the ccompat registry client, Avro, Protobuf and JSON Schema codecs, subject naming. | nothing in the workspace |
| `kaas-ui-connect` | The Connect REST client. Reads go through one private `get_json`, and `writer.rs` is the only file that sends a non-GET. Connector configs are redacted before any public type holds them. | auth, serde |
| `kaas-ui-core` | Config, the registry and its handles, lazy connection, capability projection, DTOs, admin describes, topic analysis, sizing advice, TLS inspection. It has no HTTP code. | auth, serde, connect, kaas-lib |
| `kaas-ui-api` | The router, handlers, the `Caller` extractor, the error and envelope mapping, SSE streaming, and the OpenAPI document. It never opens a socket itself. | all of the above |
| `kaas-ui-server` | `main`: load config, build the registry, start connectors, watch the config file, discover OIDC, proxy Dex, serve the embedded frontend. | api, auth, core |
| `xtask` | `ci`, `live`, `login`, `docs`, `link`/`unlink`. | — |

`kaas-ui-auth` has no workspace dependencies on purpose. `kaas-ui-core`'s
registry lookups take an `Access`, so auth sits below core. Anything in auth
that starts to need a cluster type belongs in core.

### The boundary with kaas-lib

kaas-ui uses four kaas-lib crates — `kafka-conn`, `kafka-meta`, `kafka-admin`
and `kafka-read` — at the same version. Two rules govern that boundary:

- **No upstream type in a public signature.** `kaas-ui-core::dto` defines every
  shape that reaches the wire, and its `From` impls are the only place a
  `kafka_meta` or `kafka_admin` type is converted. A kaas-lib bump therefore
  never changes the TypeScript client.
- **No Kafka version number.** kaas-lib owns version negotiation, `Unknown`
  arms, `UnsupportedApi` with both version ranges, and fallbacks such as
  `DescribeTopicPartitions` → `Metadata`. kaas-ui only decides how an absence
  is presented. When kaas-ui needs to know what a Kafka release added, that
  knowledge goes into kaas-lib, filed in
  [reference/upstream-asks.md](reference/upstream-asks.md).

## Startup and lifecycle

`kaas-ui-server` starts serving before it connects to anything. A fleet of
twelve clusters with one of them down boots as fast as a fleet where all are up.

1. **Load config.** YAML plus a `KAAS_UI_*` environment overlay (figment).
   Credentials missing from the file are read from
   `KAAS_UI_<CREDENTIAL>_<ENVIRONMENT>_<ID>`. PEM material stays in files.
   `--check` loads, reports and exits. `--openapi` prints the spec.
2. **Build the registry.** One `ClusterHandle` per Kafka cluster, one
   `RegistryHandle` per schema registry, one `ConnectHandle` per Connect
   cluster. Nothing is dialled yet.
3. **Spawn connectors.** Each Kafka cluster gets a background task that calls
   `Admin::connect_read_only`, which fetches a snapshot before it returns.
   A failure is recorded as `ClusterHealth::Unreachable` and retried with
   backoff from 1 s to 60 s. Snapshot age is checked every 10 s. Schema
   registries and Connect clusters are dialled lazily, on first use.
4. **Watch the config.** The file is re-read every 5 s, compared by bytes
   rather than watched with inotify, because Kubernetes swaps ConfigMap
   symlinks atomically and an inode watch never fires. A change builds a new
   registry that reuses the `Arc` of every unchanged handle, so existing
   connections are not disturbed, and swaps it in. An invalid edit is logged
   and the previous registry stays. `roles:` and `auth:` are read only at
   startup.
5. **Resolve the policy.** No `roles:` makes the deployment open. Otherwise
   roles are enforced. See [RBAC.md](RBAC.md#deployment-modes).
6. **Discover OIDC** if `auth:` is configured. This happens at startup so that
   a wrong issuer fails the boot rather than someone's first login. The
   session key is generated per process.
7. **Serve.** On shutdown a latch ends open SSE streams so the drain can
   complete.

`/health` never consults a cluster. If a liveness probe failed whenever someone's
broker was down, Kubernetes would restart a healthy process.

## A request, end to end

Take `GET /api/environments/dev/clusters/kaas/topics/orders` as the example:

1. **Route.** The `/api` router matches it. A non-GET would first pass
   `write_guard`, which requires `X-Kaas-Ui: 1` and checks `Origin`.
2. **Caller.** The `Caller` extractor decrypts the session cookie into a
   `Principal` and an `Access`. A missing or expired cookie gives the anonymous
   caller. Handlers cannot reach a cluster without a `Caller`.
3. **Lookup.** `AppState::cluster(env, id, caller)` is the only path to a
   handle. A cluster the caller cannot see gets the same `404` as one that does
   not exist. `AppState::connected` adds `503`, with the last error, for a
   cluster that has not connected yet, and nudges its connector to retry.
4. **Permission.** `caller.require(…)` checks the action on the resource and
   name. Failure is `403`; the cluster is already known to be visible, so
   confirming it exists reveals nothing.
5. **Call kaas-lib** and convert to DTOs.
6. **Envelope.** A per-item result (`PerItem<K, T>`) stays per item. Describing
   50 topics where 2 do not exist returns `200` with 48 items and 2 errors.
   Upstream errors map to status codes in one place (`kaas-ui-api/src/error.rs`).
   For example, a cluster rejecting kaas-ui's SASL credentials is `502`, never
   `401`, so the user is not logged out.

Payload reads add one more step: the disclosure is written to the audit log
**before** the bytes leave the process. If the audit line cannot be written,
the request fails.

## Messages

The message browser has four routes: `tail`, `page`, a single record, and
`stream` (SSE). They share one decode path.

- **Decoding** is in `kaas-ui-serde`. The magic byte decides only whether a
  payload is registry-framed. If it is, the registry says whether schema 42 is
  Avro, JSON Schema or Protobuf. If it is not, the per-topic codec applies,
  defaulting to `auto`. The raw bytes travel beside the decoded value, so
  switching to hex or string needs no refetch. Switching *to* Avro without a
  schema id is refused.
- **Filtering** is a literal substring match on the decoded value, done in
  `PayloadDecoder::accept`. It is never a pattern or an expression. A filtered
  page's `limit` is a budget for records *read*, and `nextOffset` comes from
  the window examined, so a page that matched nothing still says where to look
  next.
- **Streaming** batches rows into one SSE event every 100 ms. A drop-oldest
  hand-off sits between the scan and the writer, so a slow reader loses old
  rows rather than stalling the fetch. A governor caps open streams at 5 per
  caller and 50 in total. Malformed batches are a row type inside `messages`
  rather than an `error` event.
- **The frontend** keeps streamed rows out of the TanStack Query cache. They go
  into a capped ring buffer in their own hook.

## Writes

Writes are constructed rather than permitted: every mutating client method
takes a `&Grant`, and only the gate in `kaas-ui-auth` mints one. The rules are
in [RBAC.md](RBAC.md#writes). Here is how they are enforced in the code:

- **Kafka Connect** has the only writes built so far.
  `kaas-ui-api/src/routes/connect_write.rs` runs every write through one
  `perform` sequence: lookup, gate, typed confirmation, audit intent, write,
  audit outcome. `ConnectWriter` (in `kaas-ui-connect/src/writer.rs`) is
  constructed from a `Grant` and checks that each call matches the grant's
  operation and connector.
- **Kafka** has no writes yet. The workspace's one admin connection is the
  `Admin::connect_read_only` in `kaas-ui-core/src/registry.rs`, and
  `cargo xtask ci` checks that it is the only construction site. The first
  Kafka write needs kaas-lib's mutation ceiling (upstream asks 17–18) to build
  a connection from the cluster's `writes.allow`. That change must update the
  check in `xtask/src/checks.rs` in the same commit.

If a write somehow got past the gate, kaas-lib refuses mutating API keys on a
read-only connection, which surfaces as `Error::ReadOnly`. That reaching a user
means kaas-ui built a request its own ceiling forbade. It is a `500`, not a
`403`.

## Capabilities and degradation

The two development clusters differ widely (40 API keys against 75), and the UI
is built to show that difference rather than hide it.

- **The capability table is per connection.** kaas-ui projects kaas-lib's
  version table for a named broker and says which one in the UI. It never
  fabricates a cluster-wide answer.
- **Absence is presentation.** A missing API becomes a hidden tab, a greyed tab
  or an explanatory panel (`CapabilityTab`). `UnsupportedApi` carries both
  version ranges, which tells three cases apart: our codec is behind, the
  broker lacks the API, or the cluster is behind.
- **Failure is isolated.** An unreachable cluster, registry or Connect cluster
  degrades only the pages that touch it.

## Frontend

`web/` is React 19, TanStack Router and Query, Tailwind 4 and Base UI, built
with Vite.

- **Routes mirror the API.** `router.tsx` follows the fleet's shape:
  environment, then cluster, registry or Connect cluster, then the thing
  inside it.
- **The API client is hand-written**, in `api/types.ts` and `api/client.ts`,
  against `docs/openapi.json` (regenerated by `cargo xtask docs`). Request and
  response data goes through TanStack Query. Polling intervals are chosen per
  screen: Connect polls, the admin screens do not.
- **Permissions shape the UI.** Each cluster card carries `grants`, and the
  sidebar and tabs hide what the caller may not view. A connector carries its
  `writes` options, and write buttons are drawn from them. Every write goes
  through `components/write-dialog.tsx`.
- **Design.** The palette is mdbook's `rust` theme with semantic tokens and a
  derived dark mode; see [reference/design-system.md](reference/design-system.md).

The built bundle (`web/dist`) is embedded with `rust-embed` at compile time.
`index.html` is rewritten when served, so one build works under any path
prefix a proxy mounts it at.

## Invariants CI enforces

`cargo xtask ci` runs fmt, clippy, the unit tests, and these greps
(`xtask/src/checks.rs`):

| check | what it holds |
|---|---|
| one construction site | exactly one `Admin::connect_read_only(`, no `Admin::connect(` |
| no Kafka version literal | rule 2 |
| no local kaas-lib override | the `cargo xtask link` fence is not committed |
| login is a navigation | `/auth/login` stays a top-level `GET` so `SameSite=Lax` works |
| consuming builders | every optional setter is `#[must_use] with_*` (`STYLE.md`) |
| Connect client only reads outside `writer.rs` | no non-GET spelled elsewhere in `kaas-ui-connect` |
| one `Grant` constructor | `Grant {` is built only in `kaas-ui-auth/src/gate.rs` |

The OpenAPI test pins the exact set of non-GET paths. `unwrap`, `expect`,
`panic!` and indexing are denied workspace-wide: one malformed record must not
take down a server hosting a dozen clusters.

Unit tests need no cluster. `cargo xtask live` runs acceptance against both
real clusters (and a dead one), and `cargo xtask login` performs a real OIDC
login.

## Build and deployment

The `Dockerfile` has three stages:

1. `node` builds `web/dist`.
2. `rust` (alpine) builds the binary, embedding `web/dist`. This stage must
   come after the frontend build, or the image serves 404s.
3. The binary is copied onto `distroless/static` as nonroot.

Images publish to `ghcr.io/kaas-rs/kaas-ui`. Deployment is GitOps through
`Woestebanaan/k3s-cluster` (`apps/kaas-ui/`). ArgoCD auto-sync is off, so a
merged change is only applied after a manual `argocd app sync`.
