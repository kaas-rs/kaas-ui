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
import type { Payload, StreamRow } from "@/api/types"
import { Badge } from "@/components/ui/badge"
import { Empty, Mono, bytes } from "@/components/domain"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"

export interface MessageDetailPanelProps {
  clusterId: string
  topic: string
  /** The selected id, or nothing selected. */
  selectedId?: string
  /** The row as it was when it was selected, kept so the panel survives eviction. */
  retained?: StreamRow
  /** Whether that row is still in the buffer. */
  present: boolean
}

export function MessageDetailPanel({
  clusterId,
  topic,
  selectedId,
  retained,
  present,
}: MessageDetailPanelProps) {
  const detail = useMessageDetail(clusterId, topic, selectedId)

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
              {new Date(retained.timestamp).toISOString()}{" "}
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

function PayloadBlock({
  payload,
  label,
}: {
  payload: Payload
  label?: string
}) {
  return (
    <div className="space-y-1 py-2">
      <div className="flex items-center gap-2 text-[11px] text-ink-faint">
        {label ? <span>{label}</span> : null}
        {/* What the encoding was is said out loud: auto-detection nobody can
            see is worse than none, because the reader cannot tell text the
            producer wrote from kaas-ui's guess. */}
        <Badge variant="outline" className="h-4 px-1 text-[10px]">
          {payload.encoding}
        </Badge>
        <span className="tabular-nums">{bytes(payload.bytes)}</span>
        {payload.truncated ? (
          <span className="text-warn-ink">truncated</span>
        ) : null}
      </div>
      <pre className="max-h-full overflow-auto rounded-md border border-line bg-surface-sunken p-3 font-mono text-[11px] leading-relaxed break-all whitespace-pre-wrap">
        {payload.text}
      </pre>
    </div>
  )
}
