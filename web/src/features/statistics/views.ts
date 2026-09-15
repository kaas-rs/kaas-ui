// The statistics tab's sub-pages, in one list.
//
// A registry rather than a switch scattered across the render: adding a
// sub-page is one entry here plus one branch in `analysis-result.tsx`, and
// the rail, the URL schema and the fallback all follow from the list without
// being touched. The order here is the order the rail shows.
//
// Every entry answers `available` against the result it would render. A
// sub-page whose data did not arrive stays in the rail, disabled, saying why
// — a nav item that appears and disappears between runs is harder to trust
// than one that explains itself.

import type { TopicAnalysis } from "@/api/types"

/** In the URL, so a link opens on the sub-page it was sent about. */
export const ANALYSIS_VIEWS = ["statistics", "advisor"] as const

export type AnalysisView = (typeof ANALYSIS_VIEWS)[number]

/** The one a link with no `view` opens on, and the fallback for an absent one. */
export const DEFAULT_ANALYSIS_VIEW: AnalysisView = "statistics"

export type AnalysisViewEntry = {
  id: AnalysisView
  /** The rail's label. */
  label: string
  /** One line under it, saying what is on the page. */
  hint: string
  /** Whether this result has anything to show here. */
  available(result: TopicAnalysis): boolean
  /** Shown in place of the hint when it does not. */
  unavailable: string
}

export const ANALYSIS_VIEW_ENTRIES: AnalysisViewEntry[] = [
  {
    id: "statistics",
    label: "Statistics",
    hint: "what the scan folded",
    // The fold is the result. If there is a result at all, there is this.
    available: () => true,
    unavailable: "",
  },
  {
    id: "advisor",
    label: "Advisor",
    hint: "what to size it to",
    available: (result) => result.sizing !== undefined,
    unavailable: "the describes behind it did not answer",
  },
]

/**
 * The sub-page to render: the one asked for, or the default.
 *
 * A URL naming a sub-page this result cannot show falls back rather than
 * erroring — the same reasoning the topic tabs apply to a retired tab name.
 */
export function resolveAnalysisView(
  requested: AnalysisView,
  result: TopicAnalysis
): AnalysisView {
  const entry = ANALYSIS_VIEW_ENTRIES.find((view) => view.id === requested)
  return entry?.available(result) ? entry.id : DEFAULT_ANALYSIS_VIEW
}
