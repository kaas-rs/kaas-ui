# Phase 4 — authentication and authorization

*PLAN.md milestone M4.*

**Goal.** OIDC against Dex, sessions, role mapping, cluster visibility, the
metadata-versus-payload boundary, and the access audit log.

Creates `crates/kaas-ui-auth`.

> **Mostly built, and running.** Dex is deployed, kaas-ui is pointed at it, and
> people sign in with GitHub at `https://kaas.smeding.cloud`. What is built is
> below under [What shipped](#what-shipped); what is left is three items under
> [What is still open](#what-is-still-open), and the largest of them is that
> **no automated test has ever performed a login**.

## What shipped

**Authorization**, in `kaas_ui_auth::policy`. Roles carry subjects, clusters and
permissions of `resource` + `value` + `actions`, which is kafbat-ui's shape —
anyone arriving from that UI already knows the model. What could not carry over
is most of its vocabulary: `create`, `edit`, `delete`, `messages_produce` and
`reset_offsets` describe writes, and no code path here could perform one. Two
actions exist, `view` and `messages_read`, and an action that does not exist is
**rejected by name** rather than accepted and ignored.

Two additions to kafbat's model. `clusters` takes id patterns and
`cluster_labels` an optional selector beside it, because a fleet grown past a
handful wants `env: prod` as well as names. And **no roles at all means the
anonymous caller is an administrator** — the open deployment every development
instance runs, stated as a rule rather than left as an accident.

**Cluster visibility is enforced in the registry lookup, not the router.**
`Registry::get(id, &Access)` is the single lookup from Phase 0 with its second
parameter; a caller who may not see a cluster gets `404`, not `403`, so ids are
not enumerable by probing. One lookup site, one place to get it right.

**Authentication**, in `kaas_ui_auth::oidc`. `openidconnect` 4.0 against the
discovery document, read once at startup so a broken `issuer` fails the boot
rather than somebody's first login. PKCE with `S256`, `state` and `nonce` are
mandatory and generated with the request, so no path through the module omits
one. The `id_token`'s signature is verified against the provider's JWKS — the
whole reason Dex is in front of GitHub, which signs nothing.

**kaas-ui is a public client with no client secret.** PKCE is what proves that
whoever redeems the code started the flow. This replaces the phase's original
`client_secret_file` sketch and removes the ExternalSecret it implied: there is
no kaas-ui secret in Vault because there is no kaas-ui secret. Dex still has
one, for its GitHub connector.

**Sessions are encrypted cookies, with no store.** A subject, a name, the
resolved role names and an expiry — small enough to carry. Encrypted rather than
merely signed, because the pending-login cookie holds a PKCE verifier and a
verifier anyone can read is not a verifier. The key is generated at startup, so
**a restart ends every session**; for a single-replica read-only browser tool
that is a better trade than another secret to mount and rotate, and the startup
log says so rather than leaving it to be discovered as "it logs me out
sometimes".

**No refresh tokens.** `offline_access` is never requested; a session lasts its
eight hours and then asks for a login again. This is the phase's "decide
explicitly and write it down" trap, decided: a refresh token is a long-lived
credential to store, protect and revoke, in exchange for saving a user one
redirect a day on a tool they keep open for twenty minutes.

**Dex is served under kaas-ui's own hostname at `/dex`**, proxied to the
in-cluster Service — the arrangement ArgoCD uses for its own Dex at `/api/dex`.
Every browser hop of an OIDC login has to reach the provider, and this costs no
second DNS record and no second public surface. Nothing is stripped in the
proxy: Dex serves every endpoint under its issuer's path, and rewriting would
break the discovery document, which advertises absolute URLs built from that
same issuer.

That proxy forwards whatever method the browser sends, which retired the "every
route is a `GET`" check — see [00-foundations.md](00-foundations.md). The
read-only guarantee never depended on it: it is the single
`Admin::connect_read_only` construction site, and nothing reachable through the
proxy has an admin client at all.

**Authentication stays optional, and that is a requirement rather than a
transitional state.** kaas-ui is developed behind code-server, where there is no
identity provider and no reason to run one: with no `dex` block and no `roles`,
`/dex` is not a route, nothing redirects, and every request is the anonymous
caller who sees everything. Two tests in `kaas-ui-server` hold the line, because
"it happens to work today" is how an optional dependency stops being optional.

**The access audit**, in `kaas_ui_auth::audit`. One JSON line per disclosure on
stdout, carrying who, which cluster and topic, the seek, and the offsets
actually returned. Written **before** the payload is disclosed, and a failed
write **fails the request** — an audit log that is best-effort is a log. Metadata
is not audited: listing topics is not reading a payload, and the boundary is the
same one the `messages_read` action draws. SQLite via `sqlx` is **not** built; a
database is a second thing to run, back up and migrate for a log nobody has yet
asked to query, and the writer is injectable so adding one later changes that
module and nothing else.

## The thing this phase got wrong, and how it was found

**A provider rotates its signing keys, and discovery happens once.**

Dex mints a new signing key on a schedule and serves the previous one beside it
for a while. `Provider::discover` read the JWKS at startup and held it for the
life of the process, so from the first rotation onward **every login failed**
with `Signature verification failed` — and stayed failed, because nothing
re-read the keys. A restart fixed it until the next rotation.

It was found in production, by a person who could not sign in, and the shape of
it is worth keeping:

- the failure is **not at the boundary you would suspect**. State, nonce and
  PKCE all verified; the exchange succeeded; the token was genuine. The only
  thing wrong was that this process had never seen the key that signed it;
- it is **invisible for hours after a deploy**. A fresh pod has fresh keys, so
  every test anyone runs by hand right after shipping passes;
- it **does not degrade, it stops**. There is no partial mode, and no log line
  until a user reports it.

The fix is `Provider::refresh_keys`: on a signature failure — and only on a
signature failure, which is the one verification error plausibly *our* fault
rather than the caller's — re-read the key set and give the token exactly one
more chance. Only the verification is retried, never the exchange: the code is
single-use and spending it twice answers `invalid_grant`, turning a recoverable
login into a failed one.

Only the key set is re-fetched, not the whole discovery document. An endpoint
that moved is a reconfiguration, not something to follow silently at login time.

The unit test that guards it asserts the mechanism rather than the symptom —
that the client reads the metadata cell *now* rather than a snapshot beside it —
because collapsing that cell back into a plain field compiles, passes every
other test in the module, and breaks logins some hours after each deploy.

## Traps

- **`Error::Authentication` from a cluster is a 502, never a 401.** A cluster
  whose SASL credentials were rejected must not log the *user* out.
- **The groups claim can be large.** An encrypted cookie has a 4 KB budget.
  Store the resolved role names, not the raw claim. Done.
- **`messages_read` gates the endpoint AND the tab.** A view-only user must not
  see a message tab that 403s on click. Grants ride on each cluster's card, so
  the tab disappears with the permission.
- **The keys expire, the endpoints do not.** See above. Anything cached from a
  provider at startup needs an answer to "what happens when this changes".

## Acceptance

```sh
cargo xtask ci      # green
cargo xtask live --config config.dev.yaml
```

Met, and asserted:

- a cluster no role selects is **`404`**, and the fleet is empty rather than an
  error — unit-tested and asserted in `live`;
- PKCE, `state` and `nonce` are all present in the authorize URL, and two logins
  never share a challenge — unit tests against a recorded discovery document;
- **every message read appears in the audit log** with its offsets, a topic
  *list* produces none, and with stdout pointed at `/dev/full` a tail answers
  `500` with no payload while `/health` and the metadata routes are untouched —
  proven by running;
- the login flow verifies against the provider's **current** keys.

Not met, and the reason each is still open is in the next section:

- login end to end against a Dex with a static-password connector, in CI;
- a view-only user gets 403 on the messages endpoint and no message tab, proven
  with a real session rather than a constructed `Access`.

## What is still open

- [ ] **An automated test has never performed a login.** Everything from the
  redirect to the session cookie is covered by unit tests over recorded
  documents and by people using it; nothing exercises the whole flow
  unattended. The phase's answer — a Dex with a static-password connector, run
  in CI as a container — is the one place in this project where a container
  fixture is the right call, because the alternative is depending on GitHub
  from a test. It is also why the key-rotation bug reached a user.
- [ ] **RP-initiated logout.** `POST /auth/logout` drops kaas-ui's cookie and
  nothing else, so signing out leaves the Dex session intact and the next login
  is silent. Dex supports the endpoint; kaas-ui does not call it.
- [ ] **The grant boundary is unproven end to end.** Both halves are wired and
  unit-tested — the four payload routes spend `messages_read` against the topic
  name, and the tab is hidden when the cluster's card does not carry it — but
  proving it needs a caller who holds one permission and not the other, which
  needs the login test above.

Two exit criteria from the original plan are **retired rather than unmet**:

- ~~client secret from Vault via ExternalSecret~~ — kaas-ui is a public client
  and has no secret. See above.
- ~~no provider-specific code anywhere in the workspace~~ — true, and
  `kaas-ui-auth` has no dependency that could make it false. It was never
  falsifiable: Dex terminates GitHub, Google, Entra, LDAP or SAML and presents
  all of them as one issuer with a `groups` claim, which is why "we only
  support OIDC" is a superset rather than a limitation. The counter-example is
  next door — `kafbat-ui` carries a GitHub-specific code path with its own REST
  calls to `/user` and `/user/orgs`, because GitHub OAuth Apps issue opaque
  tokens with no `id_token`, no discovery document and no groups claim.
