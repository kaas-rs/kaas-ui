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

Nothing is built yet. The repository contains `PLAN.md` and this folder.

Three things were established before writing this plan, by running code rather
than by reading docs, and they shape everything below:

1. **kaas-lib 0.1.0 is on crates.io** and connects read-only to both target
   clusters from this workspace. No local path dependency is required to start.
2. **Development happens inside the Kubernetes cluster.** Both Kafka clusters
   are dialable by service DNS from here; Docker is *not* available. So the
   acceptance commands in each phase run against real brokers, not containers —
   see [reference/environment.md](reference/environment.md).
3. **The capability difference between the two clusters is large and real** —
   `kaas` advertises 37 api keys, Strimzi 75. Every degradation path in PLAN.md
   §5 and §7 has a live fixture from day one, which is why Phase 1 builds the
   degradation components rather than deferring them.
