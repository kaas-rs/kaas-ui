import { ChartColumn } from "lucide-react"

import type { TopicDetail } from "@/api/types"
import { useTopicSize } from "@/api/client"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { bytes, count } from "@/lib/format"

import { MAX_MINUTES } from "./use-topic-analysis"

/**
 * The start state says what starting costs before anything is read: the
 * retained record count and the on-disk size are already known from the
 * overview's queries, and a full-topic read against shared broker
 * connections is not a thing to trigger by accident.
 */
export function StartCard({
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
