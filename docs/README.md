# kaas-ui — status

What is built, what is left, and in what order. How it is built is
[ARCHITECTURE.md](ARCHITECTURE.md); who may see and change what is
[RBAC.md](RBAC.md).

## Status

**Phases 0–7 are done, and 9 (Kafka Connect: 9a, 9d) and the Connect half of 10a.**
The workspace is six crates, a frontend and an xtask.

```sh
cargo xtask ci      # green: fmt, clippy, unit tests, seven invariant checks
cargo xtask live    # green: 71 assertions against kaas, strimzi and a dead cluster
cargo xtask login   # 11 assertions, a real login. Needs the dex-test app
                    # synced; it is currently out of the cluster, so this
                    # command reports that and does nothing.
```

| phase | state |
|---|---|
| 0 — skeleton | **done** — config, registry, lazy connect, `/health`, fleet view, embedded frontend, image, CI |
| 1 — capabilities | **done** — projection, `source` naming the broker, degradation components, brokers, log dirs, configs |
| 2 — topics | **done** — server-side filter/sort/page, detail, partitions, placement grid, configs, offsets |
| 3 — messages | **done** — seven seek modes over SSE, virtualised list, detail panel, URL state |
| 4 — auth | **done** — Dex deployed, people sign in, roles enforced, reads audited, and `cargo xtask login` performs a real login against a second Dex in the cluster |
| 5 — groups | **done** — four kinds, members, committed offsets, lag as states rather than a subtraction |
| 6 — schema registry | **done** — one registry per environment, Avro/Protobuf/JSON Schema, the codec chip, the schema browser, the payload filter over decoded values |
| 7 — admin | **done** — ACLs, quotas, SCRAM users, reassignments, the transaction inspector, and the `CapabilityTab` the design system named in Phase 1 |
| 8 — cross-cluster | not started — fleet topic search, cluster comparison, capability diff |
| 9 — Kafka Connect | **done** — 9a (read) and 9d (writes). MirrorMaker 2 (9b–9c) was built and removed: kaas-ui supports Connect only |
| 10 — administration | **10a's Connect half done** — the gate, connector restart/pause/resume/stop; the Kafka half waits on kaas-lib asks 17–18 |

## Order

Agreed 2026-10-20 for Phases 9 and 10 — interleaved, because the first write
is proven on Connect, which needs no kaas-lib release:

| step | needs | delivers | state |
|---|---|---|---|
| **9a** | — | reading Connect: `kaas-ui-connect`, connectors, tasks, redacted configs, links into topics and lag | **done** |
| **10a** | 9a | the write gate — ceiling, grant, Origin check, audit intent/outcome — proven on connector restart, pause, resume, stop | **done** — one authenticated live write (a task restart, signed in through production's Dex via `config.dev-auth.yaml`) |
| kaas-lib asks 17–18 | parallel with 10a | mutation ceiling, `Admin::scoped` | **released** in kaas-lib 0.12.0, verified live by `livetest gate`; pinned |
| **10a, Kafka half** | kaas-lib release | `consumer.reset_offsets` through the same gate | |
| **9d** | 9a, 10a | the remaining Connect writes: task restart, reset topics, edit, create, delete, offsets | **done** — task restart verified live, signed in; the rest against a stub worker. GitOps ownership was built and removed |
| ~~9b · 9c~~ | — | MirrorMaker 2 flows and record decoding | **removed** — built and verified, then taken out |
| **10b–10e** | 10a | topics and messages, security, cluster and transactions, schema registry | |

Phase 8 is independent of both.

## Documents

| | |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | the crates, the fleet model, startup, a request end to end, the invariants CI holds |
| [RBAC.md](RBAC.md) | sign-in, roles, visibility, the audit log, and the write gate |
| [reference/environment.md](reference/environment.md) | the two live clusters this is developed against, and what they actually answer |
| [reference/kaas-lib-api.md](reference/kaas-lib-api.md) | the library surface, mapped: which call backs which screen |
| [reference/client-authentication.md](reference/client-authentication.md) | how to determine the way a principal authenticates, and what no cluster will answer |
| [reference/http-contract.md](reference/http-contract.md) | envelope, error mapping, the full endpoint table |
| [reference/design-system.md](reference/design-system.md) | the mdbook `rust` palette as an application design system |
| [reference/upstream-asks.md](reference/upstream-asks.md) | kaas-lib changes the UI wants, sequenced against the phases |

## Two crates that arrived late, on purpose

- **`kaas-ui-serde` did not exist until Phase 6.** Payload rendering lived in
  `kaas-ui-core::dto` for three phases — UTF-8 or hex with the encoding named
  — and the crate was created by the phase that filled it, which is rule 6
  rather than an oversight. It now holds the Confluent framing, the registry
  client and the codecs, and `kaas-ui-core` depends on it.
- **`kaas-ui-auth` did not exist until Phase 4.** Same rule, same reason: the
  crate is created by the phase that fills it.

Three things were established before any of this was built, by running code
rather than by reading docs, and they shape everything above:

1. **kaas-lib is on crates.io** — 0.1.0 when this plan was written, 0.2.0 since
   Phase 3, 0.4.0 since its builder convention settled — and connects to both
   target clusters from this workspace. No local path dependency is required
   to start.
2. **Development happens inside the Kubernetes cluster.** Both Kafka clusters
   are dialable by service DNS from here; Docker is *not* available. So the
   acceptance commands run against real brokers, not containers — see
   [reference/environment.md](reference/environment.md).
3. **The capability difference between the two clusters is large and real** —
   `kaas` advertises 40 api keys, Strimzi 75. Every degradation path has a live
   fixture from day one, which is why Phase 1 built the degradation components
   rather than deferring them.
