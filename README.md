# kaas-ui

A **read-only**, multi-cluster Kafka UI in the shape of kafbat-ui: an Axum
backend on [kaas-lib](https://github.com/kaas-rs/kaas-lib), a React + TanStack
frontend, one binary.

Read-only is the architecture, not a setting. There is exactly one
`Admin::connect_read_only` in the workspace and no mutating endpoint anywhere —
not disabled, not `403`, **absent from the router**. CI greps for both, and for
a Kafka version number, and for a `.post(`.

- [`PLAN.md`](PLAN.md) — the design. Why read-only, why multi-cluster, why Dex.
- [`docs/`](docs/README.md) — the implementation plan and its reference material.

## Running it

```sh
cargo run -p kaas-ui-server -- --config config.dev.yaml
```

Configuration is YAML plus a `KAAS_UI_*` environment overlay. Unknown keys are
rejected rather than ignored:

```yaml
server:
  listen: "0.0.0.0:8080"

clusters:
  - id: strimzi
    name: kafka-cluster (Strimzi)
    bootstrap: ["kafka-cluster-kafka-bootstrap.strimzi.svc.cluster.local:9092"]
    labels: { env: dev, kind: strimzi }
    refresh_interval: 30s
```

`--check` loads the configuration and reports what it says without serving.
`--openapi` prints the OpenAPI document and exits; the running server serves
the same document at `GET /api/openapi.json`.

The document is not rendered in the app. There is no Swagger UI either — it
would have cost 2–3 MB on a 5 MB image for a try-it console that, on an API
with one verb and no request bodies, is a link. Point whatever reads OpenAPI
at `/api/openapi.json`; `cargo xtask docs` writes the same document to
`docs/openapi.json`.

The file is re-read every five seconds. A change **swaps the registry**,
reusing the handle of every cluster that did not change — adding a cluster does
not disturb the connections of the eleven that did not move.

### The frontend

```sh
cd web && npm ci && npm run dev     # proxies /api to a locally running binary
cd web && npm run build             # what the release binary embeds
```

`rust-embed` pulls `web/dist` in at **compile** time, which is why the
frontend stage of the container build runs first.

### Behind a path prefix

kaas-ui serves from `/` and that is the normal case. To host it somewhere else
— a reverse proxy mounting it at `/kafka`, or code-server's `/proxy/8099` —
tell the server, and change nothing else:

```yaml
server:
  base_path: "/kafka"
```

or `KAAS_UI_SERVER__BASE_PATH=/kafka`. **The frontend is not rebuilt for this.**
One `npm run build` works at every prefix: the binary rewrites `index.html` as
it serves it, pointing the asset URLs at the prefix and adding a `<base>`
element that the router's `basepath` and every URL built in JavaScript read
back. A bundle compiled for one deployment would 404 its own assets in any
other, which is a trap worth not setting.

The prefix has to be *told* because it cannot be detected: a stripping proxy
forwards no record of what it removed — code-server sends no
`X-Forwarded-Prefix` and rewrites `Host` to its own — so the arriving request
is indistinguishable from one made at the root.

**The proxy must strip the prefix**, which code-server and a rewriting ingress
both do: the browser asks for `/kafka/api/clusters` and the server is handed
`/api/clusters`. kaas-ui's own routes are always rooted at `/`.

The dev server has no binary in front of it to do the rewriting, so there it is
still a build-time flag — `npm run dev:proxy`, or `VITE_BASE=… npm run dev`.

One caveat that bites this deployment specifically: **a path proxy that buffers
`text/event-stream` will break the live message view** — the stream appears to
work while arriving in stale bursts. kaas-ui's own compression layer declines
SSE, but nothing here can make somebody else's proxy behave. Port-forwarding to
`localhost` avoids the question entirely, and is the better dev loop.

## Verification

```sh
cargo xtask ci      # fmt + clippy + unit tests + the four invariant checks
cargo xtask live    # phase acceptance, against both live clusters
cargo xtask docs    # write docs/openapi.json
```

`cargo xtask ci` runs anywhere: unit tests are cluster-free and Docker-free.
Anything that needs a broker lives in `live`, which starts the real binary
against the real clusters and asserts over HTTP — 50 assertions, including that
an unreachable cluster costs the fleet request nothing measurable, that
abandoning a message stream releases it, and that a shutdown with streams open
drains in milliseconds rather than waiting for bodies that never end.

There is no `cargo xtask integration` and no `testcontainers`: Docker is not
available in the environment this is developed in, and two real three-node
clusters are a better target than one container anyway.

## What is built

| | |
|---|---|
| **fleet** | one card per configured cluster, grouped by label, with reachability, counts and snapshot age |
| **cluster** | brokers, controller, log dirs per broker, `DescribeCluster` where it exists |
| **capabilities** | the feature projection and the whole api-version table, naming the broker that answered |
| **topics** | server-side filtered/sorted/paged list, partitions, replica placement grid, configs, offsets |
| **messages** | the tail of a topic, and a live viewer: seven seek modes over SSE, a virtualized list, a detail panel, and the whole view in the URL |
| **groups** | the four group kinds, members, committed offsets and lag in its four states |

Not built yet: OIDC/Dex and roles, schema registry, the read-only admin views
(ACLs, quotas, SCRAM, reassignments, transactions) and the cross-cluster views.
See [`docs/README.md`](docs/README.md) for what each phase covers.

The message viewer runs on **kaas-lib 0.2.0**. Three things it needed went in
there rather than here, because version and implementation knowledge belongs in
the library: the anchored backward walk, `ScanSpec::following`, and a fix for
`scan` emitting records before its start offset. See
[Phase 3](docs/04-phase-3-messages.md).

## Measurements

The rewrite's headline claim is that this is a small process next to the JVM
original. Numbers rather than adjectives:

| | | predicted in PLAN.md §10 |
|---|---|---|
| container image, distroless + static musl | **5 MB** | ~25 MB |
| resident memory, idle, three clusters configured | **8.7 MiB** | ~15 MB |
| `GET /api/clusters` with a dead cluster configured | ~1.2 ms | — |
| `GET /health` | ~0.3 ms, and it never consults a cluster | — |

The image size is measured by the release job on every push and written into
its summary, so the number above is checked rather than remembered.

## Deployment

Images publish to `ghcr.io/kaas-rs/kaas-ui`. Deployment is GitOps via
`Woestebanaan/k3s-cluster` (`apps/kaas-ui/`).

Public access is the cluster's Cloudflare tunnel at **`kaas.smeding.cloud`** —
a rule in `apps/cloudflare/values.yaml`, not an Ingress or an HTTPRoute. Only
the apex is routed through Traefik, so a Gateway API route would report healthy
and never be reachable.

**ArgoCD auto-sync is off in that cluster.** A merged commit registers a change;
applying it is a manual `argocd app sync kaas-ui`. A green pipeline is not
evidence that the cluster changed.

## Licence

Apache-2.0.
