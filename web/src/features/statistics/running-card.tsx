import { Square } from "lucide-react"

import type { AnalysisProgress } from "@/api/types"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Progress } from "@/components/ui/progress"
import { bytes, count, duration } from "@/lib/format"

export function RunningCard({
  progress,
  onStop,
}: {
  progress: AnalysisProgress | null
  onStop(): void
}) {
  const fraction = progress?.fraction ?? null
  return (
    <Card>
      <CardContent className="space-y-3">
        <div className="flex items-center justify-between gap-4">
          <span className="text-[13px]">
            {progress === null
              ? "starting the scan…"
              : `scanned ${count(progress.msgsScanned)} records · ${bytes(progress.bytesScanned)} · ${duration(progress.elapsedMs)}`}
          </span>
          <Button size="sm" variant="outline" onClick={onStop}>
            <Square aria-hidden />
            stop
          </Button>
        </div>
        <Progress value={fraction === null ? null : fraction * 100} />
        {progress !== null && progress.malformedBatches > 0 ? (
          <p className="text-[12px] text-warn-ink">
            {count(progress.malformedBatches)} batch(es) would not decode and
            were skipped; the analysis continues past them.
          </p>
        ) : null}
      </CardContent>
    </Card>
  )
}
