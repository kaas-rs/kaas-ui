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
import { Input } from "@/components/ui/input"
import { Progress } from "@/components/ui/progress"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
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

function analysisUrl(
  envId: string,
  clusterId: string,
  topic: string,
  caps: { limit: number | null; maxMinutes: number }
): string {
  const params = new URLSearchParams()
  if (caps.limit !== null) params.set("limit", String(caps.limit))
  params.set("maxMinutes", String(caps.maxMinutes))
  // `EventSource` takes a URL, not a path handed to `fetch`, so the base
  // prefix is applied here — the same reason `streamUrl` does.
  return withBase(
    `/api/environments/${encodeURIComponent(envId)}/clusters/${encodeURIComponent(
      clusterId
    )}/topics/${encodeURIComponent(topic)}/analysis?${params}`
  )
}

/** The server's ceiling on `maxMinutes`, mirrored for the input's own check. */
const MAX_MINUTES = 30
const DEFAULT_MINUTES = 10

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
  // The caps, as typed. Records defaults to the topic's current retained
  // count — the same read as "everything", but a finish line the progress
  // bar can honestly reach on a topic that is still being produced to.
  // Blank means uncapped.
  const [recordCap, setRecordCap] = useState(() =>
    info.messageCount === null ? "" : String(info.messageCount)
  )
  const [minuteCap, setMinuteCap] = useState(String(DEFAULT_MINUTES))
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
    const limit = Math.floor(Number(recordCap))
    const minutes = Math.floor(Number(minuteCap))
    const es = new EventSource(
      analysisUrl(envId, clusterId, topic, {
        limit: Number.isFinite(limit) && limit > 0 ? limit : null,
        maxMinutes:
          Number.isFinite(minutes) && minutes > 0
            ? Math.min(minutes, MAX_MINUTES)
            : DEFAULT_MINUTES,
      })
    )
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
  }, [envId, clusterId, topic, recordCap, minuteCap, stop, queryClient])

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
  recordCap,
  minuteCap,
  onRecordCap,
  onMinuteCap,
  onStart,
}: {
  info: TopicDetail
  envId: string
  clusterId: string
  topic: string
  recordCap: string
  minuteCap: string
  onRecordCap(value: string): void
  onMinuteCap(value: string): void
  onStart(): void
}) {
  const size = useTopicSize(envId, clusterId, topic)
  const logical = size.data?.items[0]?.logicalBytes ?? null

  return (
    <Card>
      <CardContent className="space-y-4">
        <p className="text-[13px]">
          Analyse this topic: read records from the beginning and fold
          statistics — sizes, distinct keys, tombstones, an hourly histogram.
          Nothing is stored on the cluster; the scan is cancelled the moment
          this tab is left.
        </p>
        <div className="flex flex-wrap items-end gap-4">
          <label className="space-y-1 text-[12px]">
            <span className="text-ink-muted block">
              records to scan
              <span className="text-ink-faint ml-1">
                (blank = the whole topic)
              </span>
            </span>
            <Input
              type="number"
              min={1}
              value={recordCap}
              onChange={(event) => onRecordCap(event.target.value)}
              className="h-8 w-40 font-mono text-[12px]"
            />
          </label>
          <label className="space-y-1 text-[12px]">
            <span className="text-ink-muted block">
              max minutes
              <span className="text-ink-faint ml-1">(up to {MAX_MINUTES})</span>
            </span>
            <Input
              type="number"
              min={1}
              max={MAX_MINUTES}
              value={minuteCap}
              onChange={(event) => onMinuteCap(event.target.value)}
              className="h-8 w-24 font-mono text-[12px]"
            />
          </label>
          <Button size="sm" onClick={onStart}>
            <ChartColumn aria-hidden />
            analyse topic
          </Button>
        </div>
        <p className="text-ink-muted text-[12px]">
          The record cap defaults to the topic&apos;s current retained count
          {info.messageCount !== null ? (
            <>
              {" "}
              (<span className="font-mono">{count(info.messageCount)}</span>)
            </>
          ) : null}
          {logical !== null ? (
            <>
              , about <span className="font-mono">{bytes(logical)}</span> for
              one copy
            </>
          ) : null}
          . Whichever cap is reached first ends the scan, and the result says
          which sample its numbers describe. The read runs over the
          brokers&apos; shared connections — one analysis per cluster at a time.
        </p>
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

  const fractionNote =
    result.scannedFraction !== undefined
      ? ` — about ${Math.round(result.scannedFraction * 100)}% of the retained offset span`
      : ""

  return (
    <div className="space-y-6">
      <ErrorChips errors={result.errors} />
      {result.stoppedBy === "messageCap" ? (
        <p className="text-ink-muted text-[12px]">
          <strong>Capped result.</strong> The scan stopped at its configured
          record cap after {count(totals.totalMsgs)} records{fractionNote};
          every number below describes that sample, read from the beginning of
          the topic.
        </p>
      ) : null}
      {result.stoppedBy === "timeCap" ? (
        <p className="text-warn-ink flex items-start gap-2 text-[12px]">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          <span>
            <strong>Time-capped result.</strong> The scan hit its minute cap
            after {count(totals.totalMsgs)} records{fractionNote}; every number
            below covers only what was scanned. Raise the cap, or lower the
            record cap, to finish inside it.
          </span>
        </p>
      ) : null}
      {result.stoppedBy === "error" ? (
        <p className="text-warn-ink flex items-start gap-2 text-[12px]">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          <span>
            <strong>Partial result.</strong> An error ended the scan
            {fractionNote}; the numbers below cover what was read before it, and
            the error is named above.
          </span>
        </p>
      ) : null}

      <Card>
        <CardContent className="space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="text-ink-muted flex flex-wrap items-center gap-2 text-[12px]">
              <Badge
                variant={
                  result.stoppedBy === "end"
                    ? "outline"
                    : result.stoppedBy === "messageCap"
                      ? "secondary"
                      : "destructive"
                }
              >
                {
                  {
                    end: "complete",
                    messageCap: "capped",
                    timeCap: "time-capped",
                    error: "partial",
                  }[result.stoppedBy]
                }
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
            <Stat
              label="messages scanned"
              value={count(totals.totalMsgs)}
              hint="records read and folded — on a compacted or transactional topic this is legitimately below the offset span"
            />
            <Stat
              label="payload bytes"
              value={bytes(
                (totals.keySize?.sum ?? 0) + (totals.valueSize?.sum ?? 0)
              )}
              note="keys + values"
              hint="the bytes actually carried by keys and values, before replication — not the on-disk size"
            />
            <Stat
              label="≈ unique keys"
              value={count(totals.approxUniqKeys)}
              note="estimate"
              hint="distinct keys, from a cardinality sketch (±1.6%) — against messages scanned it reads as compaction headroom"
            />
            <Stat
              label="≈ unique values"
              value={count(totals.approxUniqValues)}
              note="estimate"
              hint="distinct values, from the same sketch — far below the message count means repeated payloads"
            />
            <Stat
              label="null keys"
              value={count(totals.nullKeys)}
              hint="records written without a key; they partition round-robin and can never be compacted together"
            />
            <Stat
              label="tombstones"
              value={count(totals.nullValues)}
              note="null values"
              hint="records with a null value — deletion markers on a compacted topic, and not the same as an empty value"
            />
            <Stat
              label="no timestamp"
              value={count(totals.missingTimestamps)}
              note={
                totals.missingTimestamps > 0
                  ? "excluded from the chart"
                  : undefined
              }
              hint="records whose producer set no timestamp; counted here rather than plotted as 1970"
            />
            <Stat
              label="malformed batches"
              value={count(totals.malformedBatches)}
              tone={totals.malformedBatches > 0 ? "warn" : undefined}
              hint="batches that would not decode at the protocol level — skipped and counted, the scan continues past them"
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

/**
 * A table header that says what its column means on hover — the same shape
 * the partition table uses on the overview tab, for the same reason: every
 * label here is a term with a plausible wrong reading.
 */
function Head({
  label,
  hint,
  right = true,
}: {
  label: string
  hint: string
  right?: boolean
}) {
  return (
    <TableHead className={right ? "text-right" : undefined}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="cursor-help decoration-dotted underline-offset-4 hover:underline">
            {label}
          </span>
        </TooltipTrigger>
        <TooltipContent>{hint}</TooltipContent>
      </Tooltip>
    </TableHead>
  )
}

const PERCENTILES: Array<{
  label: string
  hint: string
  pick(stats: SizeStats): number | undefined
  exact: boolean
}> = [
  {
    label: "min",
    hint: "the smallest — exact",
    pick: (s) => s.min,
    exact: true,
  },
  {
    label: "avg",
    hint: "the mean — exact",
    pick: (s) => Math.round(s.avg),
    exact: true,
  },
  {
    label: "p50",
    hint: "the median: half the records are smaller — a sketch estimate (±4%)",
    pick: (s) => s.p50,
    exact: false,
  },
  {
    label: "p75",
    hint: "three quarters are smaller — estimate",
    pick: (s) => s.p75,
    exact: false,
  },
  {
    label: "p95",
    hint: "19 of 20 are smaller — estimate",
    pick: (s) => s.p95,
    exact: false,
  },
  {
    label: "p99",
    hint: "99% are smaller — estimate; the usual sizing figure",
    pick: (s) => s.p99,
    exact: false,
  },
  {
    label: "p99.9",
    hint: "999 of 1000 are smaller — estimate; the outliers",
    pick: (s) => s.p999,
    exact: false,
  },
  {
    label: "max",
    hint: "the largest single record — exact",
    pick: (s) => s.max,
    exact: true,
  },
  {
    label: "sum",
    hint: "every record summed — exact",
    pick: (s) => s.sum,
    exact: true,
  },
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
              <Head
                key={column.label}
                label={column.exact ? column.label : `≈ ${column.label}`}
                hint={column.hint}
              />
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
 * Records per hour, as a single-series line on a **linear time axis**.
 *
 * The series is zero-filled: an hour nothing was written to is a point at
 * zero in its true position, because a line drawn only through the non-empty
 * hours would bridge a quiet night as if it never happened — the one lie a
 * write-rate chart must not tell. One hue from the design system's chart
 * ramp; a single series needs no legend, the section title names it. The
 * caption says which clock is being plotted, because `createTime` and
 * `logAppendTime` can disagree by however long a producer buffers.
 *
 * Hover is a column per hour, wider than the line, with the hour and its
 * count — the mark itself is too thin to be a hit target.
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
  const span = Math.max(1, Math.round((last - first) / HOUR) + 1)
  const peak = Math.max(...hours.map((hour) => hour.count))

  // Zero-filled, in hour order. Bounded: the accumulator caps its buckets,
  // so the span here is at most the cap plus the gaps inside it.
  const byHour = new Map(hours.map((hour) => [hour.hourStart, hour.count]))
  const series: Array<{ hourStart: number; count: number }> = []
  for (let index = 0; index < span; index += 1) {
    const hourStart = first + index * HOUR
    series.push({ hourStart, count: byHour.get(hourStart) ?? 0 })
  }

  const step = plotWidth / span
  const x = (hourStart: number) =>
    PLOT.left + ((hourStart - first) / HOUR) * step + step / 2
  const y = (value: number) => PLOT.top + plotHeight * (1 - value / peak)
  const path = series
    .map(
      (point, index) =>
        `${index === 0 ? "M" : "L"}${x(point.hourStart).toFixed(1)},${y(point.count).toFixed(1)}`
    )
    .join(" ")

  // Recessive gridlines; the labels wear ink, never the series colour.
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
        <path
          d={path}
          fill="none"
          stroke="var(--chart-1)"
          strokeWidth="2"
          strokeLinejoin="round"
          strokeLinecap="round"
        />
        {/* A visible marker only where the series is sparse enough for one
            per hour to read as points rather than as a rope of beads. */}
        {series.length <= 60
          ? series.map((point) => (
              <circle
                key={point.hourStart}
                cx={x(point.hourStart)}
                cy={y(point.count)}
                r="2.5"
                fill="var(--chart-1)"
              />
            ))
          : null}
        {series.map((point) => (
          <rect
            key={point.hourStart}
            x={x(point.hourStart) - step / 2}
            y={PLOT.top}
            width={Math.max(step, 1)}
            height={plotHeight}
            fill="transparent"
          >
            <title>
              {`${hourLabel(point.hourStart)} — ${count(point.count)} record${point.count === 1 ? "" : "s"}`}
            </title>
          </rect>
        ))}
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
            <Head label="partition" hint="its index within the topic" />
            <Head
              label="messages"
              hint="records scanned in this partition — one carrying most of them is a skewed partitioning key"
            />
            <Head
              label="min offset"
              hint="the lowest offset the scan read here"
            />
            <Head
              label="max offset"
              hint="the highest offset the scan read here"
            />
            <Head label="null keys" hint="records written without a key" />
            <Head
              label="tombstones"
              hint="null-value records — deletion markers on a compacted topic"
            />
            <Head
              label="≈ unique keys"
              hint="estimated distinct keys in this partition (sketch, ±1.6%)"
            />
            <Head
              label="avg value"
              hint="the mean value size in this partition — exact"
            />
            <Head
              label="bytes"
              hint="key plus value bytes scanned in this partition"
            />
            <Head
              label="malformed"
              hint="batches that would not decode; skipped and counted"
            />
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
