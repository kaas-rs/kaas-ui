import { cn } from "@/lib/utils"

import type { DiffLine } from "./diff-lines"
import { diffLines } from "./diff-lines"
import { prettyJson } from "./pretty-json"

/**
 * A line diff between two registered versions.
 *
 * Both sides are normalised first — parsed and re-printed where they are JSON
 * — so a version that differs only in the registry's whitespace shows as
 * unchanged rather than as every line rewritten.
 */
export function Diff({
  before,
  after,
  compact,
}: {
  before: string
  after: string
  compact: boolean
}) {
  const left = prettyJson(before).split("\n")
  const right = prettyJson(after).split("\n")
  const lines = diffLines(left, right)

  const shown: DiffRow[] = compact
    ? collapse(lines)
    : lines.map((line) => ({ line }))

  return (
    <pre className="max-h-[45vh] overflow-auto rounded-md border border-line bg-surface-sunken p-3 font-mono text-[11px] leading-relaxed whitespace-pre">
      {shown.map((row, index) =>
        row.elided !== undefined ? (
          // The count, not a bare `…`. "Twelve lines you are not being shown"
          // is a different claim from "something is hidden here", and only the
          // first tells you whether compact is costing you anything.
          <div
            key={index}
            className="text-ink-faint my-1 border-y border-dashed border-line/60 py-0.5 select-none"
          >
            {"  ⋯ "}
            {row.elided} unchanged line{row.elided === 1 ? "" : "s"}
          </div>
        ) : (
          <div
            key={index}
            className={cn(
              row.line?.kind === "added" && "bg-ok/15 text-ok-ink",
              row.line?.kind === "removed" && "bg-danger/15 text-danger"
            )}
          >
            {row.line?.kind === "added"
              ? "+"
              : row.line?.kind === "removed"
                ? "-"
                : " "}
            {row.line?.text}
          </div>
        )
      )}
    </pre>
  )
}

/** How many unchanged lines to keep either side of a change. */
const CONTEXT = 3

/** A rendered line, or a stand-in for the ones that were dropped. */
interface DiffRow {
  line?: DiffLine
  elided?: number
}

/**
 * Drop the unchanged lines, keeping a few either side of every change.
 *
 * A run is only worth eliding when it is taller than the marker replacing it:
 * collapsing two lines into "⋯ 2 unchanged lines" saves no height and costs
 * the reader the two lines. So a short gap between two changes is kept whole,
 * which also stops a dense diff turning into a ladder of markers.
 */
function collapse(lines: DiffLine[]): DiffRow[] {
  const keep = new Array<boolean>(lines.length).fill(false)
  lines.forEach((line, index) => {
    if (line.kind === "same") return
    const from = Math.max(0, index - CONTEXT)
    const to = Math.min(lines.length - 1, index + CONTEXT)
    for (let near = from; near <= to; near += 1) keep[near] = true
  })

  const out: DiffRow[] = []
  // Where the current run of dropped lines began, or -1 for "not in one".
  let start = -1
  const flush = (end: number) => {
    if (start < 0) return
    const run = end - start
    if (run <= 2) {
      for (let index = start; index < end; index += 1) {
        const line = lines[index]
        if (line) out.push({ line })
      }
    } else {
      out.push({ elided: run })
    }
    start = -1
  }

  lines.forEach((line, index) => {
    if (keep[index]) {
      flush(index)
      out.push({ line })
    } else if (start < 0) {
      start = index
    }
  })
  flush(lines.length)
  return out
}
