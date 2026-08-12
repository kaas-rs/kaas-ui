// The statistics tab: an on-demand full-topic analysis.
//
// Not a metrics dashboard. Pressing start opens an `EventSource` on the
// analysis route, which reads every record from the beginning and folds
// statistics server-side; this component renders the progress frames and then
// the one terminal result. Closing the stream — leaving the tab, pressing
// stop — is the cancellation; there is no verb for it and none is needed.
//
// The finished result goes into the TanStack Query cache under the topic's
// key. That is deliberately unlike the message stream, whose rows must never
// enter the cache: a result is a terminal value, not a stream, so revisiting
// the tab is instant and costs no server-side store. The trade, made openly:
// the cache is per-browser, so two people analysing the same topic scan it
// twice.
//
// Two numbers lie unless labelled: the unique counts and the percentiles are
// sketch estimates, and every rendering of them here carries the ≈ and a note
// saying so. A p99 read as exact gets used to justify a partitioning decision.

import type {
  AnalysisProgress,
  ResourceError,
  TopicAnalysis,
  TopicDetail,
} from "@/api/types"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { ErrorChips } from "@/components/domain"

import { AnalysisResult } from "./analysis-result"
import { RunningCard } from "./running-card"
import { StartCard } from "./start-card"
import { useTopicAnalysis } from "./use-topic-analysis"

export type Phase =
  | { kind: "idle" }
  | { kind: "running"; progress: AnalysisProgress | null }
  | { kind: "done"; result: TopicAnalysis }
  | { kind: "failed"; error: ResourceError }

export function TopicStatistics({
  envId,
  clusterId,
  topic,
  info,
}: {
  envId: string
  clusterId: string
  topic: string
  info: TopicDetail
}) {
  const {
    phase,
    setPhase,
    start,
    stop,
    recordCap,
    setRecordCap,
    minuteCap,
    setMinuteCap,
  } = useTopicAnalysis({ envId, clusterId, topic, info })

  return (
    <div className="space-y-6">
      {phase.kind === "idle" ? (
        <StartCard
          info={info}
          envId={envId}
          clusterId={clusterId}
          topic={topic}
          recordCap={recordCap}
          minuteCap={minuteCap}
          onRecordCap={setRecordCap}
          onMinuteCap={setMinuteCap}
          onStart={start}
        />
      ) : null}
      {phase.kind === "running" ? (
        <RunningCard
          progress={phase.progress}
          onStop={() => {
            stop()
            setPhase({ kind: "idle" })
          }}
        />
      ) : null}
      {phase.kind === "failed" ? (
        <Card>
          <CardContent className="space-y-3">
            <ErrorChips errors={[phase.error]} />
            <Button size="sm" variant="outline" onClick={start}>
              try again
            </Button>
          </CardContent>
        </Card>
      ) : null}
      {phase.kind === "done" ? (
        <AnalysisResult result={phase.result} onRerun={start} />
      ) : null}
    </div>
  )
}
