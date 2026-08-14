# Determining how a client is authenticated

A plan for answering, in the UI, the question an operator asks about every
cluster they inherit: **how does this user get in?** It is a reference to the
kaas-lib surface that answers it, a design for the screen, and the order to
build it in — plus, at some length, the part of the question no Kafka cluster
will answer at all.

Needs **kaas-lib 0.10** (`kafka-admin = "0.10"`, `kafka-conn = "0.10"`); the
calls below do not exist in 0.9.

## The shape of the honest answer

Kafka has no whoami. There is no api key that reports the principal on a
connection, none that reports a connection's mechanism, and nothing that
enumerates who can log in. What exists is one credential store the cluster owns
and a configuration file it will read back to you:

| what you can ask                         | api key                        | what it proves                                                            |
|------------------------------------------|--------------------------------|---------------------------------------------------------------------------|
| stored SCRAM credentials for a principal | `DescribeUserScramCredentials` | positively, that this principal *can* log in this way                     |
| delegation tokens owned by a principal   | `DescribeDelegationToken`      | a live SCRAM credential that authenticates **as** that principal          |
| what each listener accepts               | `DescribeConfigs` on a broker  | the mechanisms and certificate settings on offer                          |
| ACLs naming a principal                  | `DescribeAcls`                 | what it may do, and — from a DN-shaped name — a strong hint at mutual TLS |
| quotas keyed by user                     | `DescribeClientQuotas`         | that the principal is configured at all                                   |

**SCRAM is the only credential store Kafka itself owns.** A `PLAIN` user lives
in a JAAS file on the broker's disk or behind a callback handler, an
`OAUTHBEARER` identity belongs to an identity provider, a Kerberos one to a KDC,
and a mutual-TLS one to a certificate authority. None of them leaves a queryable
record, and `DescribeConfigs` returns the JAAS entries **redacted** — sensitive
configs arrive with `value: None`, which kaas-ui already models as
`ConfigEntry::is_sensitive`.

So the answer is: positive for SCRAM and tokens, inferred for everything else,
and never a statement about a live connection. Design the screen around that
rather than around a single confident string, or it will lie on the first
mutual-TLS cluster it meets.

## The two calls

```rust
use kafka_admin::{Admin, Principal};

let listeners = admin.describe_authentication().await?;              // the cluster half
let described = admin.describe_principal(&Principal::user("alice")).await?;  // the principal half
let verdict   = described.likely_mechanism(&listeners);              // crossed

verdict.to_string();      // "SCRAM-SHA-512 (stored credential)"
verdict.is_conclusive();  // true when exactly one possibility survived
```

Both are non-mutating, so both work through `Admin::connect_read_only` and the
`ApiKey::is_mutating` gate that backs it. That is not incidental: this screen
reads a cluster's security posture, and it is the screen most worth being unable
to change anything from.

### `PrincipalDescription` — per-source results

```rust
pub struct PrincipalDescription {
    pub principal: Principal,
    pub scram:  Result<Vec<ScramCredentialInfo>>,
    pub tokens: Result<Vec<PrincipalToken>>,
    pub acls:   Result<Vec<AclBinding>>,
    pub quotas: Result<Vec<QuotaAssignment>>,
}
```

Every source is a separate `Result` — rule 4's reasoning applied to one
principal instead of many resources. **Render them separately.** A caller who
may read ACLs but not SCRAM credentials gets the ACLs, and the difference
between `Ok(vec![])` and `Err(_)` is the difference between *"the cluster stores
no credential for this principal"* and *"you may not look"*. Collapsing those
two into an empty state is the single most misleading thing this screen could
do.

Three helpers do the summarising: `scram_mechanisms()`,
`has_stored_credentials()`, and `is_unrecorded()` — the last meaning every
source reported nothing, which is **not** proof the principal cannot connect. A
mutual-TLS user describes exactly like a user that does not exist.

`PrincipalToken` deliberately has no HMAC field. `DescribeDelegationToken`
returns one, and it is a SCRAM password once base64'd; if kaas-ui ever needs it
(it does not) that is `describe_delegation_tokens`.

### `ClusterAuthentication` — the listener half

```rust
pub struct ClusterAuthentication {
    pub node_id: i32,
    pub listeners: Vec<ListenerAuth>,        // name, protocol, sasl_mechanisms,
    pub principal_mapping_rules: Option<String>,  // client_auth, is_inter_broker, is_controller
}
```

`client_listeners()` is the accessor to use. It filters out the inter-broker
listener **and KRaft controller listeners**, which on a combined broker/controller
node — which is what Strimzi's dual-role pods and every small cluster run — are
listed in `listeners` right alongside the client ones. Counting them attributes
the controllers' authentication to users, and on a cluster whose brokers talk
PLAINTEXT to each other it would report an open cluster.

Read from **one** broker (the lowest node id, so two calls compare like with
like). Listener names must agree cluster-wide, so this is the cluster's answer
in every case that is not a misconfiguration — but per-listener mechanism and
certificate settings are per-broker, and a cluster mid-rollout can genuinely
disagree with itself. `node_id` records who answered.

## How the verdict is reached

Three rules, in order. Worth understanding before rendering the output, because
the UI's job is to show which one fired:

1. **Stored credential.** A SCRAM entry or delegation token that a client
   listener also enables. Positive evidence — but still "can", not "did": a
   principal with a SCRAM password may hold a certificate too. A credential for
   a mechanism no listener offers is treated as a leftover, not an answer.
2. **Certificate subject.** A DN-shaped principal name on a cluster that accepts
   certificates. Kafka's default `ssl.principal.mapping.rules` is `DEFAULT` —
   use the whole subject — so the name *is* the evidence. `CN=bob-mtls` is a
   principal, not a username.
3. **Elimination.** SCRAM and delegation tokens need a credential the cluster
   stores and there is none, so strike them; strike mutual TLS if the name is
   not a subject *and* the mapping rules are the default; report what is left.

`VerdictBasis` names which fired, and `candidates` may hold more than one — or
none, which says the cluster offers this principal no way in at all.

**The guard worth knowing:** an unreadable credential store never becomes an
elimination. If `scram` came back `CLUSTER_AUTHORIZATION_FAILED`, the basis is
`Unknown`, not "well, no SCRAM then". Not finding a credential and not being
allowed to look produce the same empty list and mean opposite things — and
kaas-ui, connecting as a service account with whatever ACLs an operator gave it,
will hit the second case routinely.

## Capability gating

Five api keys, and unusually for this codebase they should **not** be one
feature. The existing `Feature::keys()` contract is "all of them, not any of
them", and a screen that vanishes because a cluster has no delegation tokens
would be wrong — the principal half degrades source by source.

Add to `kaas-ui-core::capabilities`:

```rust
/// How a principal authenticates: listener inventory plus stored credentials.
PrincipalAuth,   // &[ApiKey::DescribeConfigs]
```

Gate the *screen* on `DescribeConfigs` alone — without the listener half there
is no verdict at all, only an inventory — and let the four principal sources
degrade individually into the per-source states below. `Feature::ScramUsers`,
`Feature::Acls` and `Feature::Quotas` already exist and already gate their own
panels; reuse them to decide which rows to even attempt.

On the `kaas` broker this mostly does not light up: it has no
`DescribeUserScramCredentials` and no delegation-token apis at all, so expect
`Unknown` verdicts and a degradation component naming the missing keys. That is
the correct answer, and it is the same shape Phase 1 already built for.

## API and DTO

One endpoint per half, matching the existing admin routes:

```
GET /environments/{env}/clusters/{id}/authentication
GET /environments/{env}/clusters/{id}/principals/{principal}
```

The principal is path-encoded and may be a DN with commas and equals signs —
`Principal::parse` splits on the **first** colon only, and `User:CN=bob,O=x` is
one principal, not a type plus three fields. Do not split it in the router.

The DTO must keep the per-source distinction. Model each source as a
three-state, not an `Option`:

```ts
type Source<T> = { state: "ok"; value: T }
               | { state: "denied"; error: string }
               | { state: "unsupported"; error: string };
```

`state: "ok"` with an empty array is a real answer and must render differently
from `denied`. This is the same discipline as `GroupDescription::Unrecognized`
— a successful description of something undescribable — and for the same
reason: the alternative moves the knowledge somewhere the compiler cannot check.

## The screen

It belongs on the SCRAM users screen, which is already the "who are the users"
surface, plus a cluster-level panel:

- **Cluster → Security → Listeners.** One row per listener: name, protocol,
  mechanisms, whether it wants a certificate, and a badge for inter-broker and
  controller listeners. This is the panel that answers "is anything here
  actually authenticated" in one glance, and it is useful on its own.
- **A principal detail panel**, reachable from the SCRAM users list, from an ACL
  row's principal, and from a quota entity. Verdict at the top with its basis as
  a chip — `stored credential` / `certificate subject` / `by elimination` /
  `unknown` — then the four sources as separate blocks.

Render a non-conclusive verdict as a list, never as the first candidate. "PLAIN
or mutual TLS" is the honest answer on a cluster with custom mapping rules, and
picking one silently is how a UI teaches an operator something false.

Do **not** show a green "authenticated" state anywhere. Nothing here proves a
principal authenticated; the closest available fact is that the cluster holds a
credential it could use.

## Build order

Each step ends in a command that proves it, in the spirit of the numbered
phases:

1. **Bump the pin to 0.10** and add `PrincipalAuth` to the `Feature` enum, its
   `keys()` arm and `ALL`. — `cargo xtask ci`, which greps the invariants and
   will fail on a missing match arm.
2. **`kaas-ui-core::admin`**: two wrappers returning owned DTOs, one per call,
   with the per-source three-state mapping. Unit-test the mapping against
   constructed `PrincipalDescription` values — no broker needed, which is most
   of the logic.
3. **Routes and OpenAPI**, alongside `acls` and `scram_users`. — `cargo xtask
   ci`, then the http-contract tests.
4. **The listeners panel.** Read-only, no principal involved, and immediately
   useful. — `cargo xtask live` against a real cluster.
5. **The principal panel**, with the four source blocks and the verdict chip.
6. **Links in** from the ACL principal column and the quota entity column.

Steps 1–3 are testable without a cluster. Steps 4–6 need one.

## Verification, and a caveat about it

`cargo xtask live` is the proof, and the two clusters it names in
[environment.md](environment.md) are the fixtures — Strimzi's four listeners
(plaintext, server TLS, OAUTHBEARER, mutual TLS) exercise every branch of the
verdict, and its `bob-mtls` KafkaUser, principal `CN=bob-mtls`, is the
certificate-subject case that no container fixture in kaas-lib can produce.

**As of 2026-08-14 neither cluster is running** — the `strimzi` namespace is
gone and `kaas` is empty — so steps 4–6 cannot currently be verified, and the
kaas-lib side ships verified only against its CI broker (`apache/kafka:4.3.1`,
one SASL listener). Bring a cluster back before building past step 3; the
inference rules are exactly the kind of thing that is right in principle and
wrong against a listener map nobody predicted.

kaas-lib's own `livetest principal [<principal>...]` prints both halves and the
verdict as a diffable report, and is the fastest way to see what a cluster
actually says before writing any UI against it.

## What this does not answer

- **How a given connection authenticated.** No connection table exists in the
  protocol. The broker's authorizer log is the only record, and it is out of
  band from kaas-ui by design.
- **Who can use `PLAIN`.** Redacted. That a listener enables it is readable;
  the user list is not.
- **Whether a principal is currently connected.** Not modelled anywhere in
  Kafka's admin surface.
- **kaas-ui's own identity.** The service account's mechanism comes from
  `ClusterConfig::connection.sasl`, which kaas-ui already holds in its config —
  no round trip needed, and `describe_principal` on your own principal is a
  worse answer than reading the config you dialled with.
