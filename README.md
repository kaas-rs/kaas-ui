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
`--openapi` prints the OpenAPI document and exits.

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

## Verification

```sh
cargo xtask ci      # fmt + clippy + unit tests + the four invariant checks
cargo xtask live    # phase acceptance, against both live clusters
cargo xtask docs    # write docs/openapi.json
```

`cargo xtask ci` runs anywhere: unit tests are cluster-free and Docker-free.
Anything that needs a broker lives in `live`, which starts the real binary
against the real clusters and asserts over HTTP — 27 assertions, including that
an unreachable cluster costs the fleet request nothing measurable.

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
| **messages** | the tail of a topic, merged across partitions, keys and values rendered with the encoding said out loud |
| **groups** | the four group kinds, members, committed offsets and lag in its four states |

Not built yet: the SSE scan half of the message browser, OIDC/Dex and roles,
schema registry, the read-only admin views (ACLs, quotas, SCRAM,
reassignments, transactions) and the cross-cluster views. See
[`docs/README.md`](docs/README.md) for what each phase covers.

## Measurements

The rewrite's headline claim is that this is a small process next to the JVM
original. Numbers rather than adjectives:

| | |
|---|---|
| container image | measured on every release and written into the job summary |
| resident memory, idle, three clusters | see the table in `docs/01-phase-0-skeleton.md` |
| `GET /api/clusters` with a dead cluster configured | ~1 ms |
| `GET /health` | ~0.3 ms, and it never consults a cluster |

## Deployment

Images publish to `ghcr.io/kaas-rs/kaas-ui`. Deployment is GitOps via
`Woestebanaan/k3s-cluster` (`apps/kaas-ui/`).

**ArgoCD auto-sync is off in that cluster.** A merged commit registers a change;
applying it is a manual `argocd app sync kaas-ui`. A green pipeline is not
evidence that the cluster changed.

## Licence

Apache-2.0.
