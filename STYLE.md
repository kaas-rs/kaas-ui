# STYLE.md — consuming `with_*` builders

Adopted from [kaas-lib's `STYLE.md`][upstream], which is the canonical statement
of the convention and carries the full reasoning, the edge cases and the notes
for reviewers porting it elsewhere. That file is written to be copied; this one
is kaas-ui's copy, plus the three things that are true here and not there.

[upstream]: https://github.com/kaas-rs/kaas-lib/blob/main/STYLE.md

## The rule

Every optional setting is a `#[must_use] pub fn with_x(mut self, …) -> Self` on
the domain type itself; required data goes in `new()`. There is no builder
struct and no terminal `build()`.

```rust
let envelope = Envelope::new(rows)      // required data
    .with_total(matched)                // everything else optional
    .with_snapshot_age(age);
```

- **Required data goes in `new()`. Everything optional is a `with_*` method.**
- **The signature is always `#[must_use] pub fn with_x(mut self, …) -> Self`.**
- **The type is its own builder.** No `FooBuilder`, no `build()`.
- **The prefix is `with_`, without exception** — booleans included.
- **Never `&mut self` setters. Never public mutable fields** for anything a
  caller is expected to set.
- **Setters assign; last call wins.** Anything additive says so in its name and
  its documentation.
- **Take `impl Into<T>` / `impl IntoIterator<Item = T>`.**
- **Settings a caller might relay** get a `with_maybe_*` sibling taking
  `Option<T>`, which assigns rather than merges.

`#[must_use]` is the half that bites, because the failure mode is silent:

```rust
let mut envelope = Envelope::new(rows);
envelope.with_total(count);   // compiles, does nothing, loses the total
```

Upstream found this on `ConnectionConfig::read_only` — the one setting in that
crate that exists as a safety gate, silently handing back a *writable*
connection when its result was dropped. That is why kaas-lib 0.4.0 annotated all
58 of its own, and why we check ours.

## Enforced, not intended

`cargo xtask ci` fails if any consuming builder under `crates/` is missing
`#[must_use]` or its `with_` prefix — `consuming_builders_are_withers` in
`xtask/src/checks.rs`, alongside the read-only and no-version-literal greps. It
reports the count on success, because a checker that has been outrun by a
refactor and silently matches nothing is worse than no checker.

## The exceptions, and why they are not holes

**`Envelope::with_errors` is additive.** Every other setter in the workspace
assigns. This one extends, because a handler enriches in stages and each stage
contributes the failures it learned. The guide reads a plural as the whole set,
so the deviation is stated in the doc comment rather than left to be discovered.
There is no replacing sibling: dropping an error already recorded is not
something a handler has a reason to do.

**Config structs have public fields.** `Config`, `ClusterEntry`, `TlsSettings`
and the rest of `kaas-ui-core::config` are figment deserialization targets. No
caller builds one fluently — YAML and the environment do — so the setter rule
has nothing to attach to. They are data, not a builder surface.

**`Envelope::items` is mutated in place.** The topic handlers enrich partitions
with offsets after construction. That is the case the guide sends elsewhere:
"the value must be mutated after construction … add explicit domain methods for
that", not `&mut` setters bolted onto the builder surface.

## What this does not cover

kaas-lib's types are kaas-lib's. `ScanSpec` and `TailSpec` take bare setters as
of 0.4.0 — `.partitions(…)`, `.limit(…)` — which is the one place a reader will
see both dialects in one expression. Upstream's own `STYLE.md` landed after that
release, so expect those to become `with_*` in a later version; when they do it
is a rename at our call sites and nothing more. Do not wrap them here to paper
over the difference.
