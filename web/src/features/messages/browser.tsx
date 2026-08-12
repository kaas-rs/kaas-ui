// The message browser.
//
// Assembles the three layers and owns the one thing that is genuinely local:
// the retained copy of the selected row, so the detail panel survives that row
// ageing out of a live buffer.
//
// Everything else is in the URL and arrives as props. The component does not
// know which route it is mounted under — it is a tab on the topic page, and
// the seek parameters belong to that page's URL, not to this file.

import { useCallback, useEffect, useMemo, useState } from "react"
import { useDefaultLayout } from "react-resizable-panels"

import { useOldestTimestamp, usePartitionBounds } from "@/api/client"
import type { StreamRow } from "@/api/types"
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import { Sheet, SheetContent, SheetTitle } from "@/components/ui/sheet"
import { useIsMobile } from "@/hooks/use-mobile"
import { displayTimeZone } from "@/lib/settings"
import { cn } from "@/lib/utils"
import { downloadBuffer } from "./download"
import { MessageDetailPanel } from "./message-detail"
import { MessageList } from "./message-list"
import { SEEK_MODES, type SeekMode } from "./seek-modes"
import type { MessageSearch } from "./search"
import { StreamNotices } from "./stream-notices"
import { StreamStatus } from "./stream-status"
import { Terminal } from "./terminal"
import { Toolbar } from "./toolbar"
import { useMessageStream } from "./use-message-stream"

export interface MessageBrowserProps {
  envId: string
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
  envId,
  clusterId,
  topic,
  search,
  onSearch,
  className,
}: MessageBrowserProps) {
  const isMobile = useIsMobile()

  // Read once per render, here, and handed to both the picker that reads times
  // in it and the list that writes them in it. Reading it is a formatter
  // construction, so it is not a thing to do per row.
  const timeZone = displayTimeZone()

  const mode = search.mode
  const config = SEEK_MODES[mode]

  const stream = useMessageStream({
    envId,
    clusterId,
    topic,
    mode,
    offset: search.offset,
    timestamp: search.timestamp,
    partitions: search.partitions,
    filter: search.filter,
    visibility: search.visibility,
    limit: search.limit,
    keyCodec: search.keyCodec,
    valueCodec: search.valueCodec,
  })

  const bounds = usePartitionBounds(envId, clusterId, topic)

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
  const oldest = useOldestTimestamp(envId, clusterId, topic)
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
        timeZone={timeZone}
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

      <StreamNotices
        timeZone={timeZone}
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
            timeZone={timeZone}
            terminal={
              stream.phase === "done" ? (
                <Terminal
                  envId={envId}
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
                envId={envId}
                clusterId={clusterId}
                topic={topic}
                selectedId={selectedId}
                retained={retained}
                present={!!selectedId && rowsById.has(selectedId)}
                keyCodec={search.keyCodec}
                valueCodec={search.valueCodec}
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
              envId={envId}
              clusterId={clusterId}
              topic={topic}
              selectedId={selectedId}
              retained={retained}
              present={!!selectedId && rowsById.has(selectedId)}
              keyCodec={search.keyCodec}
              valueCodec={search.valueCodec}
            />
          </SheetContent>
        </Sheet>
      ) : null}
    </div>
  )
}
