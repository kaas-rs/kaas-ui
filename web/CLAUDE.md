# Frontend

## Design

`docs/reference/design-system.md`. The palette is mdbook's `rust` theme — the
same one the kaas-lib book uses — with semantic tokens, a derived dark mode, and
status colours mdbook has no need for.

One trap: `#E69F67` is ~2:1 on the paper ground. It is a **surface** colour —
active nav, focus ring, selected edge. For accent text on light use
`--color-accent-ink` (`#8F5A2B`).

## Traps

- **Streams do not go in the TanStack Query cache.** SSE feeds a capped ring
  buffer in its own hook.
