# What is built

Phases 0–7 are done, and their plan files are gone — a plan for work already
finished is a document that can only go stale. What could not go
with them is here: **what each phase decided differently from its plan**, the
numbers that were measured rather than predicted, and the things that were
left unproven and are still unproven.

The code is the description of what the software does. This file is the
description of *why it is shaped that way*, for the decisions where the shape
is surprising.

| phase | what it built |
|---|---|
| 0 — skeleton | config, registry, lazy connect, `/health`, fleet view, embedded frontend, distroless image, CI |
| 1 — capabilities | the capability projection, `source` naming the broker, the four degradation components, brokers, log dirs, configs |
| 2 — topics | server-side filter/sort/page, detail, partitions, placement grid, configs, offsets |
| 3 — messages | seven seek modes over SSE, virtualised list, detail panel, URL state |
| 4 — auth | OIDC via Dex, encrypted-cookie sessions, roles in kafbat's shape, the access audit |
| 5 — groups | four group kinds, members, committed offsets, lag as states rather than a subtraction |
| 6 — schema registry | one registry per environment, Avro/Protobuf/JSON Schema, the codec chip, the schema browser, the payload filter over decoded values |
| 7 — read-only admin | ACLs, client quotas, SCRAM users, reassignments, the transaction inspector, and `CapabilityTab` at last |

Still open: [Phase 8](09-phase-8-cross-cluster.md).

## Acceptance, as it stands

```sh
cargo xtask ci      # green: fmt, clippy, 275 unit tests, five invariant greps
cargo xtask live    # green: 71 assertions against kaas, strimzi and dead
cargo xtask login   # 11 assertions, a real login. Dormant — see below.
```

The invariant greps are the ones that matter, because they are what keeps the
architecture from being edited away: exactly one `Admin::connect_read_only`
(today at `crates/kaas-ui-core/src/registry.rs:258`), no Kafka version literal
anywhere, no committed `xtask link` fence, and sign-in being an `<a href>`
rather than a `fetch` — see Phase 4.

Unit tests: 80 in `kaas-ui-api`, 95 in `kaas-ui-core`, 40 in `kaas-ui-auth`,
47 in `kaas-ui-serde` (38 unit, 9 against a stub registry), 10 in
`kaas-ui-server`, 3 in `xtask`.

`cargo xtask live` gained a third outcome in Phase 6: **skip**. The seek-mode
assertions name concrete offsets — 1000000 and 1000037 — because the properties
they check are off-by-one properties, and an off-by-one on a "somewhere near
the end" offset is invisible. The cost is a fixture that ages, and `kperf-bench`
on `kaas` has now aged out of retention. That is an unmet precondition, not a
regression, and reporting it as a failure would train everyone to ignore the
run. It is reported as a skip naming the range the topic still holds.

## Measured

From Phase 0's acceptance run, re-measured on 2026-08-03 by the run above.
PLAN.md §10 predicted ~25 MB and ~15 MB RSS; both are comfortably beaten, and
these numbers are what that claim now rests on.

| | at acceptance | today |
|---|---|---|
| container image, distroless + static musl | **5 MB** (predicted ~25 MB) | measured per release by the release job |
| release binary (glibc, `lto = "thin"`, stripped) | 9.6 MB | — |
| resident memory, idle, three clusters configured | **8.7 MiB** (predicted ~15 MB) | — |
| time to serve after start | 53 ms | 52.6 ms |
| `GET /health` | 0.3 ms | 0.6 ms |
| `GET /api/environments`, with `dead` failing in the background | 1.2 ms | 1.8 ms |

The image size comes from the release job, which measures it on every push and
writes it into the job summary — so it is checked rather than remembered.

## Phase 0 — the decisions

**The fleet view reads `Cluster::snapshot()` and nothing else.** PLAN.md §5
describes the fleet card as "`DescribeCluster` plus `Metadata` per cluster".
Against the actual target clusters that does not work: `kaas` does not
implement api key 60. The snapshot carries brokers, controller id, cluster id,
every topic and every partition's replicas/ISR/offline set — everything a fleet
card renders — and it works on both clusters. Phase 1 added `describe_cluster`
as *enrichment* that degrades visibly when absent. One optional field
(`is_fenced` per broker) is not worth making the landing page depend on an api
key half the fleet does not answer.

**Config reload polls the file rather than watching it with `notify`.** A
Kubernetes ConfigMap is a symlink to a `..data` directory that is swapped
atomically, so an inotify watch on the file's inode never fires. Watching the
*directory* works; so does comparing the file's bytes every five seconds, with
one fewer dependency and no platform-specific behaviour to get wrong. `notify`
is not in the dependency tree.

**The capability answer is a list, not an object map.** PLAN.md sketches
`{"consumerGroups": "available", …}`, whose values are sometimes a string and
sometimes an object. A `Vec<FeatureEntry>` says the same thing and types
cleanly in both Rust and TypeScript.

**The TypeScript client is hand-written, not generated by Orval.**
`cargo xtask docs` writes `docs/openapi.json` and `web/src/api/types.ts`
mirrors it. Generating it is worth doing once the schema stops moving; until
then a file a human reads is easier to keep honest than a generated one nobody
looks at.

**Reload swaps the registry rather than mutating it**, reusing existing
`Arc<ClusterHandle>`s by id, so adding a cluster does not disturb the
connections of the ones that did not change —
`registry::tests::reload_keeps_the_handles_it_did_not_change`.

## Phase 1 — the decisions

**The projection contains no version logic**, and that is enforced rather than
intended: one table, one `match`, no arithmetic on version numbers, and the
no-version-literal grep in `cargo xtask ci` fails the build if that changes.
Sixteen features, named in kaas-ui's vocabulary rather than Kafka's — those
names are a UI contract, so adding an api key to an existing feature must not
rename it.

**`source` is not decoration, and it is interim.** kaas-lib's version table is
per *connection*, deliberately: brokers mid-rolling-upgrade genuinely disagree,
and a cluster-wide table would be wrong during exactly the window when being
right matters. There is no `cluster.capabilities()` to project from, and
fabricating one by picking whichever broker answered produces a UI whose tabs
flicker — and which looks perfect on every single-broker fixture. So the table
is read from an explicitly named broker and the answer says which one; the
frontend renders "as reported by broker 1".

When [upstream ask 1](reference/upstream-asks.md) lands, `source` becomes
`{"kind": "agreed"}` or `{"kind": "disagreed", "brokers": {…}}`, and the UI
gains the thing no other Kafka tool has: "3 of 5 brokers support this, upgrade
in progress".

**Hidden tab, live route.** Tabs are rendered from capabilities, so a cluster
that does not answer `DescribeClientQuotas` shows no quotas tab rather than a
tab that errors on click. The *routes* still exist and render the explanatory
panel, so a URL shared from a Strimzi cluster and opened against `kaas`
degrades into an explanation rather than a dead end. This distinction is the
whole of PLAN.md §2's "deciding what absence looks like", and it is the one
thing in this area that genuinely cannot live in a client library.

All four degradation components were built here rather than deferred, because
all four have a live trigger today: `UnsupportedApiPanel` (`DescribeCluster` on
`kaas`), `UnknownCodeChip`, `ErrorChips` for per-item failures, the
broker-ahead-of-codec note (8 keys on Strimzi), and the undescribable-group
render Phase 5 reuses.

## Phase 2 — the decisions

**Filtering, sorting and paging are server-side**, in `routes/topics.rs`, and
the live run asserts the response row count changes with the query rather than
just the rendering. The fleet has clusters where 5000 topics is a real number.

**Offsets are not fetched per row.** `topic_offset_range` calls `refresh_topics`
first, so calling it per row on a list of 500 topics is 500 metadata refreshes —
and it also will not compile in an axum handler
([upstream ask 10](reference/upstream-asks.md)). The detail page batches through
`list_offsets` with an explicit partition list instead.

**`kaas` reports no topic ids**, so the column is omitted rather than rendered
as empty UUIDs. **Under-replicated is not offline**: `replicas.len() !=
isr.len()` and a non-empty `offline_replicas` are two different problems and get
two different colours in the placement grid.

The headline test from PLAN.md §8 holds on both clusters: describing 15 topics
of which 2 do not exist is `200 OK` with 13 items and 2 errors in the envelope,
and nothing about the page suggests a failed request.

## Phase 3 — the decisions

The phase that changed most between plan and build.

**Four routes, not two.** `messages/stream` is the SSE one; `messages` (one
bounded page) exists for "load more" past the end of a window; and
`messages/{partition}/{offset}` exists because no listing route ever sends a
whole payload — a topic at 1 KB × 10k/s is 10 MB/s the browser would parse and
never draw. The list shows a 256-character preview; the rest is fetched for the
one record someone selected, cached with `staleTime: Infinity` because a record
at an offset is immutable.

**The browser is a tab, not a page.** It had its own route for one release, on
the reasoning that a split pane wants the whole viewport; what that actually
bought was a second place to look for messages and a back button between them.
`…/topics/{t}/messages` still resolves as a redirect carrying its search params
into `…/topics/{t}?tab=messages`. The `messages/tail` route survives — it is the
only bounded, cacheable read of a topic's end, and `cargo xtask live` still
asserts the `div_ceil` spread through it — but nothing in the frontend calls it.

**"No `tokio::spawn` that outlives a response" became "no spawn that *can*
outlive one".** The pump has to be a task rather than an inline stream: the
whole point of the bounded ring is that a slow reader loses old records instead
of stalling the fetch loop, and only a separately-scheduled producer can do
that. What preserves the original property is that the task selects on
`tx.closed()`, so dropping the response drops the scan within a poll. The
acceptance run abandons five streams and watches the slots come back.

**The `scan` events do not map 1:1 onto SSE events.** Records are batched on a
100 ms interval — one event per record saturates the browser's parser long
before the list does — and malformed batches ride inside `messages` as a row
kind rather than in `error`. Both are in
[reference/http-contract.md](reference/http-contract.md).

**A shutdown has to end the streams, not wait for them.** Found by running,
after the process refused to exit on SIGTERM. `with_graceful_shutdown` stops
accepting and waits for in-flight connections, and an SSE response is an
unbounded body — a live tail's stream completes when the client leaves. A
shutdown is neither, so the drain waited on a response that would never finish.
In Kubernetes that is the full `terminationGracePeriodSeconds` on every
rollout, with every open stream severed by SIGKILL and no `phase: done` to tell
the client a deploy happened rather than a network fault. The fix is a latch
every open stream watches: 30+ seconds became ~50 ms, and the live run asserts
it with three streams open.

**A proxy in front will buffer the stream unless told not to.**
`Cache-Control: no-transform` and `X-Accel-Buffering: no`, and they are not
optional: through the Cloudflare tunnel that fronts this cluster the browser
received *nothing* without them, while the same stream through code-server
alone delivered 4.4 KB in five seconds. Every layer reported success — the
request simply stayed open and empty, which is the hardest kind of failure to
attribute.

**Time seeks are reported, not interpreted.** `kaas` holds no timestamp index
and answers a time seek with no offset at all — legitimate, and
indistinguishable from "nothing was written since". Rather than guess, the
stream carries a `resolved` block naming what each partition said, and the UI
renders it beside the empty window.

**Three changes were needed in kaas-lib, not one**, released as 0.2.0: the
anchored backward walk (expected), `ScanSpec::following` (not expected — `scan`
from `StartPosition::Latest` finished in seven milliseconds having emitted
nothing, because a partition starting at its own log end is marked finished,
which looks exactly like a working live view of an idle topic), and a fix for
`scan` emitting records *before* its start offset, because a fetch begins at the
batch containing the offset and only the backward walk was filtering. The first
two add public fields to structs that are not `#[non_exhaustive]`, which is why
a set of additive features was a minor rather than a patch.

**`kaas-ui-serde` was not created.** Payload rendering was `Payload::of` in
`kaas-ui-core::dto` — UTF-8 where the bytes are text, hex where they are not,
with the encoding said out loud. That is Phase 3's sniff order minus the JSON
step and minus the per-topic override, and the crate is created by the phase
that fills it rather than up front. It earned its boundary in Phase 6, below,
where `Payload` moved into it and grew a codec, a schema reference and a note.

## Phase 4 — the decisions

**kaas-ui is a public client with no client secret.** PKCE is what proves that
whoever redeems the code started the flow. This replaced the phase's original
`client_secret_file` sketch and removed the ExternalSecret it implied: there is
no kaas-ui secret in Vault because there is no kaas-ui secret. The limit is
worth knowing — Dex enforces a verifier whenever a challenge was issued, but
does not require one to be issued, so the strength of the arrangement rests
entirely on kaas-ui always sending `S256`. That habit is a unit test.

**Sessions are encrypted cookies with no store.** Encrypted rather than merely
signed, because the pending-login cookie carries a PKCE verifier and a verifier
anyone can read is not a verifier. The key is generated at startup, so a
restart ends every session — for a single-replica read-only browser tool, a
better trade than another secret to mount and rotate, and the startup log says
so rather than leaving it to be found.

**No refresh tokens.** `offline_access` is never requested. A refresh token is
a long-lived credential to store, protect and revoke, in exchange for saving
someone one redirect a day on a tool they keep open for twenty minutes.

**Dex is served under kaas-ui's own hostname at `/dex`**, proxied to the
in-cluster Service — ArgoCD's arrangement at `/api/dex`, for ArgoCD's reason:
every browser hop of a login must reach the provider, and this costs no second
DNS record and no second public surface. Nothing is stripped in the proxy,
because Dex serves every endpoint under its issuer's path.

That proxy forwards whatever method the browser sends, which **retired the
"every route is a `GET`" check**. The read-only guarantee never depended on it:
it is the single `Admin::connect_read_only` construction site, and nothing
reachable through the proxy has an admin client at all.

**The hops kaas-ui makes itself go to the Service, not the hostname**
(`auth.internal_url`, since 0.7.5; defaulted rather than remembered since
0.7.7). Discovery, the token exchange and the key
set are dialled in-cluster; only `authorization_endpoint` and the GitHub
connector's `redirectURI` — the two hops a *browser* makes — stay on the public
issuer. ArgoCD splits in the same place, between `--dex-server` and `/api/dex`.

Without the split the deployment could not cold-start, and until 0.7.5 it never
had. The tunnel routes `kaas.smeding.cloud` to kaas-ui and kaas-ui serves
`/dex`, so startup discovery was a request to a process that was not listening
yet: `502`, exit, forever. It went unnoticed for eleven releases because a
rolling deploy always left the previous pod Ready to answer the new one's
discovery; a node restart on 2026-08-04 removed the predecessor and turned a
latent cycle into a permanent crash loop. The trap in fixing it is rewriting
the endpoints uniformly — no browser resolves `dex.dex.svc.cluster.local`, and
a uniform rewrite breaks login at the first redirect, on the cluster only.

**And it is a default, not a field to remember — which is what ArgoCD does.**
0.7.5 shipped `auth.internal_url` as an explicit setting with a startup warning
when it was missing, which left the outage one forgotten line away and made the
warning read out a derivation the program could have performed. Reading
ArgoCD's own manifests settled it: `argocd-cmd-params-cm` ships with **no
`data:` block at all**, every dex key is wired `optional: true`, and
`server.dex.server` defaults in the binary to `http://argocd-dex-server:5556`.
The server-side address is a fixed property of the Dex shipped alongside, with
no relationship to `url:` in `argocd-cm` — so no value of the public URL can
put it on the boot path. That is why ArgoCD never had this bug.

kaas-ui's equivalent is `dex.upstream`: configuring a `dex` block *is* the
statement that there is a local Dex, and the one this deployment proxies is
the one it should talk to. The issuer's path is appended only because kaas-ui
lets Dex live under one, where ArgoCD fixes it at `/api/dex`. A deployment
authenticating against somebody else's IdP has no `dex` block, so nothing is
assumed on its behalf — which is the case that made deriving from the *public
URL* the wrong shape, and it needs no origin-matching predicate to avoid.
`Config::auth_warning` is gone; there is nothing left to warn about.

**Two exit criteria were retired rather than met.** The Vault client secret,
above. And "no provider-specific code anywhere", which was true and never
falsifiable: Dex terminates GitHub, Google, Entra, LDAP or SAML and presents
all of them as one issuer with a `groups` claim. The counter-example is next
door — `kafbat-ui` carries a GitHub-specific path with its own REST calls to
`/user` and `/user/orgs`, because GitHub OAuth Apps issue opaque tokens with no
`id_token`, no discovery document and no groups claim.

**A third was retired on a measurement: RP-initiated logout.** The phase file
said "Dex supports the endpoint; kaas-ui does not call it", and that was wrong.
Dex v2.45.1 advertises no `end_session_endpoint`, and `/dex/end_session`,
`/dex/logout`, `/dex/auth/logout` and `/dex/session/end` all answer 404;
upstream, [dexidp/dex#1697](https://github.com/dexidp/dex/issues/1697) is open
and states that Dex has no concept of sessions at all. The silent second login
is **GitHub's** session, not Dex's — and the implementation proposed upstream
only forwards to the upstream provider's end-session endpoint, which GitHub,
being OAuth2, does not have. No relying party can end it. `POST /auth/logout`
ends this session on this device, which is the whole of what is available.

**The login acceptance is a Deployment, not a container.** The phase asked for
a Dex container in CI, reasoning that the alternative was depending on GitHub
from a test. That was a false choice, and the same one this project already
declined for Kafka: `cargo xtask login` runs inside the cluster against
`dex-test` — ClusterIP, two static-password users, absent from the tunnel — so
it works identically on the ARC runners and on the development box. A container
would have run only in CI, because there is no Docker here.

Two users, because one can show that a permission works and never that its
absence bites. That is what finally proved the grant boundary: same fleet for
both, `200` from `/messages/tail` for `acceptance-admin`, `403` for
`acceptance-viewer`.

**The fixture is currently out of the cluster**, removed on 2026-08-04 after
the run above went green. Nothing about the acceptance was deleted — the
command, the config and the eleven assertions are all still here, and
re-syncing `apps/dex-test/` from the cluster repo's history brings them back
in a minute. What that costs meanwhile is the *regression* value: the login
flow and the grant boundary are proven as of today and unguarded from
tomorrow. `cargo xtask login` says so rather than failing obscurely, which is
the only reason leaving it in place is honest.

**kaas-ui draws the connector chooser, not Dex** — added 2026-08-07, after the
second connector made the omission visible. Dex with more than one connector
serves an interstitial page listing them, and it is the one screen of a login a
deployment cannot style. `auth.connectors` is a list of `{id, name}`; the
sign-in screen and the user menu draw a button each, and
`/auth/login?connector=<id>` forwards it as Dex's `connector_id`, which is read
off the authorization request and turned into a redirect straight to that
connector.

Measured against the deployed Dex v2.45.1 rather than reasoned about: with
`connector_id=microsoft` the authorization endpoint answers `302` to
`/dex/auth/microsoft`, PKCE, `state` and `nonce` intact; with an id it does not
know, `400`. The full chain through kaas-ui was walked the same way.

Two things this costs, both deliberate. The ids now live in two config files
that have to agree, and nothing checks that at startup — reading Dex's
configuration would put a second service on the boot path for a cosmetic
feature, so an unknown id is instead a `400` from *us*, with the id in the
message, before anything is sent to the provider. And an empty `connectors:`
had to keep meaning "one unnamed button, let the provider ask" rather than "no
way to sign in", because that is what every deployment predating this has.

The rule it does *not* break is `kaas-ui-auth` being provider-blind: a
`Connector` is a label and an opaque string, and no code in the workspace
branches on which one it is. Adding a third is still a config change rather
than a release.

**The harness passed twice while proving nothing**, which is the most useful
thing this phase produced. kaas-ui's cookies are `Secure` — correct, the
browser is on https — so a loopback acceptance run's RFC-6265 cookie jar drops
them; and the callback's "arrived without having started a login" branch
redirects to `/`, *where a successful login also lands*. Version one asserted
on the landing URL and was green having verified nothing. Version two parsed
`data.items` from a route that answers `items`, and read every caller's fleet
as empty — which looks exactly like access control working. Both now guarded:
the run drives each hop itself and asserts on the session cookie's existence,
and the anonymous case is only meaningful because the authenticated one
immediately after it is non-empty.

## Phase 5 — the decisions

**Four kinds, four components, one discriminated union.** `GroupDescription`'s
variants are genuinely different — a classic group has a generation and a
negotiated assignor, a KIP-848 consumer group has a group epoch, an assignment
epoch and a *server-chosen* assignor — and flattening them into one
all-optional TypeScript interface moves that knowledge somewhere the compiler
cannot check it. `Unrecognized` is a **successful** description of an
undescribable group, rendered by the Phase 1 panel.

**Lag has five states in the code, not the plan's four.** `NoCommit` (`—`),
`EmptyPartition` (`∅`), `CaughtUp` (`0`), `Lagging` (the number) — and
`Unknown` (`?`), added because the log end can fail to read, and rendering that
as any of the other four is the same class of lie the table exists to prevent.

**Group detail is fetched per route rather than on row expansion.** Same
property the plan wanted — nothing fetches offsets for the whole list — reached
by navigation instead of by an expander.

**No offset-reset view.** It appears in kafbat-ui and it is a write; there is no
code path here that could perform one.

## Phase 6 — the decisions

**A registry is declared once and referenced by id, and that is the whole
design.** `schema_registries:` is a top-level block; a cluster names one with
`schema_registry: dev`. The binding is that explicit line and **not** the `env`
label, which stays free-form display metadata — a typo in a label would point a
cluster at nothing, while an unknown registry id fails startup with the id in
the message *and* the list of ids that were declared.

Sharing is therefore a property of **construction**: `Registry::from_config`
builds one `RegistryHandle` per declared registry and hands the same `Arc` to
every cluster that names it. There is no way to build a second cache for `dev`,
and no way to hand `dev`'s decoder `prod`'s settings. `two_clusters_naming_one_
registry_hold_one_client_between_them` asserts the pointer equality, and the
stub-registry test asserts the consequence: after the first cluster resolves
schema id 1, the second decodes a record carrying it with **zero** requests
reaching the registry.

**One library, and the TLS feature is not its own.** `schema_registry_converter`
4.10 resolves ids and decodes all three formats, because all three are the same
Confluent wire format. Neither of its TLS features could be used: `native_tls`
wants OpenSSL, which the distroless image has none of, and `rustls_tls` maps to
`reqwest/rustls`, which forces `aws-lc-rs` and would turn the two-line musl
builder in the Dockerfile into a cmake project. So the converter is built with
no TLS feature at all and TLS comes from `reqwest/rustls-no-provider` on the
one copy of reqwest the graph shares — with `kaas-ui-serde` installing the
`ring` provider itself, behind a `Once`, in `RegistryHandle::new`. The crate
that needs TLS arranges for it; a `main` that forgot would produce a registry
that worked over HTTP and failed to build a client over HTTPS.

The tree does carry **two** reqwest majors: 0.12 under `openidconnect`, 0.13
under the converter. Not free, and not worth forcing: pinning them together
means either an OIDC library that is behind or a schema library that is.

**`jsonschema` 0.49 is not in the tree, and that is the plan being taken
seriously rather than ignored.** `docs/00-foundations.md` listed it for
display-time conformance; the converter's `json` feature already brings a
validator, and using it means one library resolving *and* validating rather
than two disagreeing about which schema a record was checked against. The
non-conforming path is asserted against a stub: a record that parses as JSON
and violates its subject comes back decoded, with a `nonConforming` note.

**Six note kinds, not one error.** A payload that is not what the reader asked
for carries a `note`, and the kinds are kept apart because they want different
things done about them: `decodeError`, `registryUnavailable`,
`registryAbsent`, `registryMisconfigured`, `overrideRefused`, `nonConforming`.
The one the phase plan cared most about is the fourth — a `url` pointing at
Apicurio's native API answers real requests and has no `/subjects`, so every
Avro topic would render as hex with nothing saying why. It is caught on first
use, reported as **configuration** rather than as an outage, and the message
names `/apis/ccompat/v7` as the endpoint expected.

**`RegistryAbsent` is a sixth kind the plan did not have.** A cluster with no
registry that receives a framed payload is neither an outage nor a
misconfiguration — it is `kaas` in the example config, doing exactly what it
was set up to do — and it still needs a sentence saying why that record is hex.

**Connecting is lazy and the backoff is per registry, with no background
task.** Unlike a cluster, a registry has no persistent connection to keep
warm: "connecting" is one ccompat probe. So there is no connector loop, and
`ready()` is a fast path plus a `tokio::sync::Mutex` — ten clusters naming
`dev` are ten callers of one function, not ten schedules. A registry that is
down costs one probe per backoff interval for the whole process, asserted by
decoding ten records against a dead registry and finding the request counter
unchanged. Nothing dials at startup, so `/health` is untouched.

**A `Misconfigured` registry is retried at the ceiling rather than never.**
A wrong path does not heal on its own, but a registry answering 404 while it
starts up does, and a fault that could only be cleared by a restart would be
the wrong trade.

**Apicurio does not return a subject with a schema id, and Confluent does.**
`GET /schemas/ids/1` on Apicurio 3.2.4 answers with the schema and nothing
else, so the chip would read `avro #1` with no subject — most of the useful
information missing. `GET /schemas/ids/{id}/versions` is the ccompat way to
ask, and it costs one request per **id**, cached beside the format and never
fetched again.

**The override is free in one direction, and the raw bytes are what make it
free.** A registry-backed payload carries `raw.hex` beside its decoded value,
so dropping to hex or string in the detail panel is a render rather than a
refetch — and asking for `valueCodec=hex` on a framed topic does not consult
the registry at all, which is the half of the override that has to work while
the registry is down. Upward is refused with a reason: nothing can invent a
schema id. The registry is also authoritative about what an id *is* — asking
for Protobuf and getting Avro is answered with Avro and an `overrideRefused`
note, not with a failure.

**Decode-then-filter is one operation.** `PayloadDecoder::accept` is the only
way a row is built, and the filter lives inside the decoder rather than beside
it, so there is no arrangement of calls that renders a row the filter rejected
and none that filters on anything but the decoded value. What can still run
earlier is everything that needs no payload — partitions and the window are in
the scan spec, the offset floor is checked in the read loop — and
`a_record_below_the_floor_is_never_decoded_and_never_matched` drives `forward()`
over a synthetic stream to hold that.

**The JS predicate is gone, and the filter box took its job.** Phase 6 shipped
a second filtering tier: a user JavaScript expression over the decoded value,
compiled per request into an `rquickjs` runtime with a 16 MiB cap, a 256 KiB
stack, a 10 ms per-record budget and an interrupt handler installed before the
user's source was ever parsed. It worked — measured live, `while (true) {}`
over a ten-record window was killed four times in **44 ms** with the request
returning normally and RSS at 4.4 MB — and it is still the wrong shape for a
search box. It was the largest attacker-reachable machine in the process,
maintained so that people could type `v => v.amount > 100` into a toolbar, and
what they mostly typed was a word they were looking for.

So `?filter=` moved to where the predicate stood. It is a **literal substring
of the decoded value**: no expression, no pattern, no interpreter, one call to
`str::contains`, and every character in the needle means only itself.
`a_needle_is_matched_literally_and_never_evaluated` asserts that over eight
shapes of injection — regex, SQL, JNDI, template, script tag, header
splitting — and the live run asserts the other half: `?filter=sequence` finds
rows on the Avro canary whose *bytes* contain no such string, because Avro
carries field names in the schema and not in the record.

**Matching the decoded value costs the two-tier design.** kaas-lib's
`RecordFilter` matches bytes, so a needle for the decoded value cannot go into
the scan spec at all, and every record in the window is now decoded before it
can be rejected. Two things follow, and both are in the wire contract rather
than in a comment: `limit` is a budget for records **read**, and `hasMore`
reports whether the *read* filled it. `nextOffset` comes off the window that
was examined rather than off the surviving rows — anchoring on the rows would
return `None` for a window that matched nothing, and paging would stop dead on
the first five hundred records a selective filter emptied. The exception is a
backward page cut to its limit, where the last row *shown* is the boundary:
`tail` over-fetches with `div_ceil`, and stepping past what it read would skip
records nobody saw.

The needle is capped at 256 characters — `MAX_FILTER_CHARS` — and a longer one
is a `400`. It is a comparison the server repeats per record at the caller's
choosing, which makes its length a cost the caller sets and someone else pays.
The frontend trims to the same ceiling by code point rather than by
`String.length`, because half a surrogate pair is not a substring of anything.

**No Monaco.** The phase plan named it for the schema text and the diff.
`@monaco-editor/react` is around 2.5 MB gzipped for a viewer that never edits,
against a frontend bundle that is 250 kB gzipped in total. The schema browser
pretty-prints JSON —
which Avro and JSON Schema both are — shows `.proto` as registered, and takes
a line diff between two versions with a 40-line longest-common-subsequence
implementation. Both sides are normalised before diffing, so a version that
differs only in the registry's whitespace shows as unchanged. Syntax
highlighting is the one thing genuinely not delivered.

**All three subject naming strategies name a topic where one is in the name,
and the schema is what says where.** The page used to strip `-value` and give
up, so `TopicNameStrategy` linked to its topic and the other two read as
"not derivable" — but only `RecordNameStrategy` genuinely has no topic in it.
`orders-com.acme.Order` is a topic and a record joined by the same `-` that
`orders-value` uses, and the seam is invisible in the string; it is not
invisible in the schema, which declares `com.acme.Order`. `kaas-ui-serde`'s
`naming` module reads that declared name out of the three formats — Avro
`namespace` + `name`, `package` + the first top-level `message`, JSON Schema
`title` — and `SubjectNaming` takes it off the end of the subject, exactly,
with nothing guessed. `SubjectDetail.naming` carries the strategy, the topic
and the name, so the frontend classifies nothing.

Two consequences worth stating. A subject with no declared name (a top-level
Avro union, an unreachable registry) falls back to the suffixes alone rather
than splitting at a guessed `-` — a link to a topic that does not exist is
worse than no link. And `RecordNameStrategy` now says *why* there is no topic
instead of showing the same "not derivable" as a subject nobody can parse: the
mapping from record to topics lives in the records, so the topic column is
absent and one line explains it. Decoding was never affected by any of this —
it resolves by schema id and never reads the subject.

**The same reading answers the question from the topic's end**, which is why
`naming` is on `SubjectRow` and not only on `SubjectDetail`. A topic page that
wants its schema has to find the subjects naming *this* topic, and a prefix
match cannot: `orders-` claims `orders-eu-value`, and under
`TopicRecordNameStrategy` the seam is in the schema rather than in the string.
So the topic overview searches the registry for subjects mentioning the topic —
a substring, deliberately wide — and keeps the rows whose `naming.topic` is
equal to it. The column costs no registry call: `describe` already holds the
newest schema for the id and the format, and the declared name comes with it.
The card is absent when nothing matches, which is most topics in most
deployments; both sides appear when both exist, because a key schema and a
value schema are two subjects and picking one would be picking whichever
sorted first.

**And from the list's end it is a third request, not a third column of the
first.** The topic table asks `?schemas=true` the way it asks `?metrics=true`,
against a different dependency and for the same reason: the registry is the one
thing on that page which is not the cluster, and a registry that has gone away
should cost one column rather than the table. The join is one `subjects()` — the
cached listing the fleet tile and the schema browser already read — reduced
against the fifty names on screen, so the cost is a round trip per page and
never a call per row.

Read from the subject *names* only, which is the one place the list and the
detail differ. `{topic}-{record}` needs its schema to find its seam, and
fetching one per subject to fill a column would scale with the registry to
answer a question about the page; the topic page searches for its own name and
describes the handful that come back, so that strategy is answered there and
`TopicNameStrategy` is answered in both. The same `SubjectNaming::of(name,
None)` the `topics` and `dangling` counts are built from, so the two pages
cannot disagree about which topic a subject belongs to. A row carries the
registry's own glyph, one per side and each linking to its subject — fifty rows
of the words `value` and `key` is a column of text to read where the question it
answers is one a mark answers at a glance. Nothing pops up on hover to say which
side it is; that is in the subject it links to, and the glyph carries an
`aria-label` rather than a `title`, which names it for a screen reader without
putting a tooltip on a table someone is scanning. The column is absent on a cluster
referencing no registry, `—` where the registry holds nothing for that topic,
and `·` while the request is out — `TopicSummary.schemas` is `None` for
"not answered" and `Some` with two empty sides for "nothing registered", which
is the distinction the metric columns already draw. It is not sortable: ordering
by it would mean reading the registry for every topic on the cluster before the
first row could be placed.

**The schema browser is guarded like the topic list**, `Resource::Topic` +
`Action::View`, and it does **not** require a connected cluster. A registry
serves an environment and knows nothing about brokers, so its subjects stay
browsable while the cluster you arrived through is down — which is why the
sidebar's schemas item is the one entry that survives an unreachable cluster.
A new `Resource::Schema` was considered and rejected: every role granting `all`
today would silently stop covering the browser.

## Phase 7 — the decisions

**The plan's table of what each cluster answers was measured a second time,
and three of its five rows had changed.** It said `kaas` had 24 ACLs and no
SCRAM, and that Strimzi had no authorizer. Today `kaas` answers 31 bindings and
*does* answer `DescribeUserScramCredentials`; Strimzi answers `DescribeAcls`
rather than refusing it; and neither cluster has a single client quota
configured, where the plan expected `throttled-user`'s limits to be the fixture.
`throttled-user` exists — as a SCRAM credential, which is what the plan had
seen. The live assertions therefore count nothing: they assert the *shape* of a
binding and that the count is non-zero, because a test that fails when somebody
grants a principal a topic is a test that gets deleted.

What did not change is the pair the phase existed for: `ListTransactions`,
`DescribeTransactions`, `DescribeProducers` and `ListPartitionReassignments`
are on Strimzi and absent from `kaas`. Both halves are asserted — the answer on
one cluster and the `UnsupportedApi` naming both version ranges on the other —
so the tab set really is a conformance report.

**One page with five tabs, not five items in the sidebar.** They share a shape
— a cluster-wide administrative fact, in a table, that most clusters answer for
and some do not — and five more rows under every cluster would crowd out the
four things people open daily. The nav item appears when *any* of the five is
available, which is why `anyFeature` exists beside `feature`.

**`CapabilityTab` was named by the design system in Phase 1 and written here.**
Until now the code gated tabs with a condition around `TabsTrigger`, which is
fine once and is five copies of the same three-state decision at five. The
third state is the one that makes it a component: a tab that is not rendered
still has a URL, and `?screen=transactions` in a link somebody sent must land
on the panel naming both version ranges rather than on an empty table.

The gate beside it is a **function** and not a component, which is worth saying
because writing it the obvious way is a silent bug: `const gate = <Gate …/>; if
(gate)` is always true — a component returning `null` still produces an element
object. Called, it returns the `null` a caller can branch on. Found by reading
it back, not by a type error, which is what that shape costs.

**The transaction inspector sends a start timestamp and never a duration.**
`open_for_ms(now)` takes a `now`, and whichever `now` the server passes is
wrong by the time the response is read and wronger every second the page stays
open — on the one column the screen is sorted by. The browser ticks it from the
timestamp, `SnapshotAge`'s decision applied to the number an operator is
watching to decide whether to intervene. A live assertion checks the *absence*
of a computed duration in the response, because the tempting version of this
code is the one that adds a field.

**Quotas are one call per entity type, and the same entity comes back more than
once.** An empty component list asks about no entity type and the broker
answers with nothing, so `user`, `client-id` and `ip` are asked separately —
and `user=alice, client-id=app` arrives from two of the three. Deduplicated on
the rendered entity, which is the identity the reader sees. Partial failure is
a result here too: a cluster that answers for users and not for IPs renders the
users and names the failure.

**`Resource::ClusterConfig` for all five, and no new variant.** The temptation
on an ACL screen is a `Resource::Acl`, and Phase 6 had already worked the
argument through for `Resource::Schema`: `Resource::every()` is what a role
saying `all` expands to, so a new variant silently narrows every deployed role
that has one. If a deployment ever needs "can see brokers, must not see who can
authenticate", that is a real requirement and a breaking change to the policy
file — a decision of its own rather than a side effect of this phase.

**An ACL operation this build cannot name renders as `unknown(99)`.**
`AclOperation::Unknown(i8)` is the same case as an unknown api key: expected
output, not a gap. Naming it would be a Kafka version table in kaas-ui, which
is rule 2 with extra steps.

**A live assertion that depends on a fixture's *size* stops testing without
failing.** The analysis governor's "a second analysis is refused with 429" ran
against `kperf-bench` on the premise, written in its own comment, that 146M
records "will not finish under this test". The topic holds nine thousand now, a
whole analysis takes 25ms, and the assertion slept 300ms before asking — so it
had been asserting nothing until the day it started failing. Racing the two
requests concurrently narrowed the window rather than closing it. The governor
is a set of `(environment, cluster)` keys in one process and needs no broker at
all, so it moved to a unit test where the answer is deterministic, and `live`
says why in the space it left. That is CLAUDE.md's cluster-free/live split
being applied rather than quoted.

## What is still unproven

Named rather than left implied, because each of these is a thing a reader would
otherwise assume was covered:

- **The frontend has never been verified in a browser under load.** The render
  budget in the Phase 3 spec — ten thousand records a second, React commits at
  roughly seven a second, none over 16 ms — needs the React Profiler and a real
  load generator. The design is built for it (`getSnapshot` returns a stable
  reference, the transport never touches React state, row height is fixed) and
  none of that is *measured*.
- **The malformed-batch path is unproven end to end.** Neither live cluster will
  corrupt a batch on request, and kaas-lib already covers the decoder path. The
  row type, its rendering and the raw-hex detail exist and are unit-tested;
  what is untested is real damage arriving through this layer.
- **The tail byte budget is asserted in kaas-lib's integration suite**, against
  a container, not here. kaas-ui has no Docker, and re-measuring it would test
  the library rather than this layer.
- **The consumer, share and streams group kinds have no live fixture.** Neither
  cluster produces one; three of the four kinds are covered by unit tests over
  constructed values. Deploying a Kafka Streams application to the `strimzi`
  namespace remains the cheapest way to get a real `Unrecognized` group, and
  it is worth doing: that variant is the one most likely to be wrong and least
  likely to be exercised by accident.
- **No *automated* test drives a browser through a login**, though the parts a
  browser would catch are now guarded three ways. The flow itself is
  confirmed — a browser completed it against production on 2026-08-04, name in
  the sidebar and both clusters listed.

  The guards, because the gap they close is invisible from Rust: a unit test on
  the cookie builder (`session.rs`), the same four attributes asserted on the
  wire in `cargo xtask login` so a middleware rewriting `Set-Cookie` cannot
  slip past the unit test, and the fourth invariant grep, which requires
  sign-in to be an `<a href>` and logout a `method="post"` form. Each was
  mutation-tested rather than assumed: flipping `Secure`, `HttpOnly` and
  `SameSite`, and turning the anchor into a `fetch`, each fails the guard that
  claims to cover it.

  The wire check reads the cookie *as issued*, not as deleted — the pending
  cookie is always cleared at the callback and a deletion `Set-Cookie` carries
  only what a deletion needs, which is a false failure the first version of it
  produced.

  What remains genuinely uncovered is everything only a browser does: that it
  honours those attributes, GitHub's consent-and-return leg, the authenticated
  render, and the ten-minute clock `Max-Age=600` puts on the human part.
- **Tab sets differing between the two clusters is asserted at the API**, not in
  a browser. The live run shows `kaas` projecting 7 available features against
  Strimzi's 16; that the rendered tab sets differ accordingly has been seen but
  not automated.
- **Protobuf and JSON Schema have no live fixture.** Both decode paths are
  asserted against a stub registry — including a `.proto` with a repeated
  field, an enum, bytes, a nested message and a field the descriptor does not
  know — and the only registry-backed topic in this cluster is the canary's
  Avro one. Getting a real Protobuf topic means a second canary; the Avro half
  is genuinely end to end and the other two are not.
- **Schema references resolve transitively against a stub, not a registry.**
  The Avro reference case uses `schema_registry_converter`'s own fixture and
  asserts the referenced subject was fetched. Apicurio holds no subject with a
  reference to point this at.
- **The schema browser has not been driven in a browser.** The routes, the
  subject list, the version history and the diff are exercised over HTTP; the
  page that renders them is typechecked and built and has not been clicked
  through.
- **Nothing measures what decoding costs a stream.** The protobuf decoder
  rebuilds its `protofish` context per record — the schema list is cached, the
  parsed context is not, because the converter does not expose it — and on a
  fast Protobuf topic that could matter. Avro and JSON parse from a cached
  schema and do not have the problem.

## The fleet became a hierarchy

Post-phase, and it touched every layer: configuration, the registry, every
route, the router and every page.

**A fleet is environments; an environment holds Kafka clusters, schema
registries and inventory.** That was always the mental model — a registry
serves an environment, and the fleet page had sectioned by one since Phase 0 —
but nothing in the shape said so. Membership was a *label*: `env: dev` on a
cluster, `environment: dev` on a resource, and a `schema_registries:` block at
the top level that clusters referenced by a global id. Three places to write
the same word, and each one a place to typo it.

Nesting is the membership now, and several rules retired rather than moving:

- **`ResourceEntry.environment` is gone**, and with it the validation that a
  resource may not name an environment nobody declared. There is nowhere to
  write that typo — the field does not exist. A rule you can delete is better
  than a rule you enforce.
- **A cluster's `env` label is rejected, not merged.** It is derived from the
  block, so it cannot disagree with it. That mattered: it is the one input that
  could have put a cluster in `prod` outside a `cluster_labels: {env: prod}`
  selector, silently.
- **Discovered and unnamed sections are gone.** Every environment is declared,
  because there is no top level to declare a cluster at.
- **Ids are scoped.** `kafka` in `dev` and `kafka` in `prod` are two clusters.
  The registry is keyed `(environment, id)` and so is every lookup.

**The schema registry got its own URL, and the reason it did not have one
dissolved.** `/api/clusters/{id}/schemas` existed because a registry id as a
top-level namespace would have been enumerable, and "which clusters use this
registry" can name a cluster the caller may not see. Scoping the id to an
environment settles both: `/api/environments/{env}/schema-registries/{id}`
answers only to a caller who can already see a cluster there that references
it. The URL names what it returns, and it still cannot be probed. The nav's
`viaCluster` hack — a registry row linking through an arbitrary cluster — went
with it.

**The config break is hard, and it says so.** `deny_unknown_fields` would have
called a top-level `clusters:` a misspelling; a pre-nesting config is now
refused with the destination of each block named. `config.dev.yaml`,
`config.live-auth.yaml` and the deployed ConfigMap in `k3s-cluster` were
converted together.

**One route kept the old scope under the new URL, and nothing caught it for
two phases.** `GET /api/environments/{env}/clusters` took the `{env}` segment
and ignored it: the handler returned every visible cluster in the fleet, so
`dev/clusters` listed `prod-eu`. Everything above it was correct — the fleet
arranges by environment, `/environments/{env}` filters, both lookups are keyed
`(environment, id)` — which is exactly why it survived. The frontend's own
comment on that call already said "environment-scoped because a cluster id
is", so the client believed the contract the server was not keeping, and every
`find` by id alone in the UI was one same-named cluster away from picking the
wrong one.

It was found by porting `xtask live` (below), not by a unit test, and the
lesson is the ordinary one: a scope bug looks like correct data until two
environments hold the same id, and the dev fleet's `staging`/`prod` sections
were the fixture that could have shown it all along. There is now an assertion
that a listing does not reach past the environment that names it.

**`xtask live` was written against `/api/clusters/…` and had stopped running.**
Not degraded — *stopped*: the third assertion is the fleet, it 404s, and the
run aborts there, so every phase's acceptance command had been reporting
nothing since the nesting landed. The paths are now built from one `ENV`
constant, and the two checks that could not be ported as prefixes were
rewritten as the claims they had become: the schema browser is one call to a
registry rather than the same call to two clusters, and "a cluster with no
registry" is a null on its card rather than an empty list from a route that no
longer exists. 61 assertions, green.

### Measured

- 218 unit tests pass; `cargo xtask ci` green.
- Every route the frontend calls answers `200` against the live fleet, and
  Avro on `kaas-canary-v1` still decodes through the environment-scoped
  registry handle — `registry: "apicurio"` on the payload.
- Cross-environment probes are `404`: `prod/clusters/kaas`,
  `dev/clusters/prod-eu`, `prod/schema-registries/apicurio`. The last one
  matters most — `prod` declares no registry of that id, and the reply is
  indistinguishable from "not yours".

## The statistics tab (issue #1, after Phase 6)

An on-demand full-topic analysis in the shape of kafbat's, landed as a
follow-on to Phase 3 rather than a phase of its own — the issue's design held
almost unchanged, and the streaming infrastructure carried its second consumer
without modification. What was decided at the point of building:

- **No sketch crates.** HyperLogLog (2^12 registers, ±~1.6%) and a
  log-bucketed size histogram (8 buckets per octave, ±~4% on percentiles) are
  ~150 lines in `kaas-ui-core/src/analysis.rs`, panic-free under the workspace
  lints without an `allow` — which was the selection criterion the issue set
  for choosing a crate, met by not needing one.
- **A partial result is flagged, never dressed as complete.** The lifetime
  ceiling (30 minutes), a mid-scan error, and shutdown all emit a `result`
  with `complete: false`, the scanned fraction, and the error named. Until
  upstream ask 13 lands, one partition's failure costs the *rest* of the scan;
  the fold up to that point still leaves, labelled.
- **One analysis per cluster**, enforced by a permit in `AppState` beside the
  stream governor's budget, with the refusal naming why: a full-topic read
  occupies the shared per-broker connection (ask 11), so the ceiling is about
  everyone else's latency rather than memory.
- **The result lives in the browser's query cache.** The stream-not-in-cache
  rule holds — a *result* is a terminal value, so revisiting the tab is
  instant with no server-side store. Two people analysing one topic scan it
  twice; sharing would cost an in-memory store and per-replica amnesia.
- **The hour map is capped** at 10,000 buckets, so a producer writing garbage
  timestamps costs bounded memory; the result carries `hourlyTruncated`
  rather than a silently narrowed chart. Records with no timestamp are
  counted, not plotted as 1970.
- Cancellation is closing the stream, exactly as designed: the pump selects on
  the reader going away, Radix unmounting the hidden tab panel closes the
  `EventSource`, and `no_mutating_route` needed no exception.

Measured against both clusters (`cargo xtask live`, 72 assertions): totals
agree with offset spans exactly on both; the hourly histogram populates on
`kaas` despite its missing timestamp index — the histogram reads timestamps
off records rather than seeking by time; fractions are monotonic and capped;
a second analysis on a busy cluster answers 429 and the slot frees when the
response drops.

## Upstream asks these phases raised

Filed in [reference/upstream-asks.md](reference/upstream-asks.md), open against
kaas-lib:

| | | why it matters here |
|---|---|---|
| 1 | cluster-level capability aggregation | retires the interim `source` rule above |
| 2 | batched `FindCoordinator` (KIP-699) | a 300-group page is 300 round trips today |
| 3 | multi-group `OffsetFetch` | pairs with 2; together O(n) → roughly O(1) |
| 11 | streaming reads need their own connection | a live view degrades the whole cluster's UI |
