// The sub-page rail: navigation, not tabs.
//
// A right-hand column on a wide screen and a scrollable row above the content
// on a narrow one, from one flex container — `flex-row-reverse` puts it on the
// right visually while leaving it *first* in the DOM, which is what a screen
// reader and a phone both want.
//
// Rendered from `ANALYSIS_VIEW_ENTRIES`, so a new sub-page needs nothing here.

import type { TopicAnalysis } from "@/api/types"
import { cn } from "@/lib/utils"

import {
  ANALYSIS_VIEW_ENTRIES,
  type AnalysisView,
  type AnalysisViewEntry,
} from "./views"

export function AnalysisRail({
  result,
  active,
  onSelect,
}: {
  result: TopicAnalysis
  active: AnalysisView
  onSelect(view: AnalysisView): void
}) {
  return (
    <nav
      aria-label="analysis views"
      className="-mx-1 flex gap-2 overflow-x-auto px-1 pb-1 lg:sticky lg:top-4 lg:mx-0 lg:w-52 lg:shrink-0 lg:flex-col lg:overflow-visible lg:px-0 lg:pb-0"
    >
      {ANALYSIS_VIEW_ENTRIES.map((entry) => (
        <RailItem
          key={entry.id}
          entry={entry}
          active={entry.id === active}
          available={entry.available(result)}
          onSelect={onSelect}
        />
      ))}
    </nav>
  )
}

function RailItem({
  entry,
  active,
  available,
  onSelect,
}: {
  entry: AnalysisViewEntry
  active: boolean
  available: boolean
  onSelect(view: AnalysisView): void
}) {
  return (
    <button
      type="button"
      // `page` rather than `true`: these are destinations, and the URL says so.
      aria-current={active ? "page" : undefined}
      disabled={!available}
      onClick={() => onSelect(entry.id)}
      className={cn(
        "focus-visible:ring-rust flex w-40 shrink-0 flex-col items-start gap-0.5 rounded-md border px-3 py-2 text-left transition-colors focus-visible:ring-2 focus-visible:outline-none lg:w-full lg:shrink",
        // The accent is a *surface* colour here — an edge on the selected
        // item, which is what the design system sanctions it for.
        active
          ? "border-rust bg-surface-raised text-ink"
          : "text-ink-muted border-transparent",
        available && !active && "hover:border-line hover:bg-surface-raised",
        !available && "cursor-not-allowed opacity-60"
      )}
    >
      <span className="text-[13px] font-medium">{entry.label}</span>
      <span className="text-ink-faint text-[11px] leading-snug">
        {available ? entry.hint : entry.unavailable}
      </span>
    </button>
  )
}
