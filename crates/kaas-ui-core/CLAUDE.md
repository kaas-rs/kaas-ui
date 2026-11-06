# kaas-ui-core

- **An absent credential is where the credential comes from.** One rule for
  all four — SASL `password`, OAuth `client_secret`, a registry's `password`
  and its `bearer_token`: write the key and it wins, omit it and it is read
  from `KAAS_UI_<CREDENTIAL>_<ENVIRONMENT>_<ID>`, which the deployment fills
  from a `secretKeyRef`. Overriding from the file is right for a local run and
  wrong for a deployment, which is why the deployed ConfigMap sets no secret
  at all. There were three keys per credential once (`x`, `x_file`, `x_env`)
  and the extra two were a decision at the point of writing a config rather
  than a capability. Two things whose ids flatten to one variable — including
  a cluster and a registry sharing an id — are a startup error, not a shared
  credential. The exception proves the rule: `client_assertion_file` names a
  **signed assertion somebody else wrote** — a SPIFFE JWT-SVID — so there is
  no secret to find, the environment is not consulted, a `client_secret`
  beside it is refused, and the file is re-read on every exchange because it
  outlives nothing. **PEM stays a file**: `ca_file`, `cert_file` and `key_file` are
  multi-line and belong in a mount. Everything is read at startup, so rotating
  needs a restart. Secrets redact themselves in `Debug` — the leak that
  happens is `?entry`, not a deliberate print — and a failing exchange reports
  the far end's error, never the credential.
