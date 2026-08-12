import { AlertTriangle } from "lucide-react"

import type { ResolvedSeek, StreamProgress } from "@/api/types"
import { Progress } from "@/components/ui/progress"
import { count } from "@/lib/format"
import { formatTimestamp, useResolvedDateOrder } from "@/lib/settings"

/** Everything the stream wants to say that is not a row. */
export function StreamNotices({
  timeZone,
  dropped,
  progress,
  resolved,
  error,
  phase,
}: {
  timeZone: string
  dropped: number
  progress: StreamProgress | null
  resolved: ResolvedSeek | null
  error: { message: string } | null
  phase: string | null
}) {
  // The seek someone typed, written back to them the way they typed it. It was
  // a UTC ISO string, which is not the notation the picker above takes.
  const dateOrder = useResolvedDateOrder()
  // The bar is an in-flight indicator, not a result, so it goes when the scan
  // does. A window that has been read already says so twice — the "window
  // read" badge and the terminal row — and a bar left sitting underneath them
  // reads as a control that stopped working rather than one that finished.
  //
  // That is not hypothetical: the last frame of a bounded scan is always
  // `1.0`, and on the default 500-record window it is the *only* frame, so
  // this element's whole visible life was a full bar under a finished list.
  const showBar =
    phase === "streaming" &&
    progress?.fraction !== null &&
    progress?.fraction !== undefined
  const noticeCount =
    (dropped > 0 ? 1 : 0) +
    (error ? 1 : 0) +
    (resolved?.unresolved ? 1 : 0) +
    (progress?.orderingDegraded ? 1 : 0)
  if (!noticeCount && !showBar) return null

  return (
    <div className="shrink-0 border-b border-line">
      {showBar ? (
        <Progress
          value={(progress?.fraction ?? 0) * 100}
          className="h-0.5 rounded-none"
        />
      ) : null}
      <div className="space-y-1 px-4 py-1.5 empty:hidden">
        {error ? (
          <p className="flex items-start gap-2 text-[11px] text-danger">
            <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
            {error.message}
          </p>
        ) : null}
        {dropped > 0 ? (
          // Never suppressed. Silently losing records in a debugging tool is
          // worse than showing a gap.
          <p className="text-[11px] text-warn-ink">
            {count(dropped)} message(s) were dropped to keep the stream ahead of
            this browser.
          </p>
        ) : null}
        {resolved?.unresolved ? (
          <p className="flex items-start gap-2 text-[11px] text-warn-ink">
            <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
            <span>
              This cluster resolved{" "}
              <span title={new Date(resolved.timestamp).toISOString()}>
                {formatTimestamp(resolved.timestamp, timeZone, dateOrder)}
              </span>{" "}
              to no offset on any of its {resolved.partitions.length}{" "}
              partitions, so the window is empty. Brokers that keep no timestamp
              index answer a time seek this way; seeking by offset still works.
            </span>
          </p>
        ) : null}
        {progress?.orderingDegraded ? (
          <p className="text-[11px] text-ink-muted">
            Approximately ordered across partitions — records may be up to{" "}
            {count(progress.reorderWindow)} apart. Within a partition the order
            is exact.
          </p>
        ) : null}
      </div>
    </div>
  )
}
