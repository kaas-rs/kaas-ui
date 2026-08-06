# CLAUDE.md

## What this is

A **read-only** Kafka cluster UI in the shape of kafbat-ui: an Axum backend on
[`kaas-lib`](https://github.com/kaas-rs/kaas-lib), a React/TanStack frontend,
multi-cluster from day one, OIDC via Dex.

- `PLAN.md` — the design. Why read-only, why multi-cluster, why Dex. Stable.
- `docs/` — the implementation plan. Phases 0–8 (= PLAN.md's M0–M8), plus
  release/deployment and four reference documents. Changes as phases land.
- `docs/11-built.md` — what the finished phases decided differently from their
  plan, and what was measured. **A phase that lands loses its plan file**; what
  outlived it is here.

Read `docs/README.md` first. Phases 0–5 are done; 6, 7 and 8 are not started.

## Relationship to kaas-lib and kaas

Three repositories, and confusing them is the fastest way to do the wrong work:

| | what it is | |
|---|---|---|
| `kaas-rs/kaas` | a **broker** | one of the clusters kaas-ui renders |
| `kaas-rs/kaas-lib` | a **client library** | our only Kafka dependency |
| `kaas-rs/kaas-ui` | **this** | an HTTP server and a web app |

kaas-ui depends on kaas-lib **from crates.io** and talks to kaas **over the
wire like any other cluster**. It must never grow a code path that knows which
of its clusters is kaas — see the version rule below.

kaas-lib is developed in a sibling checkout at `../kaas-lib`. For rapid
iteration, `cargo xtask link` writes a fenced `[patch.crates-io]` block into the
root `Cargo.toml` and `unlink` removes it; `cargo xtask ci` fails if the fence
was committed.

## Hard rules

**1. Read-only is the architecture, not a setting.** Exactly one
`Admin::connect_read_only(` in the workspace, in `kaas-ui-core/src/registry.rs`.
No `Admin::connect(` anywhere. That is the guarantee, and it holds whatever
HTTP verb reaches a handler: a read-only admin cannot write to a cluster. If
`Error::ReadOnly` ever reaches a user, kaas-ui has a bug: it built a request it
should have been incapable of building. Treat it as a 500, not a 405.

The HTTP surface used to claim more than this — "every route is a `GET`" — and
that claim is gone. It was a proxy for the property above, and a weaker one:
the auth flow needs `POST`, and the Dex proxy forwards whatever the browser
sends. What is enforced is the construction site, which is where writing would
have to begin.

**2. No Kafka version number appears anywhere.** No `if version >= 3.5`, no
parsing of a broker version string, no per-version branch. kaas-lib owns version
and implementation compatibility completely — negotiation, `Unknown(i16)`
arms, `UnsupportedApi` with both ranges, `Unrecognized` groups, the
`DescribeTopicPartitions`→`Metadata` fallback. If kaas-ui ever *needs* to know
that a Kafka release added something, push the knowledge down into kaas-lib
instead — file it in `docs/reference/upstream-asks.md`.

The only thing genuinely left to the UI is **deciding what absence looks like**:
hidden tab, greyed tab, or explanatory panel. That is presentation, and it
cannot live in a client library.

**3. Nothing panics.** `unwrap`, `expect`, `panic!` and `indexing_slicing` are
denied at the workspace root. This matters *more* here than in kaas-lib: its
rule 2 exists so a malformed record on one topic cannot take down a server
hosting other clusters, and kaas-ui **is** that server, now hosting a dozen.

**4. No upstream type in a public signature.** kaas-lib's rule 1, one level up.
No `kafka_admin::TopicInfo` in a `utoipa` schema — define DTOs in
`kaas-ui-core` and convert at the boundary, or every library bump breaks the
generated TypeScript client.

**5. Partial failure is a result.** `PerItem<K, T>` is
`Vec<(K, Result<T, Error>)>` and the HTTP envelope preserves it end to end.
Describing 50 topics where 2 do not exist is `200 OK` with 48 items and 2
errors, never a 500.

**6. No stubs, no `todo!()`.** If a task cannot be completed, stop and say so
rather than leaving a placeholder that looks finished. This applies to empty
crates too: `kaas-ui-serde` and `kaas-ui-auth` are created by the phase that
fills them, not up front.

**7. Conventional commits, work lands on `main`.**

**8. Optional configuration is a consuming `with_*` builder.** Every setter is
`#[must_use] pub fn with_x(mut self, …) -> Self`; required data goes in `new()`.
The prefix has no exceptions, booleans included, so a caller never has to know
which crate a type came from to guess what its setters are called. `STYLE.md`
is the whole rule, the three sanctioned exceptions, and why `#[must_use]` is
the half that matters — the convention is shared with kaas-lib, which found the
failure mode on the one setting in that workspace that exists as a safety gate.

Rules 1, 2 and 8 are enforced by greps in `cargo xtask ci`, not by good
intentions — alongside the committed-`link`-fence check and the
sign-in-is-a-navigation check that keeps `SameSite=Lax` working.

## The development environment

**Development happens inside the Kubernetes cluster that hosts the brokers.**
Two consequences, both load-bearing:

- **Docker is not available.** kaas-lib's `testcontainers` approach cannot be
  copied. There is no `cargo xtask integration`.
- **Both Kafka clusters are dialable by service DNS.** So acceptance runs
  against two real three-node clusters instead of one container.

| | `kaas` | `strimzi` |
|---|---|---|
| bootstrap | `kaas.kaas.svc.cluster.local:9092` | `kafka-cluster-kafka-bootstrap.strimzi.svc.cluster.local:9092` |
| what | the kaas broker, 3 replicas | Apache Kafka 4.2.0 via Strimzi, 3 dual-role |
| api keys | **37** | **75** |
| notable | 24 ACLs; no topic ids; **no `DescribeCluster`** | keys 88/89 unnameable by our codec; 8 keys ahead of it |

The gap between them is the point, not an inconvenience: every degradation path
has a live fixture *and* a live absence. Put the two side by side and the
differing tab sets are a conformance report.

Full detail, including measured numbers, in `docs/reference/environment.md`.
kaas-lib's `livetest probe` is the oracle — what the UI renders must agree with
what it reports.

## Verification

```sh
cargo xtask ci      # fmt + clippy + unit tests + the invariant greps. No cluster.
cargo xtask live    # phase acceptance, against both clusters
cargo xtask login   # Phase 4 acceptance. Needs the dex-test app synced
cargo xtask docs    # openapi spec + regenerate the TS client
```

Unit tests stay cluster-free so `cargo xtask ci` runs anywhere. Anything needing
a broker goes in `live`.

Every phase in `docs/` has an acceptance command. Run it. Do not report a phase
complete on the basis of `cargo build` succeeding.

## CI and deployment

CI runs on `arc-runner-set-ui`, a repo-scoped ARC scale set in the same k3s
cluster. Fork PRs are routed to GitHub-hosted runners — the ARC pods are
privileged DinD on a node hostPath and this repo is public. `actionlint` runs
before anything else, because a workflow expression error fails in zero seconds
with no logs.

Images publish to `ghcr.io/kaas-rs/kaas-ui`. Deployment is GitOps via
`Woestebanaan/k3s-cluster` (`apps/kaas-ui/`), picked up by an ApplicationSet
that discovers `apps/*`.

**ArgoCD auto-sync is off in this cluster.** A merged commit registers a change;
applying it is a manual `argocd app sync`. A green pipeline is not evidence that
the cluster changed.

See `docs/10-release-and-deployment.md`.

## Design

`docs/reference/design-system.md`. The palette is mdbook's `rust` theme — the
same one the kaas-lib book uses — with semantic tokens, a derived dark mode, and
status colours mdbook has no need for.

One trap: `#E69F67` is ~2:1 on the paper ground. It is a **surface** colour —
active nav, focus ring, selected edge. For accent text on light use
`--color-accent-ink` (`#8F5A2B`).

## Traps specific to this codebase

- **`Error::Authentication` from a cluster is 502, never 401.** A cluster whose
  SASL credentials were rejected must not log the *user* out.
- **Cluster visibility is a 404, not a 403**, enforced in the registry lookup so
  cluster ids are not enumerable. One lookup function, one place to get it right.
- **Connect lazily.** One unreachable cluster must not block startup, hang
  `/health`, or slow a page that does not touch it. `Cluster::connect` fetches a
  snapshot before returning, so it can never be on a request path.
- **`/health` must not consult a cluster.** A liveness probe that fails because
  someone's broker is down restarts a healthy process.
- **The capability table is per *connection*.** There is no
  `cluster.capabilities()`; do not fabricate one by picking a broker. Until
  upstream ask 1 lands, read from an explicitly named broker and say so in the
  UI.
- **`TailSpec::limit` is spread across partitions with `div_ceil`** — `limit=20`
  on 16 partitions returns 32. The HTTP layer merges and truncates.
- **Streams do not go in the TanStack Query cache.** SSE feeds a capped ring
  buffer in its own hook.
- **Group kinds are four variants, not one struct with optional fields.**
  `Unrecognized` is a *successful* description of an undescribable group.
- **Lag has four states** — no commit, empty partition, caught up, lagging —
  and they must not all render as `0`.
- **An `Unknown` api key is expected output, not a gap to close here.** Keys 88
  and 89 on Strimzi are the live case. Naming them in kaas-ui means a version
  table in kaas-ui, which is rule 2 with extra steps — the fix is a
  `kafka-protocol` bump in kaas-lib. Same for `brokerAhead` rows. See
  `docs/reference/environment.md`.
