# Phase 4 — authentication and authorization

*PLAN.md milestone M4.*

**Goal.** OIDC against Dex, sessions, role mapping, cluster visibility, the
`metadata` versus `messages` grant, and the access audit log.

Creates `crates/kaas-ui-auth`.

> **Slice 1 is built: authorization, without authentication.** `kaas-ui-auth`
> holds `Principal`, `Role`, `Policy` and `Access`; `roles:` is a config block;
> `Registry::get` takes the caller and answers `404` for a cluster no role
> selects; the `messages` grant gates all four payload routes and rides on each
> cluster's card so the tab disappears with it; `/api/me` and `/health` say who
> the caller is and whether roles are being enforced at all.
>
> **Every caller is still anonymous**, because nothing signs anyone in yet. Two
> consequences worth stating plainly: with no `roles:` configured the
> deployment is exactly as open as it was before — one anonymous caller,
> everything visible, which is why 0.3.0's behaviour did not change — and with
> `roles:` configured the fleet is empty for *everyone*, because no role covers
> an anonymous caller. That is the safe direction for the gap to fail, and the
> server warns about it at startup rather than leaving it to be found.
>
> **Authentication stays optional, and that is a requirement rather than a
> transitional state.** kaas-ui is developed behind code-server, where there is
> no identity provider and no reason to run one: with no `dex` block and no
> `roles`, `/dex` is not a route, nothing redirects, and every request is the
> anonymous caller who sees everything. Two tests in `kaas-ui-server` hold the
> line — the same path is the frontend without a `dex` block and the proxy with
> one — because "it happens to work today" is how an optional dependency stops
> being optional.
>
> Next: the OIDC exchange and sessions, then the access audit, then Dex in the
> cluster repo.

## Prerequisite: there is no Dex in this cluster

Checked. The only Dex running is `argocd-dex-server`, which is ArgoCD's embedded
instance and not a general-purpose IdP. `spire-spiffe-oidc-discovery-provider`
is a SPIFFE discovery endpoint, not a login provider.

**A Dex deployment is the first task of this phase**, in the cluster repo
alongside kaas-ui — `apps/dex/`, discovered by the same ApplicationSet. In CI a
static-password connector is enough; in the cluster, a GitHub connector against
an OAuth App of its own.

**It is served under kaas-ui's own hostname, at `/dex`.** Every browser hop of
an OIDC login has to reach the provider — the redirect to `/dex/auth`, and the
one GitHub sends back to `/dex/callback` — so Dex must be publicly reachable.
Rather than give it a second hostname and a second DNS record, kaas-ui proxies
`/dex/*` to the in-cluster Service, which is exactly what `argocd-server` does
for its own Dex at `/api/dex`. Dex is configured with
`issuer: https://kaas.smeding.cloud/dex` and serves every endpoint under that
path; nothing is stripped in the proxy.

That proxy forwards whatever method the browser sends, which retired the
"every route is a `GET`" check — see [00-foundations.md](00-foundations.md).
The read-only guarantee never depended on it: it is the single
`Admin::connect_read_only` construction site, and nothing reachable through the
proxy has an admin client at all.

This is a real dependency and it is worth stating before the phase starts rather
than discovering it three days in.

## Dex is the only provider

kaas-ui speaks generic OIDC and nothing else. Dex terminates GitHub, Google,
Entra, LDAP or SAML and presents all of them as one issuer with a `groups`
claim.

The payoff is that **kaas-ui contains no provider-specific code at all** — and
the counter-example is running in this cluster right now. `kafbat-ui` is
configured with a GitHub OAuth2 client, `user-name-attribute: login`, and
`custom-params: {type: github}`, because GitHub is *not* an OIDC provider:
OAuth Apps issue opaque tokens with no `id_token`, no discovery document and no
groups claim. Supporting it directly means a second code path with its own REST
calls to `/user` and `/user/orgs`.

Dex's GitHub connector does that work and emits `org` and `org:team` group
strings — exactly the shape the role mapping below wants. Adding a second
identity source later becomes a Dex config change rather than a kaas-ui release.

Say this in the README. "We only support OIDC" reads as a limitation until you
add that Dex makes it a superset.

```yaml
auth:
  issuer: https://dex.smeding.cloud
  client_id: kaas-ui
  client_secret_file: /etc/kaas-ui/secrets/oidc-client-secret
  redirect_url: https://kaas.smeding.cloud/auth/callback
  scopes: [openid, profile, email, groups]
```

## Implementation

`openidconnect` 4.0 against the discovery document. **PKCE, `state` and `nonce`
are mandatory**, and the `id_token` signature is fully verified — not decoded.

Sessions via `tower-sessions` 0.15 with an encrypted cookie store,
`SameSite=Lax`, `Secure`, `HttpOnly`. A server-side store only if forced logout
is needed; the cookie store is one less thing to run. RP-initiated logout, which
Dex supports.

The client secret comes from Vault through an `ExternalSecret`, the same pattern
`kafbat-ui-github-oauth` uses today.

## Authorization

Read-only makes this small. With no writes, permissions collapse to two axes,
and the second is the one that matters.

```yaml
roles:
  - name: everyone
    subjects: ["kaas-rs"]                  # Dex GitHub connector: org membership
    clusters: { env: dev }                 # label selector
    grants: [metadata, messages]

  - name: prod-oncall
    subjects: ["kaas-rs:platform"]         # org:team
    clusters: { env: prod }
    grants: [metadata]                     # metadata only — no payloads

  - name: prod-support
    subjects: ["kaas-rs:support"]
    clusters: { env: prod }
    grants: [metadata, messages]
    topics: ["public-*"]                   # payload access scoped by pattern
```

**`metadata` versus `messages` is the meaningful boundary.** Browsing topic
configuration is not the same act as reading customer data out of a payload.
Topic contents are the sensitive surface in a read-only tool — payloads carry
PII, tokens, order data — so "can browse metadata" and "can read message bodies"
are different grants.

**Hand-roll this.** A matrix over two actions does not need `casbin-rs`, and the
policy file above is more legible than a Casbin model. Resist the pull; this is
the part of an auth system that looks like it wants a framework and does not.

**Cluster visibility is enforced in the registry lookup, not in the router.** A
user without access gets `404`, not `403`, so cluster ids are not enumerable.
The single `Registry::get(id, who)` from Phase 0 gains its second parameter
here, and because there is only one lookup site there is only one place to get
it right.

## Access audit

Append-only log of `(timestamp, subject, cluster, topic, action, offsets)` for
**every message read**. SQLite via `sqlx`, or structured JSON on stdout for
shipping elsewhere — configurable, defaulting to stdout in the cluster where
the observability stack already collects it.

This is the log that matters in a read-only tool, and it is the one most likely
to be skipped precisely because nothing is being changed. With no writes, audit
is about *reads*: who opened which topic's messages, on which cluster, when.

It is written **before the response is sent**, not after, and a failure to write
it fails the request. An audit log that is best-effort is not an audit log.

## Traps

- **`Error::Authentication` from a cluster is a 502, never a 401.** A cluster
  whose SASL credentials were rejected must not log the user out. This is in the
  error table and it is worth checking twice here.
- **The groups claim can be large.** An encrypted cookie has a 4 KB budget.
  Store the resolved role names, not the raw claim.
- **Token refresh.** Sessions outlive the `id_token`. Either refresh against Dex
  or accept that the session length is the session length — decide explicitly
  and write it down; drifting into "it logs people out at odd times" is the
  default outcome.
- **The `messages` grant gates the endpoint AND the tab.** A `metadata`-only
  user must not see a message tab that 403s on click. Same mechanism as the
  capability projection — grants are projected into the same shape the frontend
  already consumes.

## Acceptance

```sh
cargo xtask live --config config.dev.yaml
```

- login works end to end against a Dex instance with a **static-password
  connector**, run in CI as a container — the one place in this project where a
  container fixture is the right answer, because the alternative is depending on
  GitHub from a test;
- a user in no matching role sees an **empty fleet**, not an error and not a
  login loop;
- a `metadata`-only user gets **403 on the messages endpoint and no message
  tab**;
- a user with no access to `strimzi` gets **404** on
  `/api/clusters/strimzi/topics`, and that cluster is absent from
  `/api/clusters`;
- **every message read appears in the audit log**, with offsets — asserted by
  reading the log back after a tail;
- a forced audit-write failure fails the request rather than serving the
  payload;
- PKCE, `state` and `nonce` are all present in the authorize URL and all
  verified on callback (unit test against a recorded flow).

## Exit criteria

- [ ] Dex deployed via GitOps, kaas-ui pointed at it
- [~] **no provider-specific code anywhere in the workspace** — true of what
  exists, and `kaas-ui-auth` has no dependency that could make it false; it
  cannot be *met* until there is a provider to have been blind to
- [ ] `id_token` signature verified, not decoded
- [x] **one registry lookup, taking the caller, returning 404 for invisible
  clusters** — `Registry::get(id, &Access)`, with the visibility test inside it
  rather than in the router, and asserted both in unit tests and by running a
  server with an enforcing policy: a configured cluster answers `404`, and the
  fleet is empty rather than an error
- [~] **`metadata` / `messages` gates both endpoint and tab** — both are wired:
  all four payload routes spend the grant against the topic name, and the tab
  is hidden when the cluster's card does not carry it. Unit-tested, and not yet
  provable end to end, because proving it needs a caller who holds one grant
  and not the other — which needs sessions
- [ ] audit log written before the response, failure is fatal to the request
- [ ] client secret from Vault via ExternalSecret, never in a ConfigMap
