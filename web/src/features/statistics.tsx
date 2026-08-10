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

import { useCallback, useEffect, useRef, useState } from "react"
import { useQueryClient } from "@tanstack/react-query"
import { AlertTriangle, ChartColumn, Square } from "lucide-react"

import type {
  AnalysisProgress,
  AnalysisStats,
  ResourceError,
  SizeStats,
  TopicAnalysis,
  TopicDetail,
} from "@/api/types"
import { withBase } from "@/api/base"
import { useTopicSize } from "@/api/client"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Progress } from "@/components/ui/progress"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  ErrorChips,
  Section,
  Stat,
  bytes,
  count,
  duration,
} from "@/components/domain"
import {
  displayTimeZone,
  formatTimestamp,
  useResolvedDateOrder,
} from "@/lib/settings"

function analysisUrl(envId: string, clusterId: string, topic: string): string {
  // `EventSource` takes a URL, not a path handed to `fetch`, so the base
  // prefix is applied here — the same reason `streamUrl` does.
  return withBase(
    `/api/environments/${encodeURIComponent(envId)}/clusters/${encodeURIComponent(
      clusterId
    )}/topics/${encodeURIComponent(topic)}/analysis`
  )
}

type Phase =
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
  const queryClient = useQueryClient()
  const cacheKey = ["analysis", envId, clusterId, topic]
  // A revisit opens on the cached result rather than on the start button —
  // the whole reason the result is allowed into the cache.
  const [phase, setPhase] = useState<Phase>(() => {
    const cached = queryClient.getQueryData<TopicAnalysis>(cacheKey)
    return cached ? { kind: "done", result: cached } : { kind: "idle" }
  })
  const source = useRef<EventSource | null>(null)

  const stop = useCallback(() => {
    source.current?.close()
    source.current = null
  }, [])

  // Leaving the tab unmounts this panel, and an analysis nobody is watching
  // is a full-topic read that should not be running.
  useEffect(() => stop, [stop])

  const start = useCallback(() => {
    stop()
    setPhase({ kind: "running", progress: null })
    const es = new EventSource(analysisUrl(envId, clusterId, topic))
    source.current = es

    es.addEventListener("progress", (event) => {
      const body = parse<AnalysisProgress>((event as MessageEvent).data)
      if (body) setPhase({ kind: "running", progress: body })
    })
    es.addEventListener("result", (event) => {
      const body = parse<TopicAnalysis>((event as MessageEvent).data)
      if (!body) return
      queryClient.setQueryData(cacheKey, body)
      setPhase({ kind: "done", result: body })
    })
    es.addEventListener("error", (event) => {
      // A `MessageEvent` with data is the server's own error event; a bare
      // `Event` is the connection dropping. The route sends `phase: done`
      // after every result, so a drop mid-scan without a result is a failure
      // worth saying — EventSource would silently rerun the whole scan.
      const data = (event as MessageEvent).data
      if (typeof data === "string") {
        const body = parse<ResourceError>(data)
        if (body) {
          setPhase((previous) =>
            previous.kind === "done"
              ? previous
              : { kind: "failed", error: body }
          )
        }
        return
      }
      stop()
      setPhase((previous) =>
        previous.kind === "running"
          ? {
              kind: "failed",
              error: {
                resource: topic,
                kind: "transport",
                code: null,
                codeNumber: null,
                message:
                  "the connection dropped before a result arrived; the scan was cancelled",
                retriable: true,
              },
            }
          : previous
      )
    })
    es.addEventListener("phase", (event) => {
      const body = parse<{ phase: string }>((event as MessageEvent).data)
      // `done` is deliberate — the result (or the error) is already on
      // screen, and EventSource must not reconnect and rerun the read.
      if (body?.phase === "done") stop()
    })
    // The dependency list names the identity of the analysis, not the cache
    // key array, whose reference changes per render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [envId, clusterId, topic, stop, queryClient])

  return (
    <div className="space-y-6">
      {phase.kind === "idle" ? (
        <StartCard
          info={info}
          envId={envId}
          clusterId={clusterId}
          topic={topic}
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

/**
 * The start state says what starting costs before anything is read: the
 * retained record count and the on-disk size are already known from the
 * overview's queries, and a full-topic read against shared broker
 * connections is not a thing to trigger by accident.
 */
function StartCard({
  info,
  envId,
  clusterId,
  topic,
  onStart,
}: {
  info: TopicDetail
  envId: string
  clusterId: string
  topic: string
  onStart(): void
}) {
  const size = useTopicSize(envId, clusterId, topic)
  const logical = size.data?.items[0]?.logicalBytes ?? null

  return (
    <Card>
      <CardContent className="space-y-3">
        <p className="text-[13px]">
          Analyse this topic: read every record from the beginning and fold
          statistics — sizes, distinct keys, tombstones, an hourly histogram.
          Nothing is stored on the cluster; the scan is cancelled the moment
          this tab is left.
        </p>
        <p className="text-[12px] text-ink-muted">
          This reads{" "}
          <span className="font-mono">
            {info.messageCount === null
              ? "every retained record"
              : `~${count(info.messageCount)} records`}
          </span>
          {logical !== null ? (
            <>
              {" "}
              (<span className="font-mono">{bytes(logical)}</span> one copy)
            </>
          ) : null}{" "}
          over the brokers&apos; shared connections — one analysis runs per
          cluster at a time.
        </p>
        <Button size="sm" onClick={onStart}>
          <ChartColumn aria-hidden />
          analyse topic
        </Button>
      </CardContent>
    </Card>
  )
}

function RunningCard({
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

function AnalysisResult({
  result,
  onRerun,
}: {
  result: TopicAnalysis
  onRerun(): void
}) {
  const timeZone = displayTimeZone()
  const dateOrder = useResolvedDateOrder()
  const totals = result.totalStats

  return (
    <div className="space-y-6">
      <ErrorChips errors={result.errors} />
      {!result.complete ? (
        <p className="flex items-start gap-2 text-[12px] text-warn-ink">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          <span>
            <strong>Partial result.</strong> The scan stopped
            {result.scannedFraction !== undefined
              ? ` after ~${Math.round(result.scannedFraction * 100)}% of the planned window`
              : " before reading the whole topic"}
            ; every number below covers only what was scanned.
          </span>
        </p>
      ) : null}

      <Card>
        <CardContent className="space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex flex-wrap items-center gap-2 text-[12px] text-ink-muted">
              <Badge variant={result.complete ? "outline" : "destructive"}>
                {result.complete ? "complete" : "partial"}
              </Badge>
              <span>
                analysed{" "}
                {formatTimestamp(result.startedAt, timeZone, dateOrder)}
                {" · "}took {duration(result.finishedAt - result.startedAt)}
              </span>
            </div>
            <Button size="sm" variant="outline" onClick={onRerun}>
              analyse again
            </Button>
          </div>
          <dl className="grid grid-cols-2 gap-x-6 gap-y-3 text-[13px] sm:grid-cols-4">
            <Stat label="messages scanned" value={count(totals.totalMsgs)} />
            <Stat
              label="payload bytes"
              value={bytes(
                (totals.keySize?.sum ?? 0) + (totals.valueSize?.sum ?? 0)
              )}
              note="keys + values"
            />
            <Stat
              label="≈ unique keys"
              value={count(totals.approxUniqKeys)}
              note="estimate"
            />
            <Stat
              label="≈ unique values"
              value={count(totals.approxUniqValues)}
              note="estimate"
            />
            <Stat label="null keys" value={count(totals.nullKeys)} />
            <Stat
              label="tombstones"
              value={count(totals.nullValues)}
              note="null values"
            />
            <Stat
              label="no timestamp"
              value={count(totals.missingTimestamps)}
              note={
                totals.missingTimestamps > 0
                  ? "excluded from the chart"
                  : undefined
              }
            />
            <Stat
              label="malformed batches"
              value={count(totals.malformedBatches)}
              tone={totals.malformedBatches > 0 ? "warn" : undefined}
            />
          </dl>
          {totals.minTimestamp !== undefined &&
          totals.maxTimestamp !== undefined ? (
            <p className="text-[12px] text-ink-muted">
              written between{" "}
              <span className="font-mono">
                {formatTimestamp(totals.minTimestamp, timeZone, dateOrder)}
              </span>{" "}
              and{" "}
              <span className="font-mono">
                {formatTimestamp(totals.maxTimestamp, timeZone, dateOrder)}
              </span>
              {result.clock ? <> · {result.clock}</> : null}
            </p>
          ) : null}
        </CardContent>
      </Card>

      <Section title="Record sizes">
        <Card>
          <CardContent className="space-y-2">
            <SizeTable keySize={totals.keySize} valueSize={totals.valueSize} />
            <p className="text-[11px] text-ink-faint">
              min, avg, max and sum are exact; the percentiles are sketch
              estimates with a bounded ±4% relative error.
            </p>
          </CardContent>
        </Card>
      </Section>

      {totals.hourlyMsgCounts.length > 0 ? (
        <Section title="Messages per hour">
          <Card>
            <CardContent className="space-y-2">
              <HourlyChart
                stats={totals}
                timeZone={timeZone}
                clock={result.clock ?? null}
              />
            </CardContent>
          </Card>
        </Section>
      ) : null}

      <Section title="Per partition">
        <PartitionStatsTable partitions={result.partitionStats} />
      </Section>
    </div>
  )
}

const PERCENTILES: Array<{
  label: string
  pick(stats: SizeStats): number | undefined
  exact: boolean
}> = [
  { label: "min", pick: (s) => s.min, exact: true },
  { label: "avg", pick: (s) => Math.round(s.avg), exact: true },
  { label: "p50", pick: (s) => s.p50, exact: false },
  { label: "p75", pick: (s) => s.p75, exact: false },
  { label: "p95", pick: (s) => s.p95, exact: false },
  { label: "p99", pick: (s) => s.p99, exact: false },
  { label: "p99.9", pick: (s) => s.p999, exact: false },
  { label: "max", pick: (s) => s.max, exact: true },
  { label: "sum", pick: (s) => s.sum, exact: true },
]

function SizeTable({
  keySize,
  valueSize,
}: {
  keySize?: SizeStats
  valueSize?: SizeStats
}) {
  return (
    <div className="overflow-x-auto">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead />
            {PERCENTILES.map((column) => (
              <TableHead key={column.label} className="text-right">
                {column.exact ? column.label : `≈ ${column.label}`}
              </TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {(
            [
              ["key", keySize],
              ["value", valueSize],
            ] as const
          ).map(([name, stats]) => (
            <TableRow key={name}>
              <TableCell className="text-ink-muted">{name}</TableCell>
              {PERCENTILES.map((column) => (
                <TableCell
                  key={column.label}
                  className="text-right font-mono whitespace-nowrap"
                >
                  {stats ? bytes(column.pick(stats) ?? null) : "—"}
                </TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

/**
 * Records per hour, as a single-series bar chart on a **linear time axis** —
 * an hour nothing was written to is a gap at its true position, not a missing
 * category. One hue from the design system's chart ramp; a single series
 * needs no legend, the section title names it. The x label says which clock
 * is being plotted, because `createTime` and `logAppendTime` can disagree by
 * however long a producer buffers.
 */
function HourlyChart({
  stats,
  timeZone,
  clock,
}: {
  stats: AnalysisStats
  timeZone: string
  clock: string | null
}) {
  const HOUR = 3_600_000
  const WIDTH = 800
  const HEIGHT = 190
  const PLOT = { left: 46, right: 8, top: 8, bottom: 28 }
  const plotWidth = WIDTH - PLOT.left - PLOT.right
  const plotHeight = HEIGHT - PLOT.top - PLOT.bottom

  const hours = stats.hourlyMsgCounts
  if (hours.length === 0) return null
  const first = hours[0]?.hourStart ?? 0
  const last = hours[hours.length - 1]?.hourStart ?? first
  const span = Math.max(1, (last - first) / HOUR + 1)
  const peak = Math.max(...hours.map((hour) => hour.count))

  const barWidth = Math.max(1, plotWidth / span - 2)
  const x = (hourStart: number) =>
    PLOT.left + ((hourStart - first) / HOUR) * (plotWidth / span)
  const y = (value: number) => PLOT.top + plotHeight * (1 - value / peak)

  // Three recessive gridlines; the labels wear ink, never the series colour.
  const ticks = [peak, Math.round(peak / 2)].filter(
    (tick, index, all) => tick > 0 && all.indexOf(tick) === index
  )

  const hourLabel = (ms: number) =>
    new Intl.DateTimeFormat(undefined, {
      timeZone,
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    }).format(new Date(ms))

  return (
    <figure className="space-y-1">
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        className="h-auto w-full"
        role="img"
        aria-label="records per hour"
      >
        {ticks.map((tick) => (
          <g key={tick}>
            <line
              x1={PLOT.left}
              x2={WIDTH - PLOT.right}
              y1={y(tick)}
              y2={y(tick)}
              stroke="var(--line)"
              strokeWidth="1"
            />
            <text
              x={PLOT.left - 6}
              y={y(tick) + 3}
              textAnchor="end"
              fontSize="10"
              fill="var(--ink-muted)"
            >
              {tick >= 10_000 ? `${Math.round(tick / 1000)}k` : tick}
            </text>
          </g>
        ))}
        <line
          x1={PLOT.left}
          x2={WIDTH - PLOT.right}
          y1={PLOT.top + plotHeight}
          y2={PLOT.top + plotHeight}
          stroke="var(--line-strong)"
          strokeWidth="1"
        />
        {hours.map((hour) => {
          const top = y(hour.count)
          const height = Math.max(1, PLOT.top + plotHeight - top)
          return (
            <rect
              key={hour.hourStart}
              x={x(hour.hourStart)}
              y={PLOT.top + plotHeight - height}
              width={barWidth}
              height={height}
              rx={Math.min(2, barWidth / 2)}
              fill="var(--chart-1)"
            >
              <title>
                {`${hourLabel(hour.hourStart)} — ${count(hour.count)} record${hour.count === 1 ? "" : "s"}`}
              </title>
            </rect>
          )
        })}
        <text
          x={PLOT.left}
          y={HEIGHT - 8}
          fontSize="10"
          fill="var(--ink-muted)"
        >
          {hourLabel(first)}
        </text>
        <text
          x={WIDTH - PLOT.right}
          y={HEIGHT - 8}
          textAnchor="end"
          fontSize="10"
          fill="var(--ink-muted)"
        >
          {hourLabel(last + HOUR - 1)}
        </text>
      </svg>
      <figcaption className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-ink-faint">
        <span>
          plotted by {clock ?? "record timestamp"}
          {stats.missingTimestamps > 0
            ? ` · ${count(stats.missingTimestamps)} record(s) with no timestamp are not plotted`
            : ""}
        </span>
        {stats.hourlyTruncated ? (
          <span className="text-warn-ink">
            the hour map hit its ceiling — this chart is a view, not the whole
            story
          </span>
        ) : null}
      </figcaption>
    </figure>
  )
}

function PartitionStatsTable({ partitions }: { partitions: AnalysisStats[] }) {
  return (
    <div className="overflow-x-auto rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="text-right">partition</TableHead>
            <TableHead className="text-right">messages</TableHead>
            <TableHead className="text-right">min offset</TableHead>
            <TableHead className="text-right">max offset</TableHead>
            <TableHead className="text-right">null keys</TableHead>
            <TableHead className="text-right">tombstones</TableHead>
            <TableHead className="text-right">≈ unique keys</TableHead>
            <TableHead className="text-right">avg value</TableHead>
            <TableHead className="text-right">bytes</TableHead>
            <TableHead className="text-right">malformed</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {partitions.map((partition) => (
            <TableRow key={partition.partition ?? -1}>
              <TableCell className="text-right font-mono">
                {partition.partition}
              </TableCell>
              <TableCell className="text-right font-mono">
                {count(partition.totalMsgs)}
              </TableCell>
              <TableCell className="text-right font-mono">
                {count(partition.minOffset ?? null)}
              </TableCell>
              <TableCell className="text-right font-mono">
                {count(partition.maxOffset ?? null)}
              </TableCell>
              <TableCell className="text-right font-mono">
                {count(partition.nullKeys)}
              </TableCell>
              <TableCell className="text-right font-mono">
                {count(partition.nullValues)}
              </TableCell>
              <TableCell className="text-right font-mono">
                {count(partition.approxUniqKeys)}
              </TableCell>
              <TableCell className="text-right font-mono">
                {bytes(
                  partition.valueSize
                    ? Math.round(partition.valueSize.avg)
                    : null
                )}
              </TableCell>
              <TableCell className="text-right font-mono">
                {bytes(
                  (partition.keySize?.sum ?? 0) +
                    (partition.valueSize?.sum ?? 0)
                )}
              </TableCell>
              <TableCell
                className={
                  partition.malformedBatches > 0
                    ? "text-warn-ink text-right font-mono"
                    : "text-right font-mono"
                }
              >
                {count(partition.malformedBatches)}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

function parse<T>(raw: string): T | null {
  try {
    return JSON.parse(raw) as T
  } catch {
    return null
  }
}
