# kaas-ui — implementation plan

[`PLAN.md`](../PLAN.md) at the repository root is the **design**: what kaas-ui
is, why it is read-only, why multi-cluster, why Dex. It does not change.

This folder is the **implementation plan**: the order things get built in, the
decisions PLAN.md deliberately left open, and the command that proves each phase
is finished. It is expected to change as phases land — and phases that have
landed do not keep a plan here. Their file is deleted and what outlived it is
in [11-built.md](11-built.md).

## Status

**Phases 0–5 are done.**
The workspace is four crates, a frontend and an xtask.

```sh
cargo xtask ci      # green: fmt, clippy, 136 unit tests, four invariant greps
cargo xtask live    # green: 49 assertions against kaas, strimzi and a dead cluster
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
| 6 — schema registry | not started |
| 7 — read-only admin | not started — ACLs, quotas, SCRAM, reassignments, transactions |
| 8 — cross-cluster | not started — fleet topic search, cluster comparison, capability diff |

Phase *n* is milestone M*n* in PLAN.md §8. One numbering, not two.

## Read in this order

| | | |
|---|---|---|
| [00](00-foundations.md) | **Foundations** | workspace, dependency pins, how kaas-lib is consumed, the invariants CI enforces |
| [11](11-built.md) | **What is built** | phases 0–5: what each decided differently from its plan, what was measured, what is still unproven |
| [07](07-phase-6-schema-registry.md) | **Phase 6 — schema registry** | Avro, Protobuf, JSON Schema, per-topic overrides |
| [08](08-phase-7-read-only-admin.md) | **Phase 7 — read-only admin** | ACLs, quotas, SCRAM, reassignments, transactions |
| [09](09-phase-8-cross-cluster.md) | **Phase 8 — cross-cluster** | fleet topic search, cluster comparison, capability diff |

Running alongside all of them:

| | | |
|---|---|---|
| [10](10-release-and-deployment.md) | **Release and deployment** | ARC runners, GHCR under `kaas-rs`, ArgoCD GitOps into the k3s cluster |

## Reference

| | |
|---|---|
| [reference/environment.md](reference/environment.md) | the two live clusters this is developed against, and what they actually answer |
| [reference/kaas-lib-api.md](reference/kaas-lib-api.md) | the library surface, mapped: which call backs which screen |
| [reference/http-contract.md](reference/http-contract.md) | envelope, error mapping, the full endpoint table |
| [reference/design-system.md](reference/design-system.md) | the mdbook `rust` palette as an application design system |
| [reference/upstream-asks.md](reference/upstream-asks.md) | kaas-lib changes the UI wants, sequenced against the phases |

## Two crates that look missing and are not

- **`kaas-ui-serde` does not exist although Phase 3 has run.** Payload
  rendering lives in `kaas-ui-core::dto` and is deliberately smaller than the
  plan: UTF-8 or hex with the encoding named, no JSON step, no per-topic
  override. The crate earns its boundary in
  [Phase 6](07-phase-6-schema-registry.md), where Avro and Protobuf go behind a
  trait. See [00](00-foundations.md).
- **`kaas-ui-auth` holds no OIDC in the version some of these documents
  describe.** It does now — see [11-built.md](11-built.md). Rule 6 is why it
  arrived in slices rather than as a stub: the crate is created by the phase
  that fills it.

Three things were established before writing this plan, by running code rather
than by reading docs, and they shape everything above:

1. **kaas-lib is on crates.io** — 0.1.0 when this plan was written, 0.2.0 since
   Phase 3 — and connects read-only to both target clusters from this
   workspace. No local path dependency is required to start.
2. **Development happens inside the Kubernetes cluster.** Both Kafka clusters
   are dialable by service DNS from here; Docker is *not* available. So the
   acceptance commands run against real brokers, not containers — see
   [reference/environment.md](reference/environment.md).
3. **The capability difference between the two clusters is large and real** —
   `kaas` advertises 37 api keys, Strimzi 75. Every degradation path in PLAN.md
   §5 and §7 has a live fixture from day one, which is why Phase 1 built the
   degradation components rather than deferring them.
