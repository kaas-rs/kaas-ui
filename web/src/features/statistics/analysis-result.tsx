import { AlertTriangle } from "lucide-react"

import type { TopicAnalysis } from "@/api/types"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { ErrorChips, Section, Stat } from "@/components/domain"
import { bytes, count, duration } from "@/lib/format"
import {
  displayTimeZone,
  formatTimestamp,
  useResolvedDateOrder,
} from "@/lib/settings"

import type { TopicSearch } from "@/features/messages/search"

import { AnalysisRail } from "./analysis-rail"
import { HourlyChart } from "./hourly-chart"
import { PartitionStatsTable } from "./partition-stats-table"
import { SizeTable } from "./size-table"
import { SizingAdviceSections } from "./sizing-advice"
import { resolveAnalysisView } from "./views"

export function AnalysisResult({
  result,
  onRerun,
  search,
  onSearch,
}: {
  result: TopicAnalysis
  onRerun(): void
  /** The topic page's URL state — the same object the message browser reads. */
  search: TopicSearch
  onSearch(next: Partial<TopicSearch>): void
}) {
  const timeZone = displayTimeZone()
  const dateOrder = useResolvedDateOrder()
  const totals = result.totalStats
  const view = resolveAnalysisView(search.view, result)

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

      {/* The rail first in the DOM and on the right on a wide screen:
          `flex-row-reverse` gets the placement without costing a screen
          reader — or a phone, where it lands above the content it steers. */}
      <div className="flex flex-col gap-6 lg:flex-row-reverse lg:items-start">
        <AnalysisRail
          result={result}
          active={view}
          onSelect={(next) => onSearch({ view: next })}
        />
        <div className="min-w-0 flex-1 space-y-6">
          {view === "statistics" ? (
            <>
              {/* The overview: what the run was, and the headline numbers it
                  produced. No heading over it — a card that opens the page and
                  holds its headline figures does not need a word above it
                  saying that it does.

                  Tinted rather than raised, and the tint is the brand accent
                  at a tenth: #E69F67 is ~2:1 on the paper ground, which makes
                  it useless as text and exactly right as a surface — the same
                  reason the rail marks its selected item with it. Every card
                  below is the ordinary raised one, so this reads as the top of
                  the page without a second border weight or a shadow. */}
              <Card className="bg-rust/10 border-rust/40 space-y-4 px-5 py-4">
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
                      {" · "}took{" "}
                      {duration(result.finishedAt - result.startedAt)}
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
                      {formatTimestamp(
                        totals.minTimestamp,
                        timeZone,
                        dateOrder
                      )}
                    </span>{" "}
                    and{" "}
                    <span className="font-mono">
                      {formatTimestamp(
                        totals.maxTimestamp,
                        timeZone,
                        dateOrder
                      )}
                    </span>
                    {result.clock ? <> · {result.clock}</> : null}
                  </p>
                ) : null}
              </Card>

              <Section title="Record sizes">
                <Card>
                  <CardContent className="space-y-2">
                    <SizeTable
                      keySize={totals.keySize}
                      valueSize={totals.valueSize}
                    />
                    <p className="text-[11px] text-ink-faint">
                      min, avg, max and sum are exact; the percentiles are
                      sketch estimates with a bounded ±4% relative error.
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
            </>
          ) : null}

          {/* The advice rides on this result rather than on a route of its
              own: it needs the write rate the fold measured, and metadata
              cannot supply one. A scan the two describes behind it could not
              follow leaves the statistics intact and this sub-page absent —
              which is what the rail says in place of its hint. */}
          {view === "advisor" && result.sizing ? (
            <SizingAdviceSections
              advice={result.sizing}
              profile={search.profile}
              onProfile={(profile) => onSearch({ profile })}
            />
          ) : null}
        </div>
      </div>
    </div>
  )
}
