import { Loader2, RotateCw } from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { count } from "@/lib/format"

export function StreamStatus({
  phase,
  live,
  reconnecting,
  rows,
  onDownload,
  onRestart,
}: {
  phase: string | null
  live: boolean
  reconnecting: boolean
  rows: number
  onDownload(): void
  onRestart(): void
}) {
  return (
    <div className="flex items-center gap-2 text-[11px] text-ink-muted">
      {/* The count is also the way out of the browser: what is buffered is
          what the file contains, so the number that says how much there is
          is the thing you click to get it. */}
      {rows > 0 ? (
        <button
          type="button"
          onClick={onDownload}
          title={`Download these ${count(rows)} records as JSON — payload text is the list preview, truncated at 256 characters`}
          className="cursor-pointer tabular-nums underline decoration-dotted underline-offset-2 hover:text-ink"
        >
          {count(rows)} buffered
        </button>
      ) : (
        <span className="tabular-nums">0 buffered</span>
      )}

      {reconnecting ? (
        <Badge variant="outline" className="gap-1 text-warn-ink">
          <Loader2 className="size-3 animate-spin" aria-hidden /> reconnecting
        </Badge>
      ) : phase === "seeking" ? (
        <Badge variant="outline" className="gap-1">
          <Loader2 className="size-3 animate-spin" aria-hidden /> seeking
        </Badge>
      ) : phase === "streaming" && live ? (
        <Badge variant="outline" className="gap-1 text-ok">
          <span className="size-1.5 rounded-full bg-ok" aria-hidden /> live
        </Badge>
      ) : phase === "done" && live ? (
        // A live stream only ends because the server ended it — a rollout, or
        // a lifetime expiring. The toolbar has no restart button in this mode,
        // on the grounds that a live tail does not need one, so the badge that
        // reports the ending is what picks it back up.
        <Badge
          asChild
          variant="outline"
          className="cursor-pointer gap-1 text-warn-ink hover:bg-surface-raised"
        >
          <button
            type="button"
            onClick={onRestart}
            title="Open the stream again"
          >
            <RotateCw className="size-3" aria-hidden /> stream ended
          </button>
        </Badge>
      ) : phase === "done" ? (
        <Badge variant="outline">window read</Badge>
      ) : null}
    </div>
  )
}
