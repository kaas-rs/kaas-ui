import { useEffect, useState } from "react"

import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"
import { duration } from "@/lib/format"

/** "as of 4s ago", ticking, warm past the configured staleness ceiling. */
export function SnapshotAge({
  ageMs,
  asOfMs,
  maxStalenessMs,
}: {
  ageMs: number | null | undefined
  /**
   * When `ageMs` was true — the owning query's `dataUpdatedAt`. The tick is
   * anchored here, not at mount, so each refetch resets the display instead
   * of compounding with how long the component has been on screen.
   */
  asOfMs: number
  maxStalenessMs?: number
}) {
  const [now, setNow] = useState(() => Date.now())

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [])

  if (ageMs === null || ageMs === undefined) return null
  // `now` only advances once a second, so right after a refetch it can sit
  // behind `asOfMs`; clamp rather than briefly understate the age.
  const age = ageMs + Math.max(0, now - asOfMs)
  const stale = maxStalenessMs !== undefined && age > maxStalenessMs

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className={cn(
            "text-[12px]",
            stale ? "font-medium text-warn-ink" : "text-ink-faint"
          )}
        >
          as of {duration(age)} ago
        </span>
      </TooltipTrigger>
      <TooltipContent>
        {stale
          ? "older than this cluster's staleness ceiling"
          : "age of the metadata snapshot this was built from"}
      </TooltipContent>
    </Tooltip>
  )
}
