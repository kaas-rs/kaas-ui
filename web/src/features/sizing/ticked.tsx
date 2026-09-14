// Backticks in server prose, rendered as code.
//
// The diagnostics are written in `kaas-ui-core/src/sizing.rs`, where naming a
// setting means writing `` `segment.ms` `` — the same convention the rest of
// this repo's prose uses. Splitting on the backtick here keeps that one
// spelling working in both places instead of making the Rust choose between
// readable source and readable output.
//
// Deliberately not a markdown renderer. One delimiter, no nesting, no links:
// the strings come from our own crate, and the moment this parses more than
// that it becomes a thing to audit.

import { Fragment } from "react"

export function Ticked({ text }: { text: string }) {
  return (
    <>
      {text.split("`").map((part, index) =>
        index % 2 === 1 ? (
          <code
            key={index}
            className="rounded-sm bg-surface-sunken px-1 font-mono text-[0.95em]"
          >
            {part}
          </code>
        ) : (
          <Fragment key={index}>{part}</Fragment>
        )
      )}
    </>
  )
}
