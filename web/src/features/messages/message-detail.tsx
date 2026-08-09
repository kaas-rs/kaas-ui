// The detail panel: master–detail, never inline expansion.
//
// Rows do not expand. An expanding row is a variable-height row, and variable
// heights are what make the fixed-row-height virtualizer — and the exact
// scroll compensation that depends on it — impossible.
//
// The panel is driven by a selected id and a *retained copy* of that row's
// list metadata. A live buffer is a moving window: the row someone selected
// forty seconds ago has very likely aged out of it, and clearing the panel
// underneath them because of that would be the most annoying possible
// behaviour. So the header keeps rendering, with a note saying why.

import { AlertTriangle, Loader2 } from "lucide-react"

import { useMessageDetail } from "@/api/client"
import type { StreamRow } from "@/api/types"
import { Badge } from "@/components/ui/badge"
import { Empty, Mono, bytes } from "@/components/domain"
import {
  displayTimeZone,
  formatTimestamp,
  useResolvedDateOrder,
} from "@/lib/settings"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { PayloadBlock } from "./payload"

export interface MessageDetailPanelProps {
  envId: string
  clusterId: string
  topic: string
  /** The selected id, or nothing selected. */
  selectedId?: string
  /** The row as it was when it was selected, kept so the panel survives eviction. */
  retained?: StreamRow
  /** Whether that row is still in the buffer. */
  present: boolean
  /**
   * The toolbar's codec override, so the panel reads the record the same way
   * as the row that was clicked. A list rendered as hex opening into a
   * registry-decoded panel would be the view disagreeing with itself.
   */
  keyCodec?: string
  valueCodec?: string
}

export function MessageDetailPanel({
  envId,
  clusterId,
  topic,
  selectedId,
  retained,
  present,
  keyCodec,
  valueCodec,
}: MessageDetailPanelProps) {
  const detail = useMessageDetail(envId, clusterId, topic, selectedId, {
    keyCodec,
    valueCodec,
  })

  if (!selectedId) {
    return (
      <Empty>
        Select a message to inspect its payload. Use <Mono>j</Mono> and{" "}
        <Mono>k</Mono> to move through the list.
      </Empty>
    )
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="shrink-0 border-b border-line px-4 py-3">
        <div className="flex items-baseline gap-2">
          <h2 className="text-sm font-medium">
            Offset{" "}
            <span className="tabular-nums">
              {retained?.offset.toLocaleString()}
            </span>
          </h2>
          <Badge variant="outline" className="tabular-nums">
            partition {retained?.partition}
          </Badge>
          {retained?.kind === "record" && retained.transactional ? (
            <Badge variant="outline">transactional</Badge>
          ) : null}
        </div>
        {retained?.kind === "record" ? (
          <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
            <dt className="text-ink-faint">Timestamp</dt>
            <dd className="tabular-nums">
              <RecordTime at={retained.timestamp} />{" "}
              <span className="text-ink-faint">({retained.timestampType})</span>
            </dd>
            <dt className="text-ink-faint">Value size</dt>
            <dd className="tabular-nums">
              {retained.value ? bytes(retained.value.bytes) : "tombstone"}
            </dd>
          </dl>
        ) : null}
        {!present ? (
          <p className="mt-2 text-[11px] text-warn-ink">
            No longer in the buffer — the stream has moved past it. The payload
            below is still read from the topic.
          </p>
        ) : null}
      </header>

      <div className="min-h-0 flex-1 overflow-auto px-4 py-3">
        {detail.isLoading ? (
          <p className="flex items-center gap-2 text-xs text-ink-muted">
            <Loader2 className="size-3.5 animate-spin" aria-hidden /> reading
            the record
          </p>
        ) : detail.error ? (
          <p className="text-xs text-danger">
            {(detail.error as Error).message}
          </p>
        ) : detail.data?.kind === "malformed" ? (
          <div className="space-y-3">
            <p className="flex items-start gap-2 text-xs text-warn-ink">
              <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
              <span>
                Offsets {detail.data.offset.toLocaleString()}–
                {detail.data.lastOffset.toLocaleString()} did not decode:{" "}
                {detail.data.reason}. The scan continued past it.
              </span>
            </p>
            <PayloadBlock label="Raw batch" payload={detail.data.raw} />
          </div>
        ) : detail.data ? (
          <Tabs defaultValue="value" className="h-full">
            <TabsList variant="line">
              <TabsTrigger value="value">Value</TabsTrigger>
              <TabsTrigger value="key">Key</TabsTrigger>
              <TabsTrigger value="headers">
                Headers
                {detail.data.headers.length
                  ? ` (${detail.data.headers.length})`
                  : ""}
              </TabsTrigger>
            </TabsList>
            <TabsContent value="value">
              {detail.data.value ? (
                <PayloadBlock payload={detail.data.value} />
              ) : (
                <p className="py-3 text-xs text-warn-ink italic">
                  Tombstone — the record has no value, which is not the same as
                  an empty one.
                </p>
              )}
            </TabsContent>
            <TabsContent value="key">
              {detail.data.key ? (
                <PayloadBlock payload={detail.data.key} />
              ) : (
                <p className="py-3 text-xs text-ink-faint">No key.</p>
              )}
            </TabsContent>
            <TabsContent value="headers">
              {detail.data.headers.length ? (
                // Ordered, duplicable, and the value is nullable — a list, not
                // a map. Collapsing to an object silently drops duplicates.
                <ol className="space-y-1 py-2 text-xs">
                  {detail.data.headers.map((header, index) => (
                    <li key={`${header.name}-${index}`} className="flex gap-2">
                      <span className="font-mono text-ink-muted">
                        {header.name}
                      </span>
                      <span className="font-mono break-all">
                        {header.value ? (
                          header.value.text
                        ) : (
                          <em className="text-ink-faint">null</em>
                        )}
                      </span>
                    </li>
                  ))}
                </ol>
              ) : (
                <p className="py-3 text-xs text-ink-faint">No headers.</p>
              )}
            </TabsContent>
          </Tabs>
        ) : null}
      </div>
    </div>
  )
}

/**
 * The record's moment, written the way the row above it was written.
 *
 * This panel showed a UTC ISO string while the list beside it showed local
 * time in the reader's own notation, so one record read as two moments. The
 * ISO form is the `title` instead — it is what gets pasted into a seek or a
 * colleague's terminal, and it is the only rendering here that carries its own
 * zone.
 */
function RecordTime({ at }: { at: number }) {
  const order = useResolvedDateOrder()
  return (
    <span title={new Date(at).toISOString()}>
      {formatTimestamp(at, displayTimeZone(), order)}
    </span>
  )
}
