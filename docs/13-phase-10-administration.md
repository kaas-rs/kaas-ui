# Phase 10 — administration under RBAC

*Proposed 2026-10-20. Changes hard rule 1; the replacement text is at the end
and lands in CLAUDE.md, PLAN.md and `xtask/src/checks.rs` with 10a, not
before — a rule that says one thing while CI enforces another is worse than
either.*

**Goal.** kaas-ui can change clusters — topics, groups, ACLs, quotas, users,
messages, schemas, connectors — when, and only when, configuration and a role
both say so. A deployment that says nothing is exactly as read-only as today,
by construction, not by a flag a handler checks.

## Read-only by default, writable by grant

Read-only was never the point. The point was that a UI that writes can become
a second source of truth. Whether it does depends on the deployment — some
clusters' topics and connectors are reconciled from Git, some are not — and
that is the deployment's knowledge, not kaas-ui's. So:

1. **Read-only stays the default at every layer.** No config, no role, no
   write — and no anonymous write, ever.
2. **Which writes a target admits is configured per target.** A target whose
   state something else reconciles leaves out of its ceiling the operations
   that would fight it, and keeps the ones that would not, like a restart.

kaas-ui does not model who owns what. A GitOps layer — ownership as a third
gate, a 409 carrying the change to make in Git, manifests rendered beside
every write — was built in 10a and removed on 2026-10-21: every way of saying
what Git declares turned out to be one tool's model, Strimzi's, in disguise.
See [11-built.md](11-built.md#gitops-built-and-removed).

## Two gates, both required

A write happens only when **both** agree. Each lives in a different place, so
no single mistake opens both.

| gate | where | decides | default |
|---|---|---|---|
| **ceiling** | per cluster / registry / Connect cluster, in config | which *kinds* of write this target can ever see | none — a read-only connection, as today |
| **grant** | roles, in config | which *callers* may do which actions to which *named* things | none |

```yaml
environments:
  - id: dev
    kafka_clusters:
      - id: strimzi
        writes:
          # The ceiling. Absent ⇒ Admin::connect_read_only, byte for byte
          # today's behaviour. Present ⇒ a connection whose kaas-lib gate
          # admits exactly the api keys these actions need, and nothing else.
          allow: [topic.create, topic.edit, consumer.reset_offsets, messages.produce]

roles:
  - name: platform-oncall
    subjects: ["kaas-rs:oncall"]
    clusters: ["*"]
    permissions:
      - resource: consumer
        actions: [view, reset_offsets]
      - resource: connector
        value: "gtfs-*"
        actions: [view, operate]
```

- **Ceiling ∩ grant.** A role granting `topic: [delete]` on a
  cluster whose ceiling omits `topic.delete` grants nothing there, and
  `/api/me` says so rather than showing a button that 403s.
- **`Policy::open()` never writes.** Today a deployment without an identity
  provider makes the anonymous caller an administrator, which is right for
  reading and indefensible for writing. Open mode resolves to *view
  everything, write nothing*; a ceiling on a cluster with no IdP is a startup
  error, not a silent no-op.
- **A ceiling change is a reconnect.** The ceiling is part of the connection
  config, so `reload.rs` already treats a changed entry as a new handle. That
  is correct, and is written down here so nobody "optimises" it into an
  in-place swap.

## The action catalogue

A closed set. Each action names its tier, the RBAC resource it is granted on,
and — for Kafka — the **api keys it may emit**, which come from kaas-lib (see
ask B) because which key an operation sends is kaas-lib's business, not ours.

Tiers drive confirmation UX only, never authorization:

- **operate** — reversible; no data lost. One click and a confirm.
- **edit** — changes what the thing is configured to do. Shows what will
  change.
- **destroy** — loses data or position. The form requires typing the
  resource's name; the audit entry is written first and the write fails if it
  cannot be.

| action | tier | RBAC (`resource: action`) | Kafka api keys | kaas-lib |
|---|---|---|---|---|
| `topic.create` | edit | `topic: create` | CreateTopics | ✓ `create_topics`, `validate_topics` |
| `topic.edit` (configs) | edit | `topic: edit` | IncrementalAlterConfigs (AlterConfigs fallback) | ✓ `alter_configs`, `validate_config_changes` |
| `topic.add_partitions` | edit | `topic: edit` | CreatePartitions | ✓ `create_partitions` |
| `topic.delete` | destroy | `topic: delete` | DeleteTopics | ✓ |
| `messages.produce` | edit | `topic: messages_produce` | Produce, InitProducerId | ✓ Producer — needs ask C |
| `messages.delete` (truncate) | destroy | `topic: messages_delete` | DeleteRecords | ✓ `delete_records` |
| `consumer.reset_offsets` | destroy | `consumer: reset_offsets` | OffsetCommit | ✓ refuses a non-empty group |
| `consumer.delete_offsets` | destroy | `consumer: reset_offsets` | OffsetDelete | ✓ |
| `consumer.delete` | destroy | `consumer: delete` | DeleteGroups | ✓ |
| `consumer.remove_members` | operate | `consumer: operate` | LeaveGroup | **ask D** |
| `acl.create` / `acl.delete` | edit / destroy | `acl: edit` | CreateAcls / DeleteAcls | ✓ |
| `quota.edit` | edit | `client_quota: edit` | AlterClientQuotas | ✓ |
| `scram_user.edit` / `.delete` | edit / destroy | `scram_user: edit` | AlterUserScramCredentials | ✓ |
| `cluster.edit` (broker configs) | edit | `cluster_config: edit` | IncrementalAlterConfigs | ✓ |
| `cluster.reassign` / `.cancel` | edit | `cluster_config: edit` | AlterPartitionReassignments | ✓ |
| `cluster.elect_leaders` | operate | `cluster_config: operate` | ElectLeaders | ✓ |
| `transaction.abort` | destroy | `transaction: abort` | WriteTxnMarkers | **ask D** |
| `schema.*` | edit / destroy | `schema: create, edit, delete` | — registry REST | kaas-ui-serde |
| `connector.*` | see Phase 9 | `connect` / `connector` | — Connect REST | kaas-ui-connect |

RBAC vocabulary follows kafbat-ui's where an equivalent exists (`create`,
`edit`, `delete`, `messages_produce`, `messages_delete`, `reset_offsets`,
`operate`) so a migrated role file mostly reads the same. `policy.rs`'s
module comment — "the shape is kafbat-ui's; the verbs cannot be" — is
rewritten by this phase: now they can, and only those that exist do.

Out of scope, deliberately: delegation tokens (a credential minted by a web
UI), `UpdateFeatures`, `UnregisterBroker`, `AlterReplicaLogDirs`, Connect's
`PUT /admin/loggers`. Each is either a credential or a cluster-lifecycle act
that belongs to the operator.

## The construction site, again

Rule 1's property — *a write is impossible to build, not merely forbidden* —
survives. It moves from "no mutating connection exists" to "no mutating
handle exists without a grant".

```rust
// kaas-ui-auth: the only place a Grant can be minted.
pub struct Grant<'a> { action: Action, target: TargetRef<'a>, who: &'a Principal, _seal: Seal }

impl Access {
    pub fn authorize<'a>(&'a self, action: Action, target: TargetRef<'a>,
                         ceiling: &Ceiling) -> Result<Grant<'a>, Denied>;
}

// kaas-ui-core: the only path from a Grant to something that can write.
impl ClusterHandle {
    pub async fn writer(&self, grant: &Grant<'_>) -> Result<ScopedAdmin, Error>;
}
```

- `Seal` is private to `kaas-ui-auth`, so a `Grant` cannot be constructed
  anywhere else — the type system says authorization happened.
- `writer()` turns the grant's action into kaas-lib's api-key scope (ask B)
  and returns `Admin::scoped(scope)` over the cluster's existing pool. A
  handler holding a `ScopedAdmin` for `consumer.reset_offsets` **cannot emit
  DeleteTopics** — kaas-lib refuses it before the socket, the same mechanism
  `connect_read_only` uses today.
- The same pattern for REST targets: `kaas-ui-connect` and the registry
  client gain `ConnectWriter` / `RegistryWriter` types whose only constructor
  takes a `&Grant`. Their mutating calls live in one module each.

**Invariants, enforced by `cargo xtask ci`** (each also by a unit test, as
now):

1. `Admin::connect(` appears nowhere. `Admin::connect_read_only(` and
   `Admin::connect_with_mutations(` appear exactly once each, in
   `registry.rs`, and the second is reached only when a ceiling is configured.
2. `Grant {` (construction) appears only in `kaas-ui-auth/src/gate.rs`.
3. `.scoped(` appears only in `ClusterHandle::writer`.
4. `.post(`, `.put(`, `.patch(`, `.delete(` appear in `kaas-ui-connect` and
   `kaas-ui-serde` only inside their `writer` modules.
5. Every write route is registered through one `write_route` helper that
   requires the action in its signature — so the route table is the
   catalogue, and an endpoint that writes without naming an action does not
   compile.

`Error::ReadOnly` reaching a user is still a 500: kaas-ui built a request its
own ceiling forbade, which is a bug in `writer()`, not a user mistake.

## A write request, end to end

1. **Origin.** Writes are `POST`/`PUT`/`DELETE` and the session cookie is
   `SameSite=Lax`, which already withholds it from cross-site non-GET
   requests. Belt and braces: the `Origin` header must equal the public URL,
   and the request must carry `X-Kaas-Ui: 1`, which a cross-site form cannot
   set. Both checked in one middleware on the write router.
2. **Visibility.** The target is looked up through the existing registry
   lookup. Not visible ⇒ **404**, unchanged.
3. **Authorization.** `Access::authorize` ⇒ `Grant` or `Denied`. Visible but
   not permitted ⇒ **403**: the id is already known to the caller, so 403 leaks
   nothing and is more honest than a 404 for something they can see.
4. **Preview.** `?dry_run=true` where Kafka supports `validate_only`
   (CreateTopics, CreatePartitions, AlterConfigs) runs it on the broker and
   returns what would happen.
5. **Audit, before.** An `intent` record — who, roles, target, action,
   parameters with secrets redacted — is written, and a failed write fails
   the request, the rule `audit.rs` already applies to reads.
6. **Execute** through the scoped handle. Destroy tier requires
   `confirm: "<resource name>"` in the body.
7. **Audit, after.** An `outcome` record — success, or the broker's error
   verbatim. Partial outcomes (an ACL batch where two of five failed) are a
   `PerItem`, rule 5, in both the response and the log.

`/api/me` gains, per visible target, the actions the caller can actually
perform there — ceiling ∩ grant — so the frontend never renders a button
that will 403, and greying a button is reserved for "the cluster cannot"
(an api key the broker does not offer — rule 2's absence, already projected
by Phase 1).

## kaas-lib changes

Filed in `reference/upstream-asks.md` as 17–20 by 10a. kaas-lib is at 0.11.0
and already has almost every admin RPC this needs; what it lacks is a gate
finer than a boolean.

**Kafka only.** Nothing below concerns Connect or the schema registry: their
REST clients, readers and writers both, stay in kaas-ui (`kaas-ui-connect`,
`kaas-ui-serde`), gated by the same `Grant`.

**A. A mutation ceiling on the connection.** `ConnectionConfig::read_only:
bool` becomes an allowed set:

```rust
ConnectionConfig::default().with_allowed_mutations(ApiKeySet::from([ApiKey::CreateTopics, ApiKey::OffsetCommit]))
```

Empty is today's `read_only`; `connect_read_only` stays as the spelling of
"empty" so nothing in the ecosystem breaks. The check stays where it is,
before the socket in `Connection::send`, deny-by-default via
`ApiKey::is_mutating`. `Error::ReadOnly { api_key }` keeps its name and gains
nothing — the set that refused it is the caller's own config. Consuming
`with_*` builder, `#[must_use]`, per the shared STYLE.md rule.

**B. A narrower handle over the same pool, and each operation's key set.**

```rust
impl Admin { pub fn scoped(&self, keys: ApiKeySet) -> Admin }   // ceiling ∩ keys, never wider
pub mod ops { pub const RESET_OFFSETS: ApiKeySet = …; pub const ALTER_CONFIGS: ApiKeySet = …; … }
```

`scoped` intersects with the connection ceiling, so it can only narrow. The
`ops` constants belong in kaas-lib because the set can depend on negotiation —
`alter_configs` falling back from IncrementalAlterConfigs to AlterConfigs is
exactly a version decision kaas-ui must not make (rule 2). A testkit test per
mutating method asserts it emits nothing outside its constant, so the
constant cannot rot.

**C. A producer from an existing cluster handle.** `Producer::connect` builds
its own pool. Producing from the UI wants `Producer::from_cluster(cluster,
scope)` over the cluster's pool, scoped to `Produce` (+ `InitProducerId` if
idempotent), so a produce form does not open a second set of connections per
cluster per click.

**D. The missing RPCs.** `abort_transaction` (WriteTxnMarkers, KIP-664 —
what `kafka-transactions.sh abort` does for a hanging transaction, the one
write the Phase 7 inspector most obviously wants) and `remove_group_members`
(LeaveGroup, KIP-345 — evicting a static member that will never come back).
Share-group offset alter/delete only when kaas-ui renders share groups.

None of these changes behaviour for a read-only caller, which is still every
caller by default.

## Frontend

- `useCan(target, action)` from `/api/me`; no button for an action the caller
  cannot perform.
- One `WriteDialog` primitive: form → **preview** (dry run) →
  confirm (typed name for destroy) → result with the audit id. Every write
  goes through it, which keeps the tier semantics in one place.
- A **Changes** page per environment: the outcome records this instance wrote,
  newest first. Read from the audit sink if it is queryable; otherwise
  in-memory since start, and labelled as such.

## Live acceptance

**10a (Connect pilot)**, against the Strimzi `kafka-connect` with a
`writes: { allow: [connector.restart] }` ceiling in the live config:

- with no `writes:` block, the restart route answers 403 and nothing but `GET`
  reaches the worker;
- in open mode (no IdP) a `writes:` block is a startup error;
- a granted restart succeeds and writes an intent line before and an
  outcome line after;
- a role without `connector: operate` gets 403; a caller who cannot see the
  Connect cluster gets 404; a request without `X-Kaas-Ui` or with a foreign
  `Origin` is refused before authorization;
- a failing audit sink fails the restart and the worker sees no `POST`.

**10a Kafka half onward**, against a ceiling-enabled copy of the Strimzi
cluster entry, with throwaway names under a `kaas-ui-live-` prefix that the
test cleans up:

- with no `writes:` block, every write route answers 403 for the open-mode
  admin and the connection is provably read-only (an `Error::ReadOnly` from a
  planted handler under `#[cfg(test)]`);
- create → describe → add partitions → edit config → delete a topic, each
  with its intent and outcome audit lines;
- a role without `topic: delete` gets 403; a caller who cannot see the
  cluster gets 404;
- reset offsets on a group with a live member is refused by kaas-lib's
  empty-group check and the refusal reaches the UI verbatim;
- a `ScopedAdmin` for `consumer.reset_offsets` refuses `delete_topics`
  (unit test against testkit, not the live cluster);
- **the same on kaas**: writes kaas does not implement are absent from the
  UI, not broken buttons.
- `cargo xtask ci`: each new invariant fails on a planted violation.

## Order

Agreed 2026-10-20: interleaved with Phase 9, **9a → 10a → 9b/9c/9d and the
rest**. The whole sequence is in [README.md](README.md#order).

1. **10a — the gate**, after Phase 9a. `Grant` and the ceiling; the write
   router, Origin check, audit
   intent/outcome, `/api/me` actions, `WriteDialog`; the rule rewrite and the
   new invariants. **Proven end to end on `connector.restart`**, not on a
   Kafka write: it exercises every gate, needs no kaas-lib release (Connect is
   REST, in `kaas-ui-connect` from 9a), and loses nothing if it goes wrong. kaas-lib asks
   A and B are filed and built in parallel.
2. **10a, Kafka half** — once kaas-lib publishes A and B:
   `connect_with_mutations`, `ClusterHandle::writer`, and
   `consumer.reset_offsets` through the same gate, because it is the write
   people most often need during an incident and kaas-lib's empty-group check
   makes it safe to demonstrate.
3. **10b — topics and messages.** Create, edit, partitions, delete, truncate,
   produce (ask C).
4. **10c — security.** ACLs, quotas, SCRAM users.
5. **10d — cluster and transactions.** Broker configs, reassignments, leader
   election, transaction abort (ask D).
6. **10e — schema registry.** Register, delete, compatibility, behind the
   same gate, in `kaas-ui-serde`'s writer.

Phase 9's remaining Connect writes (9d) depend on 10a and nothing after it.

## Later, not now

- **Step-up authentication** for destroy (re-prompt via Dex `max_age`).

## Decisions — settled 2026-10-20

1. **Rule 1's new wording** — landed with 10a, in CLAUDE.md; PLAN.md §1
   carries an amendment note rather than a rewrite.
2. **403 for visible-but-forbidden** — yes. 404 stays for what the caller
   cannot see.
3. **Open mode** — views everything, writes nothing; a `writes:` block with no
   identity provider is a startup error. The dev config gets an IdP when
   writes need exercising live.
4. **Git ownership** — not modelled. Built in 10a as a third gate and
   removed 2026-10-21; a target whose state Git owns says so with its
   ceiling.

## Rule 1, rewritten

> **1. Read-only by default is the architecture; writing is constructed, not
> permitted.** A cluster with no `writes:` block is connected with
> `Admin::connect_read_only(` and cannot be written to by any code path. A
> cluster with one is connected with `Admin::connect_with_mutations(` whose
> kaas-lib gate admits exactly the api keys of its configured actions. Both
> appear once, in `registry.rs`; `Admin::connect(` appears nowhere. A
> mutating handle is reachable only through `ClusterHandle::writer(&Grant)`,
> and a `Grant` is minted only in `kaas-ui-auth`'s gate, which requires the
> ceiling and a role to agree. An anonymous
> caller never holds a grant. If `Error::ReadOnly` reaches a user, kaas-ui
> built a request its own ceiling forbade: a 500, not a 403.
