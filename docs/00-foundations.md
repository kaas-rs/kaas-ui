# Foundations

Everything every phase assumes. Decided once here so no phase has to re-open it.

## Toolchain

`rust-toolchain.toml` pins **1.97.1**. kaas-lib's published crates declare
`rust-version = "1.97"`; the workspace default here is older, so the pin is what
makes `cargo build` work rather than fail on a resolver message about a rust
version. The 1.97.1 toolchain is already installed.

```toml
[toolchain]
channel    = "1.97.1"
components = ["rustfmt", "clippy", "rust-src", "rust-analyzer"]
profile    = "minimal"
```

Edition 2024, matching kaas-lib.

## How kaas-lib is consumed

**Depend on crates.io.** `kafka-admin`, `kafka-read`, `kafka-meta` and
`kafka-conn` are published at `0.1.0` and verified to work against both target
clusters from this workspace. A path dependency on a sibling checkout would make
the repository unbuildable for anyone who does not have one.

```toml
[workspace.dependencies]
kafka-admin = "0.1"
kafka-read  = "0.1"
kafka-meta  = "0.1"
kafka-conn  = "0.1"
```

The four release in lockstep and must be pulled at the same version.

**For rapid iteration on the library**, `cargo xtask link` writes a fenced block
into the root `Cargo.toml`:

```toml
# --- BEGIN kaas-lib local override (cargo xtask link) ---
[patch.crates-io]
kafka-conn  = { path = "../kaas-lib/crates/kafka-conn" }
kafka-meta  = { path = "../kaas-lib/crates/kafka-meta" }
kafka-admin = { path = "../kaas-lib/crates/kafka-admin" }
kafka-read  = { path = "../kaas-lib/crates/kafka-read" }
# --- END kaas-lib local override ---
```

`cargo xtask unlink` removes it. `cargo xtask ci` **fails if the block is
present**, so a linked tree cannot be committed or released by accident. This is
chosen over a `.cargo/config.toml` `paths` override because the override is then
visible in the diff and in `Cargo.lock`, rather than being invisible local
state that makes two developers' builds silently differ.

## Workspace layout

```
kaas-ui/
  Cargo.toml            workspace, lints, dependency table
  rust-toolchain.toml
  rustfmt.toml          copied from kaas-lib
  .cargo/config.toml    rustflags = ["-D", "warnings"]
  crates/
    kaas-ui-core/       config, cluster registry, domain DTOs, capability projection
    kaas-ui-api/        axum routers, request/response DTOs, utoipa
    kaas-ui-server/     the binary: wiring, embedded frontend
    kaas-ui-serde/      payload decoding                      — created in Phase 3
    kaas-ui-auth/       OIDC, sessions, RBAC, access audit    — created in Phase 4
  web/                  vite + react
  docs/
  xtask/
```

PLAN.md §3 lists all five crates. Only three exist after Phase 0 — kaas-lib's
rule 3 forbids stubs, and an empty crate that compiles is a stub with a
manifest. `kaas-ui-serde` and `kaas-ui-auth` are created by the phase that
fills them.

Strictly layered, no cycles: `core` knows about kaas-lib and nothing about HTTP;
`api` knows about `core` and axum and never opens a socket; `server` wires them.

## Dependency pins

Verified present on crates.io at the time of writing.

### Backend, Phase 0

| crate | version | why |
|---|---|---|
| `axum` | 0.8 | the router |
| `tower-http` | 0.6 | trace, compression, `SetResponseHeader`. 0.6 rather than 0.7 because axum 0.8 depends on 0.6 and there is no reason to carry two |
| `tokio` | 1.53 | matches kaas-lib's floor |
| `figment` | 0.10 | YAML + env overlay, `features = ["yaml", "env"]` |
| `serde` / `serde_json` | 1 | |
| `arc-swap` | 1.9 | same as kaas-lib, for `ClusterHandle` |
| `thiserror` | 2.0 | |
| `tracing` / `tracing-subscriber` | 0.1 / 0.3 | |
| `rust-embed` | 8.12 | the frontend, `features = ["axum"]` not needed — serve manually |
| `utoipa` | 5.5 | schema derives from day one, so Orval has something to read |
| `utoipa-axum` | 0.2 | router integration |

Later phases add: `tokio-stream` 0.1 and `async-stream` 0.3 (Phase 3),
`openidconnect` 4.0, `tower-sessions` 0.15, `sqlx` 0.9 (Phase 4),
`apache-avro` 0.21, `prost-reflect` 0.16, `schema_registry_converter` 4.10,
`jsonschema` 0.49 (Phase 6), `rquickjs` 0.12 (Phase 6, JS predicates),
`notify` 8.2 (config reload; 9.0 is still a release candidate).

### Frontend

React 19.2, Vite 8, TypeScript, Tailwind 4.3, shadcn/ui, and the TanStack set:
Query 5, Table 8, Virtual 3, Router 1. Orval generates the client from the
utoipa spec.

Tailwind 4 configures in CSS, not `tailwind.config.js` — worth knowing before
copying a v3 shadcn snippet.

## Lints

Copied from kaas-lib, and PLAN.md §3 is right that they matter more here:
kaas-lib's rule 2 exists so a malformed record on one topic cannot take down a
server hosting other clusters, and kaas-ui **is** that server.

```toml
[workspace.lints.clippy]
unwrap_used              = "deny"
expect_used              = "deny"
panic                    = "deny"
indexing_slicing         = "deny"
as_conversions           = "deny"
cast_possible_truncation = "deny"
cast_sign_loss           = "deny"

[workspace.lints.rust]
unsafe_code                   = "forbid"
missing_debug_implementations = "warn"
```

Each crate opts in with `[lints] workspace = true`. Tests may unwrap freely via
the same `#![cfg_attr(test, allow(...))]` header kaas-lib uses.

## The three CI invariants

`cargo xtask ci` runs fmt, clippy, unit tests, and these three greps. Each
corresponds to a claim PLAN.md makes that is only true if mechanically checked.

**1. One construction site.** `Admin::connect(` appears nowhere;
`Admin::connect_read_only(` appears exactly once, in
`kaas-ui-core/src/registry.rs`. Backed by a unit test as well as the grep, since
the grep is easy to satisfy with a rename.

**2. No Kafka version number anywhere.** No `3.5`, no `4.2`, no parsing of a
broker version string, no per-version branch. If kaas-ui ever needs to know that
a Kafka release added something, that knowledge belongs in kaas-lib — see
[reference/upstream-asks.md](reference/upstream-asks.md).

**3. No local override committed.** The `xtask link` fence must be absent.

A fourth check is worth adding once there is a router: **no non-GET data route**.
`.post(`, `.put(`, `.patch(` and `.delete(` may appear only in the auth router.

## Error handling and the domain boundary

Apply kaas-lib's rule 1 one level up: **no `kafka_admin::*` or `kafka_meta::*`
type appears in a `utoipa` schema or an HTTP response.** `kaas-ui-core` defines
its own DTOs and converts at the boundary, for exactly the reason kaas-lib does
not expose `kafka-protocol` types — otherwise every library bump breaks the
generated TypeScript client.

The conversion layer is also where the four properties in PLAN.md §2 are
preserved rather than flattened:

- per-item results stay per-item, all the way to JSON;
- `Error` stays typed until the last possible moment, where
  [reference/http-contract.md](reference/http-contract.md) maps it to a status;
- `snapshot.age()` rides along in the envelope;
- the version table is projected, never recomputed.

## Verification

```sh
cargo xtask ci            # fmt + clippy + unit tests + the three greps
cargo xtask live          # the phase acceptance runs, against both clusters
cargo xtask docs          # openapi spec + regenerate the TS client
cargo xtask link|unlink   # kaas-lib local override
```

There is no `cargo xtask integration` and no `testcontainers` dependency:
[Docker is not available here](reference/environment.md), and two real
three-node clusters are a better target than one container. `cargo xtask live`
takes a target like kaas-lib's skill does and runs each phase's acceptance
assertions against it.

Unit tests must stay Docker-free *and* cluster-free, so `cargo xtask ci` runs
anywhere. Anything needing a broker goes in `live`.
