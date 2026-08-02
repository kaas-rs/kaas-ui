# kaas-ui — implementation plan

[`PLAN.md`](../PLAN.md) at the repository root is the **design**: what kaas-ui
is, why it is read-only, why multi-cluster, why Dex. It does not change.

This folder is the **implementation plan**: the order things get built in, the
decisions PLAN.md deliberately left open, and the command that proves each phase
is finished. It is expected to change as phases land.

## Read in this order

| | | |
|---|---|---|
| [00](00-foundations.md) | **Foundations** | workspace, dependency pins, how kaas-lib is consumed, the invariants CI enforces |
| [01](01-phase-0-skeleton.md) | **Phase 0 — skeleton** | config, registry, `/health`, fleet view, embedded frontend, image |
| [02](02-phase-1-fleet-and-capabilities.md) | **Phase 1 — capabilities** | the capability projection, degradation components, broker and config views |
| [03](03-phase-2-topics.md) | **Phase 2 — topics** | list, detail, partitions, configs, replica placement |
| [04](04-phase-3-messages.md) | **Phase 3 — messages** | tail, then scan over SSE |
| [05](05-phase-4-auth.md) | **Phase 4 — auth** | OIDC via Dex, sessions, roles, access audit |
| [06](06-phase-5-consumer-groups.md) | **Phase 5 — groups** | four group kinds, committed offsets, lag |
| [07](07-phase-6-schema-registry.md) | **Phase 6 — schema registry** | Avro, Protobuf, JSON Schema, per-topic overrides |
| [08](08-phase-7-read-only-admin.md) | **Phase 7 — read-only admin** | ACLs, quotas, SCRAM, reassignments, transactions |
| [09](09-phase-8-cross-cluster.md) | **Phase 8 — cross-cluster** | fleet topic search, cluster comparison, capability diff |

Phase *n* is milestone M*n* in PLAN.md §8. One numbering, not two.

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

## Status

**Phases 0, 1, 2 and 3 are built, plus the group views from Phase 5.** The
workspace is three crates, a frontend and an xtask; `cargo xtask ci` is green
and `cargo xtask live` passes 50 assertions against `kaas`, `strimzi` and a
deliberately dead third cluster.

Phase 3 needed three changes to kaas-lib, released as **0.2.0**: the anchored
backward walk, `ScanSpec::following`, and a fix for `scan` emitting records
before its start offset. The first two add public fields to structs that are
not `#[non_exhaustive]`, which is why a set of additive features is a minor
rather than a patch — see [Phase 3](04-phase-3-messages.md).

| phase | state |
|---|---|
| 0 — skeleton | **done.** Config, registry, lazy connect, `/health`, fleet view, embedded frontend, image, CI |
| 1 — capabilities | **done.** Projection, `source` naming the broker, degradation components, brokers, log dirs, configs |
| 2 — topics | **done.** Server-side filter/sort/page, detail, partitions, placement grid, configs, offsets |
| 3 — messages | **done.** Seven seek modes over SSE, virtualized list, detail panel, URL state. No `kaas-ui-serde` — see the phase file |
| 4 — auth | not started. There is no Dex in the cluster yet, which is a prerequisite rather than a step |
| 5 — groups | **mostly done.** Four kinds, members, committed offsets, four-state lag. No offset-reset view (it would be mutating anyway) |
| 6 — schema registry | not started |
| 7 — read-only admin | not started. ACLs, quotas, SCRAM, reassignments, transactions |
| 8 — cross-cluster | not started |

What each finished phase decided differently from its plan is recorded in that
phase's own file, under "Decisions this phase changed". Nothing was quietly
skipped, and the two that look skipped are not:

- **`kaas-ui-auth` does not exist** because Phase 4 has not run, which is the
  rule rather than an omission.
- **`kaas-ui-serde` does not exist although Phase 3 has run.** Payload
  rendering lives in `kaas-ui-core::dto` and is deliberately smaller than the
  plan: UTF-8 or hex with the encoding named, no JSON step, no per-topic
  override. The crate earns its boundary when Phase 6 adds Avro and Protobuf
  behind a trait — see [00](00-foundations.md) and
  [04](04-phase-3-messages.md).

Three things were established before writing this plan, by running code rather
than by reading docs, and they shape everything below:

1. **kaas-lib is on crates.io** — 0.1.0 when this plan was written, 0.2.0 since
   Phase 3 — and connects read-only to both target clusters from this
   workspace. No local path dependency is required to start.
2. **Development happens inside the Kubernetes cluster.** Both Kafka clusters
   are dialable by service DNS from here; Docker is *not* available. So the
   acceptance commands in each phase run against real brokers, not containers —
   see [reference/environment.md](reference/environment.md).
3. **The capability difference between the two clusters is large and real** —
   `kaas` advertises 37 api keys, Strimzi 75. Every degradation path in PLAN.md
   §5 and §7 has a live fixture from day one, which is why Phase 1 builds the
   degradation components rather than deferring them.
