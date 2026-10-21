# Phase 9 — Kafka Connect and MirrorMaker 2

*Not in PLAN.md's milestone list; proposed 2026-10-20. Adding it as M9 is the
first decision below.*

**Goal.** Kafka Connect clusters as first-class, dialled members of an
environment — connectors, tasks, plugins, offsets, the topics they touch —
and MirrorMaker 2 replication flows on top of them, including a dedicated MM2
cluster running beside a general-purpose Connect cluster in the same
environment. Read by default; the Connect REST API's writes — restart,
pause, stop, resume, config, offsets, create, delete — behind
[Phase 10](13-phase-10-administration.md)'s three gates (ceiling, grant,
GitOps ownership), with the same construction-site guarantee.

**This phase asks nothing of kaas-lib.** kaas-lib is a Kafka and Kafka-admin
client and stays one. Every Connect REST call lives in `kaas-ui-connect`;
MM2's internal topics are read with the fetches kaas-ui already makes, and
their records are decoded in `kaas-ui-serde` beside the other payload codecs.

## What the others do

Researched 2026-10-20 from kafbat-ui's controller and TypeSpec contract,
Redpanda Console's Go backend, and Apache Kafka's published Connect OpenAPI
document (`kafka.apache.org/42/generated/connect_rest.yaml`, 29 operations).

| | kafbat-ui | Redpanda Console | this phase |
|---|---|---|---|
| config shape | nested under a Kafka cluster | top level, unattached | inside an environment, `kafka_cluster:` reference, **verified** |
| auth | basic, JKS keystore | basic, bearer, TLS | basic, bearer, TLS/mTLS — the credential rule we already have |
| connector list | all clusters, search, sort, CSV | all clusters, one overview | per environment, server-side filter like topics |
| status | connector state + task counts | a derived holistic status | derived states, paused ≠ failed (below) |
| task traces | yes | as titled error entries | yes |
| config | shown, masked by **key-name regex** | shown | masked by the plugin's **declared `PASSWORD` type**, regex as fallback |
| topics / consumer group | yes / yes (`consumerNamePattern`) | `kafka.topic` key only | `/topics` + links into topic, message and lag views |
| offsets | reset only | — | **shown** (`GET /offsets`) |
| create, edit, delete, pause, resume, stop, restart, reset | yes, per-action RBAC | yes (create wizard) | per-action RBAC **and** a per-Connect-cluster ceiling; GitOps-owned connectors refuse edits and render the `KafkaConnector` change instead |
| MirrorMaker 2 | as ordinary connectors | per-plugin create guides | **flows**: alias→alias, mirrored topics, heartbeat age, checkpoint translation |

Neither tool knows a MirrorSourceConnector is anything but a connector. That
gap — and the links into topic, lag and message views the rest of kaas-ui
already draws — is where this phase earns its place.

## What the live Connect cluster answers

Measured 2026-10-20 against Strimzi's `kafka-connect` (2 workers, Kafka
4.2.0, 5 source connectors, 7 plugins incl. the three Mirror connectors):

- `GET /` → `{version, commit, kafka_cluster_id}`; `GET /health` → 200
  `healthy`.
- `GET /connectors?expand=status&expand=info` returns every connector's
  status, config and task ids **in one call**. `expand` is real but absent
  from the OpenAPI document — the server reads the query string directly.
- Status carries a plugin `version` per connector and per task. Displayed,
  never compared (rule 2).
- **Two different 404s.** An unknown path answers
  `{"error_code":404,"message":"HTTP 404 Not Found"}`; an unknown connector
  answers `"Connector nope not found"` / `"No status found for connector
  nope"`. Absence of an endpoint (an older worker without `/offsets`) and
  absence of a connector must be told apart, and the body is the only place
  that says which.
- **Configs come back with secrets in them.** `connect.mqtt.password` would
  be plaintext if it were set inline.
- `GET /connector-plugins/{class}/config` declares a `type` per key, and
  `PASSWORD` is reliable where present: 9 keys on MirrorSourceConnector, 1 on
  the MQTT source. But MM2's real keys are **prefixed** —
  `source.cluster.sasl.jaas.config`, `target.cluster.ssl.truststore.password`
  — while the definition lists them bare. And some secrets are declared
  `STRING` (`connect.mqtt.ssl.key`, `connect.gtfsrt.http.headers`).
- Strimzi's generated NetworkPolicy admits 8083 only from Connect pods and the
  operator. `apps/strimzi/kafka-connect-kaas-ui-networkpolicy.yaml` in
  k3s-cluster widens it for the kaas-ui pod and the code-server dev pod, and
  selects `strimzi.io/kind in (KafkaConnect, KafkaMirrorMaker2)` so a
  dedicated MM2 cluster is covered the day it exists.

## The shape

**MirrorMaker 2 is not a separate kind of thing.** Strimzi's
`KafkaMirrorMaker2` *is* a Connect cluster, with the same REST API, running
three connector classes. A general Connect cluster can host those same three
classes beside a GTFS source. So there is one dialled kind — a Connect
cluster — and "is MM2" is a property of a **connector**, read off
`connector.class`. "MM2 simultaneously with a Connect cluster" then needs no
special case: two Connect clusters in one environment, one dedicated, one
mixed, and the flows view gathers Mirror connectors from both.

```yaml
environments:
  - id: dev
    kafka_clusters:
      - id: strimzi
      - id: kaas
    connect_clusters:
      - id: connect
        name: Kafka Connect
        url: http://kafka-connect-connect-api.strimzi.svc.cluster.local:8083
        kafka_cluster: strimzi          # reference, checked against GET /
        writes:                         # absent ⇒ no ConnectWriter can exist (Phase 10)
          allow: [connector.restart, connector.restart_task]
          gitops:
            owns_connectors: ["*"]      # Strimzi reconciles every KafkaConnector
            source: https://github.com/Woestebanaan/k3s-cluster/tree/master/apps/strimzi/kafka-connectors
            format: strimzi
      - id: mm2
        name: MirrorMaker 2
        url: http://mm2-mirrormaker2-api.strimzi.svc.cluster.local:8083
        kafka_cluster: kaas
    # MM2 alias → cluster. Omitted aliases resolve to the cluster with the
    # same id in this environment; `env/id` reaches another environment.
    mirror_aliases:
      upstream: strimzi
      dr: prod/prod-eu
```

- **`kafka_cluster:` is checked, not trusted.** `GET /` names the Kafka
  cluster id the workers store their state in. Where kaas-ui knows the
  referenced cluster's id, a mismatch is a startup warning and a banner — a
  Connect cluster attached to the wrong Kafka cluster is exactly the mistake
  this catches. Where the id is unknown, the UI says "unverified".
- **Credentials follow the existing rule** — `username`, then `password` and
  `bearer_token` written or read from `KAAS_UI_<CREDENTIAL>_<ENV>_<ID>`; TLS
  reuses `TlsSettings`. A Connect id flattening to the same variable as a
  cluster or registry is the existing startup error.
- **`mirror_aliases` may cross environments**, unlike `schema_registry:`.
  Disaster recovery from `dev` to `prod` is *the* use of MM2; refusing it
  would make the feature useless where it matters.
- **`resources:` keeps `kind: kafka_connect`** for inventory nobody dials. An
  id in both `resources:` and `connect_clusters:` is a startup error naming
  both.
- **Visibility.** A Connect cluster is visible when a caller's selectors
  match its labels (env added structurally, as for clusters). A **flow** is
  visible only when both ends' clusters are visible — a mirrored topic's name
  on a cluster you cannot see is information about that cluster. 404, one
  lookup: `Registry::connect_cluster(env, id, who)`.

## Reading by default, writing by grant

`Admin::connect_read_only` makes writing to Kafka impossible to build. Connect
has no such mode, and the credentials kaas-ui is given will usually be able to
delete connectors. The guarantee therefore has to be the same kind of thing as
Phase 10's — a construction site — in a new crate, **`kaas-ui-connect`**,
created by this phase:

- **`ConnectClient`** (9a) has one typed method per read endpoint below and
  **one private `get_json`**. No `request(method, …)`, no generic passthrough,
  no `Method` in a public signature. Every caller gets one.
- **`ConnectWriter`** (9d) lives in `kaas-ui-connect/src/writer.rs`, and its
  only constructor takes a Phase 10 `&Grant`. One method per action, each
  checking the grant's action matches — a writer minted for
  `connector.restart` cannot `DELETE` a connector. Its mutating calls are the
  only `.post(` / `.put(` / `.patch(` / `.delete(` in the crate, which is
  Phase 10's invariant 4.
- A Connect cluster with no `writes:` block never constructs a writer: the
  gate refuses before a `Grant` exists, so there is nothing to call it with.
- A unit test against a loopback listener asserts every `ConnectClient`
  method sends `GET`, and every `ConnectWriter` method sends exactly the
  method and path its action names — "both or neither", as `checks.rs`'s
  header says.

**Read endpoints:** `/`, `/health`, `/connectors?expand=status&expand=info`,
`/connectors/{c}`, `/config`, `/status`, `/tasks`, `/tasks/{t}/status`,
`/topics`, `/offsets`, `/connector-plugins?connectorsOnly=true`,
`/connector-plugins/{p}/config`.

### The Connect actions (9d)

Same catalogue shape as Phase 10. Ceiling is per Connect cluster
(`writes.allow`); grants are on resource `connect` (the Connect cluster) and
`connector` (named, `value:` patterns), with kafbat-ui's action names.

| action | tier | changes | RBAC | REST call |
|---|---|---|---|---|
| `connector.restart` | operate | — | `connector: operate` | `POST /connectors/{c}/restart?includeTasks=&onlyFailed=` |
| `connector.restart_task` | operate | — | `connector: operate` | `POST /connectors/{c}/tasks/{t}/restart` |
| `connector.pause` / `.resume` / `.stop` | operate | state | `connector: operate` | `PUT /connectors/{c}/pause` · `/resume` · `/stop` |
| `connector.edit` | edit | config | `connector: edit` | `PUT /connectors/{c}/config` |
| `connector.create` | edit | existence | `connect: create` | `POST /connectors`, preceded by `PUT …/config/validate` as the dry run |
| `connector.delete` | destroy | existence | `connector: delete` | `DELETE /connectors/{c}` |
| `connector.reset_offsets` | destroy | offsets | `connector: reset_offsets` | `DELETE /connectors/{c}/offsets` — connector must be STOPPED |
| `connector.alter_offsets` | destroy | offsets | `connector: reset_offsets` | `PATCH /connectors/{c}/offsets` — connector must be STOPPED |
| `connector.reset_topics` | operate | — | `connector: operate` | `PUT /connectors/{c}/topics/reset` |

The **tier** is what a mistake costs, and decides confirmation only. What a
write **changes** decides ownership: a connector Git owns refuses a write
that changes an aspect its Git declares, and a restart changes none. Which
aspects are declared depends on the tool keeping connectors in Git, so it is
configured, not assumed — see below.

`PUT /admin/loggers` stays out (Phase 10's out-of-scope list). The offset
actions do not stop the connector for you: the form says the connector must
be STOPPED, offers `connector.stop` as a separate, separately-authorized
step, and passes Connect's 400 through verbatim if it is not.

### GitOps ownership for Connect

Three statements per Connect cluster, none naming a tool:

```yaml
writes:
  gitops:
    owns_connectors: ["*"]          # which — or a generated ownership_file
    declares: [config, existence]   # what of them; absent ⇒ the format's answer
    format: connector_json          # how Git holds them: rest | connector_json | strimzi_kafka_connector
    source: https://…               # where, for the link
```

| Git holds connectors as | `format` | declares by default |
|---|---|---|
| one `{name, config}` JSON file each, applied by a pipeline (CI `curl`, kcctl, Terraform) | `connector_json` | config, existence |
| Strimzi `KafkaConnector` resources (`strimzi.io/use-connector-resources: "true"`) | `strimzi_kafka_connector` | everything |
| anything else — a MirrorMaker 2 declaration, a tool with no renderer | `rest` | everything |

- What a format cannot hold — no connector file says "restart task 3" —
  renders as the REST call, with a note saying so.
- MirrorMaker 2's connectors are generated from one declaration, so Git owns
  all of them (`owns_connectors: ["*"]`). They are not `KafkaConnector`s even
  under Strimzi, and the fixture's Connect cluster is `rest`.
- Connect's 409 during a rebalance is a **503 with `Retry-After`**, not the
  ownership 409 — different status codes for "Git owns this" and "try again
  in a second".

## Secrets

Redaction happens in `kaas-ui-connect`, before a DTO exists. The raw config
never reaches the API crate, the audit log or the browser.

1. **Declared type, suffix-aware.** A key is secret when it, or any suffix of
   it on a `.` boundary, is a `PASSWORD` key in that plugin's definition. That
   is what catches `source.cluster.sasl.jaas.config`,
   `producer.override.ssl.key.password`, `consumer.override.…`.
2. **Name fallback** for plugins that declare secrets as `STRING`: kafbat's
   list, anchored at the end (`password`, `secret`, `token`, `key`,
   `credentials`, `passphrase`, `jaas.config`, `aws.access.*`,
   `connection.uri`, `headers`). `key.converter` survives because the match is
   on the last segment.
3. **Config-provider references stay visible.** `${file:/x:pw}` is a pointer,
   not a secret, and seeing it is how you know the deployment is doing it
   right.
4. **Fail closed.** If the plugin definition cannot be fetched, layer 2 alone
   applies and the config panel says so.

Applied to `/config`, the configs inside `/tasks`, and `info.config` from the
expanded list. Task traces are shown unredacted — they are stack traces, and
masking them would hide the error — which is a known residual (a JDBC URL in
an exception message) and goes in the doc, not in a regex.

## Connector state

Connect reports a connector state and a state per task. Rendered as one of
these, not as green/red:

| state | when |
|---|---|
| running | connector RUNNING, every task RUNNING |
| degraded | connector RUNNING, some tasks FAILED |
| failed | connector FAILED, or every task FAILED |
| paused | connector PAUSED — **not** a failure, somebody chose it |
| stopped | connector STOPPED |
| unassigned / restarting | during a rebalance, shown as transient |
| no tasks | RUNNING with zero tasks — a misconfiguration, its own row |

Same lesson as lag's four states: Redpanda's version renders paused and failed
in the same column, and that is the one confusion this must not have.

## HTTP

```
GET /api/environments/{env}/connect-clusters
GET /api/environments/{env}/connect-clusters/{id}                       info, health, counts, kafka-cluster check
GET /api/environments/{env}/connect-clusters/{id}/connectors            one expand call, filtered and paged server-side
GET /api/environments/{env}/connect-clusters/{id}/connectors/{name}     status, tasks, redacted config, topics, offsets
GET /api/environments/{env}/connect-clusters/{id}/plugins
GET /api/environments/{env}/mirrors                                     flows (9b)
GET /api/environments/{env}/mirrors/{source}/{target}                   one flow (9b, 9c)

# 9d — built 2026-10-21: one shape, a POST with a JSON body, for every write
POST …/connect-clusters/{id}/connectors                                  connector.create   {name, config, dryRun?}
POST …/connect-clusters/{id}/connectors/{name}/{operation}               restart | pause | resume | stop | reset-topics
                                                                         | edit {config, dryRun?} | delete {confirm}
                                                                         | reset-offsets {confirm} | alter-offsets {offsets, confirm}
POST …/connect-clusters/{id}/connectors/{name}/tasks/{t}/restart         connector.restart_task
```

Action-style rather than the REST verbs Connect itself uses — the plan had
`PUT …/config`, `DELETE …/{name}`, `PATCH …/offsets`. A destroy-tier write
carries a typed confirmation, and `DELETE` bodies are badly supported by the
clients and proxies between a browser and kaas-ui; one shape also means one
guard, one client function, and an openapi test that lists exactly three
non-GET paths.

Write routes follow Phase 10's sequence unchanged: Origin check, 404 for
invisible, 403 for visible-but-not-granted, 409 for owned, typed confirmation
for destroy, audit intent, execute through the `ConnectWriter`, audit outcome.
Configs reach the audit log redacted — and the edit form's `******` is merged
back from the worker's own copy inside the writer, into a type that cannot be
serialised, so the secret it restores reaches the worker and nothing else.

- Connect unreachable, timed out, or **401/403 from Connect** → 502. Same
  trap as `Error::Authentication`: Connect rejecting kaas-ui's credentials must
  not log the user out.
- 409 (worker rebalancing) → 503 with `Retry-After`.
- A missing endpoint (path-404, by body) → the field is `absent` and the UI
  hides that panel. A missing connector → 404.
- `/topics` with topic tracking disabled answers an error; that is a
  per-connector `absent`, not a failed page. Measure the status code in
  `live` before writing the match.
- Fleet-wide lists are `PerItem` per Connect cluster — one dead Connect
  cluster is one error row, as in Redpanda's overview.
- Lazy and cached like cluster snapshots: the expanded connector list per
  Connect cluster with a short TTL, fetched on first use, never at startup,
  never from `/health`. Timeouts default to Redpanda's: 15 s connect, 6 s
  request.

## Links to what already exists

This is the payoff, and it is cheap because the other end is already built:

- **connector → topics** (`/topics`) → topic detail, message browser;
- **sink connector → consumer group** `connect-<name>`, unless
  `consumer.override.group.id` says otherwise → Phase 5's lag view;
- **topic detail → "written by" / "read by"**, from an index built off the
  cached connector lists;
- **source connector offsets** (`/offsets`) — the source-side position,
  which neither kafbat nor Redpanda shows.

## MirrorMaker 2

### 9b — flows, from REST

A Mirror connector is one whose `connector.class` is
`org.apache.kafka.connect.mirror.Mirror{Source,Checkpoint,Heartbeat}Connector`
(a class name, not a version — rule 2 is not involved). Across every visible
Connect cluster in reach:

- group by `(source.cluster.alias, target.cluster.alias)` into a **flow**,
  each holding up to three connectors **and the Connect cluster hosting each**
  — the same flow can be split across clusters, and saying where each piece
  runs is the point of the simultaneous case;
- a missing piece is shown as missing: no checkpoint connector means no
  offset translation, so a consumer failing over will start from
  `auto.offset.reset`;
- resolve aliases through `mirror_aliases`; an unresolved alias is shown by
  name without links;
- read `replication.policy.class` and `replication.policy.separator`.
  `DefaultReplicationPolicy` maps `t` ↔ `<alias><sep>t`,
  `IdentityReplicationPolicy` maps `t` ↔ `t`; a custom class gets the table
  without the mapping, and says why;
- **mirrored topics**: MirrorSource's `/topics` (target-side names), mapped
  back through the policy, each row with partition count on both sides —
  drift is a finding — and links to both topic pages;
- `topics`, `topics.exclude`, `groups`, `groups.exclude` shown as the flow's
  filter, verbatim. They are regexes for MM2 to evaluate; kaas-ui does not
  evaluate them.

### 9c — what the clusters themselves say

MM2 writes three internal topics, and kaas-ui reads both clusters already.
This is the part that works for **dedicated-mode MM2**
(`connect-mirror-maker.sh`), which has no public REST API at all:

| topic | lives on | record | gives |
|---|---|---|---|
| `heartbeats`, `<alias>.heartbeats` | each side | key `(sourceAlias, targetAlias)`, value `timestamp` | **end-to-end liveness**: age of the newest replicated heartbeat |
| `<source>.checkpoints.internal` | target | key `(group, topic, partition)`, value `(upstreamOffset, downstreamOffset, metadata)` | **failover readiness**: where each group would resume on the target |
| `mm2-offset-syncs.<target>.internal` | source by default (`offset-syncs.topic.location`) | key `(topic, partition)`, value `(upstreamOffset, offset)` | a **lower bound** on how far replication has got |

- **The decoding lives in `kaas-ui-serde`.** kaas-lib fetches the records
  as bytes, as it does for any topic; turning bytes into fields is payload
  decoding, the same job as Avro. The encoding is small — big-endian
  integers, strings as an INT16 length plus UTF-8. `Heartbeat` and
  `Checkpoint` open with an INT16 format version and only version 0 exists:
  the decoder reads version 0 and renders any other value as "unrecognised
  record format N", never guessing. That is a record's own format, not a
  broker or protocol version, so rule 2 is not engaged — and an unrecognised
  value is shown as absence, which is the part rule 2 leaves to the UI.
- **Offset syncs are sparse by design** (emitted when the offset gap exceeds
  `offset.lag.max`), so replication lag from them is "at most N behind as of
  the last sync", rendered as a state, never a bare number.
- **Which heartbeat topic a flow populates is measured, not assumed.** The
  live fixture settles which side's `heartbeats` the flow's heartbeat
  connector writes and which side receives `<alias>.heartbeats` before the
  panel is wired.
- **Discovery**: on any visible cluster, topics named
  `*.checkpoints.internal`, `mm2-offset-syncs.*.internal` and
  `<alias>.heartbeats` imply a flow. A flow found this way and not via REST
  is labelled "discovered from topics — no Connect API, task state unknown",
  which is the honest description of a dedicated-mode deployment.

## Frontend

- **Sidebar and fleet card**: a Connect cluster stops being "not probed" and
  gets a health dot and connector counts by state.
- **Connect cluster page**: header (version, commit, Kafka cluster check),
  connector table (name, class, type, state, tasks running/total, topics),
  plugins tab.
- **Connector page**: Overview / Tasks (worker, state, trace) / Config
  (redacted, provider references visible, grouped by the plugin definition's
  `group`) / Topics / Offsets. Mirror connectors get a "part of flow
  `a → b`" chip.
- **Mirrors page** per environment: one card per flow, `source → target`,
  its three connectors and where each runs, then the mirrored-topic table;
  9c adds heartbeat age and the checkpoint table per group.
- **Topic page**: "mirrored from" / "mirrored to" and "written by connector"
  badges.
- **Actions (9d)** through Phase 10's `WriteDialog`, shown only when
  `/api/me` says the caller can perform them on that connector: restart and
  restart-failed-tasks on the connector header, restart per task row,
  pause/resume/stop, edit config (redacted values are kept, not resubmitted
  as `******` — the writer merges unchanged secrets back server-side),
  offsets, delete. Owned connectors show "Managed in Git", the rendered
  `KafkaConnector` / `KafkaMirrorMaker2` change, and only the operate actions.
- On the Mirrors page, actions act on the underlying connector and say which
  Connect cluster it runs on — restarting "the flow" is three connectors
  across possibly two clusters, and the dialog lists them rather than hiding
  it.

## Live fixtures

Both need SCRAM users with write ACLs on the target and are changes to
`k3s-cluster` that **write to the clusters** — approve before applying.

- **A — dedicated:** a `KafkaMirrorMaker2` `mm2`, `strimzi → kaas`, its own
  Connect cluster, `DefaultReplicationPolicy`, topics `gtfs-.*`, checkpoint
  and heartbeat connectors on, `sync.group.offsets.enabled: true`. kaas as an
  MM2 *target* needs CreateTopics, CreatePartitions and the config APIs; if it
  cannot host it, that is a kaas finding to file, and the fallback is a
  second Strimzi-hosted alias with an explicit `topics` regex so the
  prefixed topics do not re-match.
- **B — co-hosted:** the three Mirror connectors as `KafkaConnector` CRs on
  the existing `kafka-connect`, `kaas → strimzi`, topic `kaas-canary-v1`.
  That cluster then runs five ordinary and three mirror connectors side by
  side, and the two flows together are the simultaneous case.

## Acceptance

`cargo xtask live` grows, against the two Connect clusters above:

- every connector a direct `GET /connectors?expand=status` lists appears,
  with the same state — the Connect API itself is the oracle here, as
  `livetest probe` is for the brokers;
- `kafka_cluster:` verification passes for both, and a deliberately wrong
  reference in a test config warns;
- no config value of a `PASSWORD`-declared key, prefixed or not, appears in
  any response body — asserted by fetching the raw config directly and
  grepping our output for each secret value;
- an unknown connector is 404; an unknown endpoint on a worker is `absent`,
  not an error;
- both flows render, each piece attributed to the Connect cluster hosting it;
  mirrored topics map through the policy and link to real topics on both
  sides;
- 9c: heartbeat age under 60 s on both flows; a group consuming on the source
  has a checkpoint on the target;
- a third, unreachable Connect URL in the test config is one error row and
  does not slow the others;
- `cargo xtask ci`'s invariant fails on a planted `.post(` in
  `kaas-ui-connect` outside `writer.rs`.

9d adds, against a throwaway connector (a `FileStreamSource` or a
`MirrorHeartbeatConnector` named `kaas-ui-live-*`, created and deleted by
the test on a Connect cluster *not* owned by Git):

- with no `writes:` block on the Connect cluster, every write route is 403
  and no request other than `GET` reaches the worker;
- restart → pause → resume → stop → reset offsets → delete, each producing an
  intent and an outcome audit line, and each visible in the next `GET`;
- on the Strimzi-owned `kafka-connect`, restart succeeds and edit is 409
  with a `KafkaConnector` manifest;
- reset offsets on a RUNNING connector returns Connect's own refusal;
- a role with `connector: operate` on `gtfs-*` can restart a `gtfs-` connector
  and gets 403 on `mqtt-home-connector`.

## Order

Agreed 2026-10-20: interleaved with Phase 10, **9a → 10a → 9b/9c/9d and
the rest**. The whole sequence is in [README.md](README.md#order).

1. **9a** — `kaas-ui-connect` (`ConnectClient`, reads only), config,
   registry lookup, redaction, the Connect pages, the links. First, because
   it needs nothing from kaas-lib or from Phase 10, it is what retiring
   kafbat-ui for Connect waits on, and 10a's pilot write runs through it.
2. *Phase 10a* — the write gate, proven on `connector.restart`, which is
   9d's first action landing early.
3. **9b** — flows from REST, fixtures A and B.
4. **9c** — the three decoders in `kaas-ui-serde`; heartbeats, checkpoints,
   offset syncs, topic-based discovery. No upstream dependency.
5. **9d** — the remaining Connect writes on 10a's gate: restart task, pause,
   resume, stop, edit, create, delete, offsets. Independent of 9b/9c.

9a–9c only read. Independent of Phase 8; either can go first. 8's cross-cluster topic search
is where a mirrored topic's two names would want to be one row, which argues
mildly for 9b before 8 finishes.

## Decisions to make before starting

1. **M9 in PLAN.md?** PLAN.md is the stable design and has no Connect.
   Either it gains an M9 or this stays a docs-only phase.
2. ~~Separate crate vs. kaas-lib sibling~~ — **decided 2026-10-20**: kaas-lib
   stays Kafka-only; the Connect client, reads and writes, is
   `kaas-ui-connect`. The only version-sensitive knowledge in it is "this
   endpoint path-404s", which is the absence handling rule 2 permits.
3. **Config visibility.** Redacted configs still name hosts and URLs. Is
   seeing a Connect cluster enough to see its connector configs, or does
   that want its own role scope?
4. ~~Where MM2's record decoders live~~ — **decided 2026-10-20**:
   `kaas-ui-serde`. kaas-lib stays a Kafka and Kafka-admin client.
5. **Fixture A's target** — kaas, as a test of kaas, or Strimzi, as a test
   of kaas-ui only.
