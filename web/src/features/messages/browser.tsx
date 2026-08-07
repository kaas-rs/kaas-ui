// The message browser.
//
// Assembles the three layers and owns the one thing that is genuinely local:
// the retained copy of the selected row, so the detail panel survives that row
// ageing out of a live buffer.
//
// Everything else is in the URL and arrives as props. The component does not
// know which route it is mounted under — it is a tab on the topic page, and
// the seek parameters belong to that page's URL, not to this file.

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { AlertTriangle, Loader2, RotateCw } from "lucide-react"
import { useDefaultLayout } from "react-resizable-panels"

import {
  fetchMessagePage,
  useOldestTimestamp,
  usePartitionBounds,
} from "@/api/client"
import type { ResolvedSeek, StreamProgress, StreamRow } from "@/api/types"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import { Sheet, SheetContent, SheetTitle } from "@/components/ui/sheet"
import { useIsMobile } from "@/hooks/use-mobile"
import { count } from "@/components/domain"
import { displayTimeZone } from "@/lib/settings"
import { cn } from "@/lib/utils"
import { downloadBuffer } from "./download"
import { MessageDetailPanel } from "./message-detail"
import { MessageList, ROW_HEIGHT } from "./message-list"
import { withIds } from "./rows"
import { SEEK_MODES, type SeekMode } from "./seek-modes"
import type { MessageSearch } from "./search"
import { Toolbar } from "./toolbar"
import { useMessageStream } from "./use-message-stream"

export interface MessageBrowserProps {
  clusterId: string
  topic: string
  /** The validated seek parameters, straight from the route. */
  search: MessageSearch
  /** Write a change back to the URL. Merging is the host's business. */
  onSearch(next: Partial<MessageSearch>): void
  /**
   * The panel's height, from whatever is hosting it. Given rather than
   * measured: the split pane and the virtualizer both need a definite height,
   * and `h-full` inside a page that scrolls is not one.
   */
  className?: string
}

export function MessageBrowser({
  clusterId,
  topic,
  search,
  onSearch,
  className,
}: MessageBrowserProps) {
  const isMobile = useIsMobile()

  const mode = search.mode
  const config = SEEK_MODES[mode]

  const stream = useMessageStream({
    clusterId,
    topic,
    mode,
    offset: search.offset,
    timestamp: search.timestamp,
    partitions: search.partitions,
    filter: search.filter,
    visibility: search.visibility,
    limit: search.limit,
  })

  const bounds = usePartitionBounds(clusterId, topic)

  // Panel sizes in localStorage, which the constraints allow explicitly.
  // Message data never goes there — a buffer of a live topic is not something
  // to persist, and restoring one would show stale rows as if they were live.
  const layout = useDefaultLayout({
    id: "messages-split",
    storage: localStorage,
  })

  // The timestamp of the oldest record the topic still holds, which is what
  // bounds the calendar. Derived from a record rather than from a retention
  // setting: `retention.ms` says when a segment *becomes* eligible for
  // deletion, not when it went, so a topic routinely holds data older than its
  // retention says and a calendar built on the setting disables days that have
  // perfectly good records behind them.
  const oldest = useOldestTimestamp(clusterId, topic)
  const retentionStart = oldest ? new Date(oldest) : undefined

  // The row as it was when it was picked. A live buffer is a moving window, so
  // by the time someone reads the payload the row itself may be gone; keeping
  // the metadata is what lets the panel keep its header instead of blanking.
  const [retained, setRetained] = useState<StreamRow | undefined>()
  const rowsById = useMemo(() => {
    const index = new Map<string, StreamRow>()
    for (const row of stream.rows) index.set(row.id, row)
    return index
  }, [stream.rows])

  const selectedId = search.selected
  useEffect(() => {
    if (!selectedId) {
      setRetained(undefined)
      return
    }
    const present = rowsById.get(selectedId)
    if (present) setRetained(present)
  }, [selectedId, rowsById])

  const onSelect = useCallback(
    (id: string) => onSearch({ selected: id }),
    [onSearch]
  )

  const onApply = useCallback(
    (next: { mode: SeekMode; offset?: number; timestamp?: number }) => {
      // Changing mode clears the buffer and the selection. The store is keyed
      // on the URL, so a new mode builds a new store; the selection has to be
      // dropped explicitly because a row id from one window means nothing in
      // another.
      onSearch({ ...next, selected: undefined })
    },
    [onSearch]
  )

  return (
    <div
      className={cn(
        "flex min-h-0 flex-col overflow-hidden rounded-md border border-line",
        className
      )}
    >
      <Toolbar
        mode={mode}
        offset={search.offset}
        timestamp={search.timestamp}
        filter={search.filter}
        partitions={search.partitions}
        visibility={search.visibility}
        bounds={bounds.data?.items ?? []}
        retentionStart={retentionStart}
        timeZone={displayTimeZone()}
        onApply={onApply}
        onFilterChange={(filter) => onSearch({ filter, selected: undefined })}
        onPartitionsChange={(partitions) =>
          onSearch({ partitions, selected: undefined })
        }
        onVisibilityChange={(visibility) =>
          onSearch({ visibility, selected: undefined })
        }
        onRestart={stream.restart}
      />

      {/* A row of its own under the controls. It was riding at the end of the
          toolbar to save height, and it read as one more control — which is
          the opposite of what a status line is for. */}
      <div className="flex shrink-0 items-center justify-end border-b border-line px-4 py-1">
        <StreamStatus
          phase={stream.phase}
          live={config.live}
          reconnecting={stream.reconnecting}
          rows={stream.rows.length}
          onDownload={() => downloadBuffer(clusterId, topic, mode, stream.rows)}
          onRestart={stream.restart}
        />
      </div>

      <Notices
        dropped={stream.dropped}
        progress={stream.progress}
        resolved={stream.resolved}
        error={stream.error}
        phase={stream.phase}
      />

      <ResizablePanelGroup
        orientation="horizontal"
        className="min-h-0 flex-1"
        {...layout}
      >
        <ResizablePanel
          id="messages-list"
          defaultSize="62"
          minSize="30"
          className="relative flex min-h-0 flex-col"
          // `react-resizable-panels` puts `overflow: auto` on this element as
          // an inline style, so a class cannot reach it. Everything inside
          // owns its own scrolling — the row list scrolls vertically and
          // clips horizontally — and a second scroller wrapped around that
          // only ever appears as a stray horizontal bar under the table.
          style={{ overflow: "hidden" }}
        >
          <MessageList
            rows={stream.rows}
            mode={mode}
            selectedId={selectedId}
            onSelect={onSelect}
            onEdgeChange={stream.setAtEdge}
            unseen={stream.unseen}
            terminal={
              stream.phase === "done" ? (
                <Terminal
                  clusterId={clusterId}
                  topic={topic}
                  rows={stream.rows}
                  mode={mode}
                  search={search}
                  onAppend={stream.append}
                />
              ) : null
            }
          />
        </ResizablePanel>

        {isMobile ? null : (
          <>
            <ResizableHandle withHandle />
            <ResizablePanel
              id="messages-detail"
              defaultSize="38"
              minSize="22"
              className="min-h-0"
              // Same as the list: the payload pane below has its own scroller.
              style={{ overflow: "hidden" }}
            >
              <MessageDetailPanel
                clusterId={clusterId}
                topic={topic}
                selectedId={selectedId}
                retained={retained}
                present={!!selectedId && rowsById.has(selectedId)}
              />
            </ResizablePanel>
          </>
        )}
      </ResizablePanelGroup>

      {isMobile ? (
        <Sheet
          open={!!selectedId}
          onOpenChange={(open) => !open && onSearch({ selected: undefined })}
        >
          <SheetContent side="bottom" className="h-[70vh] p-0">
            <SheetTitle className="sr-only">Message detail</SheetTitle>
            <MessageDetailPanel
              clusterId={clusterId}
              topic={topic}
              selectedId={selectedId}
              retained={retained}
              present={!!selectedId && rowsById.has(selectedId)}
            />
          </SheetContent>
        </Sheet>
      ) : null}
    </div>
  )
}

function StreamStatus({
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

/** Everything the stream wants to say that is not a row. */
function Notices({
  dropped,
  progress,
  resolved,
  error,
  phase,
}: {
  dropped: number
  progress: StreamProgress | null
  resolved: ResolvedSeek | null
  error: { message: string } | null
  phase: string | null
}) {
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
              This cluster resolved {new Date(resolved.timestamp).toISOString()}{" "}
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

/**
 * The end of a bounded window.
 *
 * Rendered rather than left blank: a list that simply stops looks exactly like
 * one that is still loading, and that is acceptance criterion 12.
 */
function Terminal({
  clusterId,
  topic,
  rows,
  mode,
  search,
  onAppend,
}: {
  clusterId: string
  topic: string
  rows: StreamRow[]
  mode: SeekMode
  search: {
    offset?: number
    timestamp?: number
    partitions?: string
    filter?: string
    limit?: number
  }
  onAppend(rows: StreamRow[]): void
}) {
  const [loading, setLoading] = useState(false)
  const [exhausted, setExhausted] = useState(false)
  const anchor = useRef<number | null>(null)

  const config = SEEK_MODES[mode]
  const offsets = rows.map((row) => row.offset)
  // Guarded: `Math.min()` of nothing is `Infinity`, which would go on the wire
  // as an offset and come back as a confusing 400.
  const next = offsets.length
    ? config.sort === "desc"
      ? Math.min(...offsets) - 1
      : Math.max(...offsets) + 1
    : null

  async function loadMore() {
    const from = anchor.current ?? next
    if (from === null) return
    setLoading(true)
    try {
      const params = new URLSearchParams({
        // "More" of a backward window is the next window further back; of a
        // forward one, the next window further on. Either way it is an
        // offset-anchored page, which is why `toOffset`/`fromOffset` back it
        // whatever mode is on screen.
        mode: config.sort === "desc" ? "toOffset" : "fromOffset",
        offset: String(from),
        limit: String(search.limit ?? 500),
      })
      if (search.partitions) params.set("partitions", search.partitions)
      if (search.filter) params.set("filter", search.filter)

      const page = await fetchMessagePage(clusterId, topic, params)
      if (!page.items.length) {
        setExhausted(true)
        return
      }
      onAppend(withIds(page.items))
      anchor.current = page.nextOffset
      if (!page.hasMore || page.nextOffset === null) setExhausted(true)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div
      className="flex items-center justify-center gap-3 border-t border-line text-[11px] text-ink-faint"
      style={{ height: ROW_HEIGHT }}
    >
      <span>
        End of window — {count(rows.length)} message
        {rows.length === 1 ? "" : "s"}
      </span>
      {exhausted ? (
        <span>nothing further in this direction</span>
      ) : (
        <Button
          size="sm"
          variant="outline"
          className="h-6 text-[11px]"
          onClick={() => void loadMore()}
          disabled={loading}
        >
          {loading ? "loading…" : "Load more"}
        </Button>
      )}
    </div>
  )
}
