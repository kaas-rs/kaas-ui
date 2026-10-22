# RBAC

How kaas-ui decides who someone is, what they can see, and what they can
change. The code is in `kaas-ui-auth`: `identity.rs`, `oidc.rs`, `policy.rs`,
`gate.rs` and `audit.rs`. The rest of the workspace calls into it at two points:
the `Caller` extractor in `kaas-ui-api/src/auth.rs`, and the registry lookups in
`kaas-ui-core/src/registry.rs`.

Each question has its own mechanism:

| question | mechanism | configured by |
|---|---|---|
| Who are you? | OIDC through Dex, resulting in a `Principal` | `auth:` (and `dex:`) |
| What can you see and read? | roles resolved into an `Access` | `roles:` |
| What can you change? | the write gate mints a `Grant` | `roles:` **and** the target's `writes:` |

## Deployment modes

Whether `auth:` and `roles:` are present decides the mode:

| `auth:` | `roles:` | behaviour |
|---|---|---|
| absent | empty | **Open.** Every request is the anonymous caller, who sees every cluster and can read every payload. Nobody can write. |
| present | empty | **Open, with sign-in.** Signing in records who is looking but grants nothing extra, because everything is already visible. Nobody can write, since no role exists to permit it. |
| absent | non-empty | **Enforcing, with no one to cover.** Every caller is anonymous and no role covers the anonymous caller, so the fleet is empty for everyone. The startup log warns about this. |
| present | non-empty | **Enforcing.** Callers see and do what their roles grant. |

Two configurations are startup errors rather than silent no-ops:

- A `writes:` block on a target when `auth:` is absent, because an anonymous
  caller can never write.
- A `writes:` block whose `allow` list is empty.

The open mode is what every development instance runs. The startup log says
which mode is active.

## Identity

### Signing in

kaas-ui is an OIDC public client of [Dex](https://dexidp.io). Dex connects to
the actual identity providers (GitHub, Entra, and others), so kaas-ui never
names a provider. Adding one is a Dex config change, not a kaas-ui release.

```yaml
dex:
  upstream: http://dex.dex.svc.cluster.local:5556   # proxied at /dex/*
auth:
  issuer: https://kaas.example.com/dex   # compared to every token's `iss`
  client_id: kaas-ui
  redirect_url: https://kaas.example.com/auth/callback
  scopes: [openid, profile, email, groups]   # the default
  session_ttl: 8h                            # the default
  connectors:                                # optional: one button each
    - { id: github, name: GitHub }
    - { id: microsoft, name: Microsoft }
```

The login is the authorization-code flow, and all of these are always on:

- **PKCE with `S256`.** kaas-ui has no client secret, so the PKCE challenge is
  what proves the caller redeeming the code is the one who started the flow.
- **`state`**, compared when the browser returns.
- **`nonce`**, verified inside the `id_token`.
- **The `id_token` signature**, verified against the provider's JWKS.

`auth.internal_url` defaults from `dex.upstream`. Discovery, the token exchange
and the key set fetch then go straight to the Dex Service, while only the
browser's redirects use the public issuer.

The routes are `GET /auth/login[?connector=<id>]`, `GET /auth/callback` and
`POST /auth/logout`. They exist only when `auth:` is configured.
`/auth/login` must stay a `GET`, and `cargo xtask ci` checks that it does.

### The principal

A `Principal` carries only what roles can match on and what the UI renders:

| field | from | used for |
|---|---|---|
| subject | `sub` | the stable id in the audit log. Dex's `sub` is opaque: it encodes the connector and the upstream user id. |
| name | `name` | display |
| aliases | `preferred_username`, `email` | naming a person in a role |
| groups | `groups` | naming a team in a role: an Entra group, a GitHub `org` or `org:team` |

Roles match on all of them as one flat set, so a role's `subjects` may list any
of them.

### The session

Sessions are kept in an encrypted cookie, `kaas-ui-session`, with
`SameSite=Lax`. There is no session store. The cookie holds the subject, the
display name, the **names of the roles** the login resolved to, and an expiry
that is checked on the server. Two consequences:

- **Roles are resolved at login, not on each request.** The cookie holds role
  names rather than the groups claim, because a large organisation's groups
  claim will not fit in 4 KB.
- **Restarting ends every session.** The encryption key is generated at
  startup. `roles:` and `auth:` are also read only at startup, so a change to
  either needs a restart, and everyone signs in again under the new roles.

kaas-ui never requests refresh tokens or `offline_access`. When the session
TTL expires, the user signs in again.

`GET /api/me` returns the caller as kaas-ui sees them: `authenticated`,
`subject`, `displayName`, `roles`, `enforcing`, `loginAvailable` and the
sign-in `connectors`.

## Roles

```yaml
roles:
  - name: prod-support                    # shown in /api/me and the audit log
    subjects: ["acme:support", "jane@acme.com"]
    clusters: ["prod-*"]                  # cluster ids; empty = every cluster
    cluster_labels: { env: prod }         # every pair must match
    permissions:
      - resource: topic
        value: "public-*"                 # which ones, by name
        actions: [view, messages_read]
      - resource: consumer
        actions: [view]
```

- **`subjects`** match against the subject, aliases and groups. A bare `"*"`
  matches any *authenticated* caller and never the anonymous one.
- **`clusters`** are Kafka cluster ids. Empty matches every cluster.
- **`cluster_labels`** must all match. Every cluster has an `env` label equal to
  the environment it is declared in, so `{env: prod}` selects by environment.
- **`value`** is a glob on the resource's name. `*` is the only wildcard and the
  text around it is anchored: `public-*` does not match `not-public-orders`.
  Absent means every name.
- **Roles are additive.** A caller holds the union of every role whose
  `subjects` cover them. There are no deny rules.

### Resources and actions

| resource | named by | actions | what it guards |
|---|---|---|---|
| `cluster_config` | — | `view` | cluster detail, brokers, log dirs, TLS, cluster configs, capabilities, and the admin screens: ACLs, quotas, SCRAM users, reassignments, transactions, producers |
| `topic` | topic name | `view`, `messages_read` | `view`: the topic list, detail, offsets and configs. `messages_read`: the records themselves (tail, page, stream, single record) and topic analysis |
| `consumer` | group id | `view` | consumer groups, members, committed offsets and lag |
| `connect` | Connect cluster id | `view`, `create` | `view`: seeing a Connect cluster and its connectors at all. `create`: creating a connector on it |
| `connector` | connector name | `operate`, `edit`, `delete`, `reset_offsets` | the Connect writes, see [Writes](#writes) |

`all` expands to every action of that resource. `Permission::all()` (every
resource, every action) is what an `admin` role looks like.

The resource and action vocabulary is kafbat-ui's, but includes only verbs some
code path actually performs. A permission that would grant nothing is refused at
startup, because it would look like protection without being any:

- a role with no name, no subjects, or no permissions;
- a permission with no actions;
- a `value` on `cluster_config`, which has no names to match;
- an action the resource does not have, such as `operate` on `topic` or
  `messages_read` on `consumer`.

Topic configs need `cluster_config: view` **and** `topic: view` on that topic,
so a config page cannot reveal a topic the `value` pattern hides.

## Visibility: 404, not 403

Something the caller cannot see answers `404`, the same as something that does
not exist. Answering `403` would confirm the id exists, and that would let
anyone enumerate the fleet by probing. The check lives in the registry lookups
(`Registry::get`, `environment`, `schema_registry`, `connect_cluster`), one per
kind, so no handler can forget it.

| thing | visible when |
|---|---|
| Kafka cluster | a role covering the caller selects it (by `clusters` and `cluster_labels`) and grants anything at all on it |
| environment | at least one of its clusters is visible |
| schema registry | if any cluster references it: the caller has `topic: view` on one of *those* clusters. If none does: `topic: view` on any cluster in the environment |
| Connect cluster | `connect: view` with a `value` matching its id, selected on the Kafka cluster it keeps state in, **and** `topic: view` on that Kafka cluster. Without a `kafka_cluster:`, selected on its own id and `env` label, with `topic: view` anywhere in the environment |
| inventory (`resources:`) | the same rule as a cluster, against the resource's id and labels |

The schema registry and Connect rules exist because subjects and connectors
reveal topic names. A caller who cannot view a cluster's topics must not learn
them from the registry or the connectors next to it.

Inside a visible cluster:

- **Lists are filtered by name.** The topic and group lists drop names outside
  the caller's `value` patterns.
- **A direct request outside the pattern is `403`.** At that point the cluster
  is already visible, so the `403` reveals nothing new. A `?name=` filter is
  checked against the pattern before the broker is asked, so the answer does
  not reveal whether the topic exists.
- **Signed in but covered by no role** gives an empty fleet, not an error.

### What the frontend is told

Each cluster card carries `grants`: per resource, which of `view` and
`messages_read` the caller holds on that cluster. The sidebar and tabs hide
what the caller may not view, because a tab that returns 403 when clicked is
worse than no tab. The topic page hides Messages without `messages_read`. These
are cluster-level answers. A role scoped to `public-*` still reports
`messages_read`, and the per-topic check refuses topics outside the pattern.

## Payload reads and the audit log

`messages_read` is checked together with the topic name as one question
(`Access::may_read_topic`). A role granting `messages_read` on `public-*` says
nothing about `payments`, and checking the action and the name separately is
how that would leak.

Every disclosure is audited, and **the entry is written before the payload
leaves the process**. If the audit write fails, the request fails. The sink is
one JSON line per entry on stdout, collected by the cluster's log pipeline.

```json
{"at":"2026-10-22T10:15:56.123Z","subject":"Cgx…","displayName":"Jane",
 "cluster":"kaas","topic":"orders","action":"page","mode":"latest",
 "firstOffset":1200,"lastOffset":1219,"records":20}
```

`action` is one of `tail`, `page`, `record`, `stream` and `analysis`. A stream
or an analysis is recorded when it opens, because that is when disclosure
begins. Offsets and counts come from what was returned, not from what was
requested. Metadata (listing topics, describing groups) is not audited.

## Writes

### Two gates

A write happens only when two separately configured things agree, so a single
mistake cannot open both:

1. **The ceiling.** The target's `writes.allow` lists the operations it admits
   at all. Without a `writes:` block a target admits nothing. An operator- or
   pipeline-managed target leaves out the operations that would fight its
   reconciler.
2. **A role.** One of the caller's roles must grant the operation's permission
   on that target and name. This uses `Access::role_permits`, not `may`, so the
   open mode's anonymous administrator holds nothing here.

The caller must also be signed in. `Access::authorize` checks all three and is
**the only constructor of `Grant`**:

- `Grant` has a private seal field, so it cannot be built outside
  `kaas-ui-auth/src/gate.rs`, and `cargo xtask ci` checks that `Grant {` is
  constructed nowhere else.
- It is not `Clone`. It carries the operation, the target (environment,
  cluster, host, name), the subject, the covering roles, and an id that links
  the audit lines.
- Every mutating client method takes `&Grant` and checks that the grant is for
  *this* operation on *this* name, so a grant to restart `a` cannot stop `b`.

`Access::check` answers the same question without minting a grant. The UI uses
it to decide which buttons to draw, and it is never used to perform a write.

### The operation catalogue

Today every operation is a Kafka Connect operation:

| operation | tier | needs |
|---|---|---|
| `connector.restart` | operate | `connector: operate` |
| `connector.restart_task` | operate | `connector: operate` |
| `connector.pause` | operate | `connector: operate` |
| `connector.resume` | operate | `connector: operate` |
| `connector.stop` | operate | `connector: operate` |
| `connector.reset_topics` | operate | `connector: operate` |
| `connector.edit` | edit | `connector: edit` |
| `connector.create` | edit | `connect: create`, with `value` matched against the Connect cluster id |
| `connector.delete` | destroy | `connector: delete` |
| `connector.reset_offsets` | destroy | `connector: reset_offsets` |
| `connector.alter_offsets` | destroy | `connector: reset_offsets` |

The **tier** sets how much confirmation a write asks for, never who may perform
it. Operate is one click and a confirm. Edit shows what will change, and an edit
or a create can be a dry run that the plugin validates without applying (dry
runs are not audited, since nothing changed). Destroy requires the connector's
name typed into `confirm`, and is refused with `400` without it.

### How a role selects a Connect target

There is no separate axis for Connect in a role. A Connect operation is selected
on the **Kafka cluster the Connect cluster keeps its state in**, the same
cluster that decides who may see it. A role with `clusters: [strimzi]` therefore
covers connectors on every Connect cluster whose `kafka_cluster:` is `strimzi`.
A Connect cluster without `kafka_cluster:` is selected by its own id and its
`env` label.

```yaml
environments:
  - id: prod
    kafka_clusters:
      - id: strimzi
        bootstrap: [...]
    connect_clusters:
      - id: connect
        url: http://connect-api.strimzi.svc:8083
        kafka_cluster: strimzi
        writes:
          allow: [connector.restart, connector.restart_task,
                  connector.pause, connector.resume]

roles:
  - name: connect-operators
    subjects: ["acme:platform"]
    clusters: [strimzi]
    permissions:
      - resource: topic
        actions: [view]                   # to see the Connect cluster at all
      - resource: connect
        value: connect
        actions: [view]
      - resource: connector
        value: "sink-*"
        actions: [operate]
```

Members of `acme:platform` can restart, pause and resume connectors named
`sink-*` on `prod/connect`. They cannot stop one, because `connector.stop` is
not in the ceiling, and they cannot edit one, because no role grants `edit`.

### A write, end to end

Every Connect write is a `POST` with a JSON body, through `perform` in
`kaas-ui-api/src/routes/connect_write.rs`:

| step | refused with |
|---|---|
| `write_guard`: `X-Kaas-Ui: 1` is present, and `Origin`, if sent, names this host | `403` |
| lookup: the Connect cluster as this caller sees it | `404` |
| gate: `Access::authorize` mints a `Grant` or names the gate that refused (anonymous, not enabled, forbidden) | `403`, with the reason |
| confirm: a destroy-tier write carries the connector's name | `400` |
| **audit the intent** | `500`, and nothing is sent |
| the write, through `ConnectWriter`, which checks the grant again | `502` with the worker's words (`503` while the workers rebalance) |
| audit the outcome | `500` |

A cross-site form cannot set the `X-Kaas-Ui` header, and a cross-site `fetch`
that tries to set it triggers a CORS preflight that kaas-ui never approves.
Together with the `SameSite=Lax` cookie, that is the CSRF defence.

A write is two audit lines sharing an `id`. Both go to the same sink as reads,
so one query finds both:

```json
{"at":"…","id":"w-1729591000000-3","phase":"intent","subject":"Cgx…",
 "displayName":"Jane","roles":["connect-operators"],"environment":"prod",
 "target":"connect","name":"sink-orders","operation":"connector.restart",
 "parameters":{"includeTasks":true,"onlyFailed":false}}
{"at":"…","id":"w-1729591000000-3","phase":"outcome", … ,"ok":true}
```

`parameters` have secrets removed before they are logged. A failed outcome
carries the far end's error verbatim.

### What the UI shows

A connector's detail response carries `writes`: every operation in the ceiling,
each with its `tier`, whether it is `allowed` for this caller, and if not, which
gate `refusedBy` (`anonymous`, `notEnabled` or `forbidden`) and the reason. The
page draws its buttons from this list, so a target without a ceiling shows no
write UI. Every write goes through one dialog (`web/src/components/write-dialog.tsx`).

### Kafka writes

None exist yet. Adding one touches these places:

- **`Operation`** in `gate.rs`: the new operation, its tier and its
  `(Resource, Action)`.
- **`Resource::actions`** in `policy.rs`, if it needs a verb the resource does
  not have yet. Startup validation follows automatically.
- **A ceiling on Kafka cluster entries.** Today only Connect clusters have a
  `writes:` block.
- **A granted connection.** The cluster's only admin connection is
  `Admin::connect_read_only`. kaas-lib's mutation ceiling (upstream asks 17–18)
  builds a connection that can emit only the API keys `writes.allow` admits, so
  a handler bug still cannot send `DeleteTopics` to a cluster that allows only
  offset resets.
- **The CI check** in `xtask/src/checks.rs` that requires exactly one
  `connect_read_only` and no `Admin::connect(`, changed in the same commit.
- **The OpenAPI test**, which holds the exact set of non-GET paths.
