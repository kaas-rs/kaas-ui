# Phase 7 — the read-only admin surface

*PLAN.md milestone M7.*

**Goal.** ACL viewer, client quotas, SCRAM user list, reassignment status, and
the transaction inspector.

**This is where kaas-ui exceeds kafbat-ui rather than catching up.** kaas-lib
already supports all of it and the JVM tools largely do not surface it. It is
also cheap, because it is all read — five screens over RPCs that already exist,
with no new infrastructure.

## What gets built

| screen | RPCs | `kaas` | `strimzi` |
|---|---|---|---|
| ACL viewer | `DescribeAcls` | **yes — 24 ACLs** | yes, none configured |
| Client quotas | `DescribeClientQuotas` | yes | yes |
| SCRAM users | `DescribeUserScramCredentials` | no | yes |
| Reassignments | `ListPartitionReassignments` | no | yes |
| Transactions | `ListTransactions`, `DescribeTransactions`, `DescribeProducers` | no | yes |

The `kaas`/`strimzi` columns are measured, not guessed. Note the shape: `kaas`
is the *only* cluster with ACLs to look at, and Strimzi is the only one with
everything else. Between them every screen has a live fixture and every screen
has a live *absence* to render — which is the Phase 1 capability projection
being paid off.

**Every one of these features is hidden on the cluster that lacks it and
routable to an explanation.** No new UI machinery: the `CapabilityTab` and
`UnsupportedApiPanel` from Phase 1 do all of it.

## The transaction inspector

The most interesting screen, and the one with no equivalent in the JVM tooling.

- `ListTransactions` → transactional ids and states;
- `DescribeTransactions` → producer id, epoch, timeout, start timestamp, the
  topic-partitions enrolled in the transaction;
- `TransactionDescription::open_for_ms(now)` → how long it has been open, which
  is the number that matters. A transaction open for hours is holding up the
  LSO and every `read_committed` consumer behind it;
- `DescribeProducers` per topic-partition → the producer states on that
  partition, which is how you find the one that is stuck.

Sort by open duration descending and the screen answers "why is my
`read_committed` consumer stalled" in one look.

`DescribeProducers` is one of the four "specific broker" RPCs — it routes to the
partition leader. kaas-lib handles that; do not fan it out by hand.

## Quotas and SCRAM

`DescribeClientQuotas` takes a `QuotaFilter` over entity types (user, client-id,
IP) and returns the configured limits. The `kaas` cluster has a
`throttled-user` and a `kperf-bench-throttled` topic, so there is something real
to render.

`DescribeUserScramCredentials` returns mechanism and iteration count per user —
**never a credential**. Worth saying out loud in the UI: this screen lists who
can authenticate, not how.

`AlterClientQuotas`, `AlterUserScramCredentials`, `CreateAcls`, `DeleteAcls`,
`AlterPartitionReassignments` and `ElectLeaders` are all mutating and all absent
from kaas-ui. There is no route that reaches one.

## Reassignments

`ListPartitionReassignments` returns the in-progress moves: adding replicas,
removing replicas, per partition. `Admin::reassignments_in_progress(&topics)` is
the cheap "is anything happening" check for a badge.

Combined with the Phase 2 placement grid this is genuinely useful: the grid
shows where replicas are, the reassignment view shows where they are going.

Strimzi's Cruise Control is deployed in this cluster, so reassignments are a
thing that actually happens here rather than a hypothetical.

## Traps

- **An empty ACL list and an unavailable ACL API are different.** Strimzi has no
  authorizer configured, so `DescribeAcls` may return an empty list *or* an
  error depending on configuration. Render "no ACLs" and "this cluster has no
  authorizer" differently.
- **`AclFilter` with everything set to `Any` is the "show me all" query**, and
  it is not the `Default`. Build it explicitly.
- **Quota entities are nullable per type.** A `null` user with a set client-id
  means "any user, this client" — the default-quota semantics. Rendering `null`
  as blank loses the meaning; render it as `<default>`.
- **Transaction state strings are broker vocabulary.** Pass them through; do not
  map them to a kaas-ui enum that will be missing a state on the next release.

## Acceptance

```sh
cargo xtask live --config config.dev.yaml
```

- the ACL viewer on `kaas` renders **24 bindings** with principal, resource
  pattern, operation and permission, matching `livetest probe`;
- on `strimzi` the same screen renders the empty/no-authorizer state, not an
  error;
- the SCRAM, reassignment and transaction tabs are **absent on `kaas`** and
  present on `strimzi`, driven by the capability projection with no per-screen
  special casing;
- navigating directly to `/clusters/kaas/transactions` renders the
  `UnsupportedApiPanel` showing `broker: none`, `ours: 0–1`;
- quotas render `throttled-user`'s configured limits on `kaas`;
- a transaction opened by a test producer against `strimzi` appears in the list
  with a rising `open_for_ms`, and its enrolled partitions match
  `DescribeProducers` for those partitions;
- the CI grep still finds no mutating call anywhere in the workspace.

## Exit criteria

- [ ] five screens, all read, no mutating call reachable
- [ ] every screen hidden where unsupported and routable to an explanation
- [ ] ACL viewer verified against the 24 real bindings on `kaas`
- [ ] transaction list sorted by open duration
- [ ] empty-result and unsupported states visibly different everywhere
- [ ] README updated: this is the part kafbat-ui does not have
