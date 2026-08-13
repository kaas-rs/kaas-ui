# Phase 7 — the read-only admin surface

*PLAN.md milestone M7.*

**Goal.** ACL viewer, client quotas, SCRAM user list, reassignment status, and
the transaction inspector.

**This is where kaas-ui exceeds kafbat-ui rather than catching up.** kaas-lib
already supports all of it and the JVM tools largely do not surface it. It is
also cheap, because it is all read — five screens over RPCs that already exist,
with no new infrastructure.

## What gets built

| screen        | RPCs                                                            | `kaas`            | `strimzi`            |
|---------------|-----------------------------------------------------------------|-------------------|----------------------|
| ACL viewer    | `DescribeAcls`                                                  | **yes — 24 ACLs** | yes, none configured |
| Client quotas | `DescribeClientQuotas`                                          | yes               | yes                  |
| SCRAM users   | `DescribeUserScramCredentials`                                  | no                | yes                  |
| Reassignments | `ListPartitionReassignments`                                    | no                | yes                  |
| Transactions  | `ListTransactions`, `DescribeTransactions`, `DescribeProducers` | no                | yes                  |

The `kaas`/`strimzi` columns are measured, not guessed. Note the shape: `kaas`
is the *only* cluster with ACLs to look at, and Strimzi is the only one with
everything else. Between them every screen has a live fixture and every screen
has a live *absence* to render — which is the Phase 1 capability projection
being paid off.

The seven routes are already written down in
[reference/http-contract.md](reference/http-contract.md) and marked `[7]`. They
are not restated here; what is here is what building them has to decide.

## A sixth screen, blocked upstream

A KRaft quorum panel — leader, voters, observers, and how far each is behind —
is the natural sixth, and it is **not** in the table above because `Admin` has
no `describe_quorum`. `DescribeQuorum` (api key 55) is advertised by Strimzi and
absent from `kaas`, so the panel would have the same fixture-and-absence pair as
everything else the moment the call exists. It is
[upstream ask 6](reference/upstream-asks.md), and it stays there: adding it to
this phase means writing the RPC in kaas-ui, which is rule 2 with extra steps.

## What Phase 4 and the nesting changed

This file was written before either landed, and both reach every screen below.

**Everything is addressed `(environment, id)`.** There is no
`/clusters/kaas/…` any more — a cluster id alone addresses nothing, because two
environments may each hold a `kafka`. Every route is
`/api/environments/{env}/clusters/{id}/…` and every lookup goes through
`Registry::get`, which is also what makes an invisible cluster a **404 rather
than a 403**. No screen here decides visibility for itself.

**Every route needs a `Resource` and an `Action`, and this phase should not add
a variant.** Today's are `ClusterConfig`, `Topic` and `Consumer`; all five
screens are cluster-wide administrative facts rather than facts about a named
topic or group, so they belong to **`Resource::ClusterConfig` + `Action::View`**
— the same grant that already covers brokers, log dirs and configs.

The temptation is a new `Resource::Acl` or `Resource::Security`, and Phase 6
already worked through why not: a new variant is not additive, because
`Resource::every()` is what a role saying `all` expands to, and every deployed
role granting `all` today would silently stop covering the new thing. That
argument killed `Resource::Schema` and it kills this one. If a deployment ever
does need "can see brokers, must not see who can authenticate", that is a real
requirement and a breaking change to the policy file, and it gets its own
decision rather than arriving as a side effect of this phase.

**Say it in the audit log.** Phase 4 audits reads; a SCRAM listing and an ACL
listing are exactly the reads worth having a record of.

## What UI machinery actually exists

The claim this file used to make — "no new UI machinery, `CapabilityTab` and
`UnsupportedApiPanel` do all of it" — is half true, and the half that is not
will otherwise be discovered late.

What exists and is ready:

- **the projection itself.** `Feature::{Acls, Quotas, ScramUsers,
  Reassignments, Transactions}` have been in `capabilities.rs` since Phase 1,
  each carrying `Available` or `Unsupported { api, api_key, broker, ours }`.
  Nothing about the five screens needs a capability change.
- **`UnsupportedApiPanel`** — the explanation a hidden thing routes to.
- **`featureState` / `FeatureBadge`** — the projection read at a call site.

What does not exist: there is **no `CapabilityTab` component.**
`reference/design-system.md` describes one, and the code gates tabs the way
`topic-detail.tsx` gates messages and statistics — a condition around
`TabsTrigger`. Five screens gated the same way is the point at which that
condition should become the component the design system already named, and this
phase is where it gets written.

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

**Send the start timestamp, not the duration.** `open_for_ms(now)` takes a
`now`, and whichever `now` the server passes is wrong by the time the response
is read and wronger every second the page stays open. The DTO carries the start
timestamp and the browser ticks — `SnapshotAge` is the same decision, and the
column that matters most on this screen is exactly the one that must not be a
number frozen at serialisation.

**`describe_transactions` returns `PerItem<String, TransactionDescription>`.**
Rule 5 is not advice here: listing thirty transactions where two have since
committed is `200 OK` with twenty-eight items and two errors. `Envelope::
from_per_item` is the only split, and this screen must not be the first to
hand-roll one.

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
- **No upstream type in a DTO.** `AclBinding`, `TransactionDescription`,
  `ProducerState`, `ScramCredentialInfo`, the quota entity and
  `OngoingReassignment` are six new types crossing the boundary in one phase,
  which is the most rule 4 has ever been asked to hold at once. They get DTOs in
  `kaas-ui-core` and a conversion at the edge, or the next kaas-lib bump breaks
  the TypeScript.
- **Every column header and every stat carries a one-line hint.** That is the
  standing convention now, not a nicety, and this phase is full of the terms it
  exists for: an ACL pattern that is `PREFIXED` rather than `LITERAL`, a quota
  entity that is `<default>`, a producer epoch that is not a leader epoch, an
  `open for` that is a duration since a start rather than a timeout. `HintHead`,
  `SortableHead`, and `Stat`'s `hint`.

## Acceptance

```sh
cargo xtask live
```

- the ACL viewer on `kaas` renders **24 bindings** with principal, resource
  pattern, operation and permission, matching `livetest probe`;
- on `strimzi` the same screen renders the empty/no-authorizer state, not an
  error;
- the SCRAM, reassignment and transaction tabs are **absent on `kaas`** and
  present on `strimzi`, driven by the capability projection with no per-screen
  special casing;
- navigating directly to
  `/environments/dev/clusters/kaas/transactions` renders the
  `UnsupportedApiPanel` showing `broker: none`, `ours: 0–1`;
- quotas render `throttled-user`'s configured limits on `kaas`;
- a transaction opened by a test producer against `strimzi` appears in the list
  with a rising open duration, and its enrolled partitions match
  `DescribeProducers` for those partitions;
- a caller whose role grants `topic: view` and nothing else gets **404** on all
  five, in every environment;
- the CI grep still finds no mutating call anywhere in the workspace.

## Exit criteria

- [ ] five screens, all read, no mutating call reachable
- [ ] every screen hidden where unsupported and routable to an explanation
- [ ] ACL viewer verified against the 24 real bindings on `kaas`
- [ ] transaction list sorted by open duration, ticking in the browser
- [ ] empty-result and unsupported states visibly different everywhere
- [ ] guarded by `ClusterConfig` + `View`, with no new `Resource` variant
- [ ] a hint on every column header and every stat
- [ ] README updated: this is the part kafbat-ui does not have
