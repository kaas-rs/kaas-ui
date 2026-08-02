// The message browser page.
//
// Assembles the three layers and owns the things that are genuinely page
// state: which row is selected, and the retained copy of that row so the
// detail panel survives the row ageing out of a live buffer.
//
// Everything else lives in the URL. A view seeked to a timestamp, filtered,
// with a message selected is the artifact people send each other from a Kafka
// UI, and it has to reproduce exactly on load.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { AlertTriangle, ChevronLeft, Loader2 } from "lucide-react";

import { fetchMessagePage, useOldestTimestamp, usePartitionBounds } from "@/api/client";
import type { ResolvedSeek, StreamProgress, StreamRow } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Progress } from "@/components/ui/progress";
import { useDefaultLayout } from "react-resizable-panels";

import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { Sheet, SheetContent, SheetTitle } from "@/components/ui/sheet";
import { useIsMobile } from "@/hooks/use-mobile";
import { count } from "@/components/domain";
import { MessageDetailPanel } from "@/features/messages/message-detail";
import { MessageList, ROW_HEIGHT } from "@/features/messages/message-list";
import { withIds } from "@/features/messages/rows";
import { SEEK_MODES, type SeekMode } from "@/features/messages/seek-modes";
import { Toolbar } from "@/features/messages/toolbar";
import { useMessageStream } from "@/features/messages/use-message-stream";

/**
 * The zone times are shown in.
 *
 * One place, so the picker and the list agree. Today it is the browser's; when
 * the app header grows a zone selector this reads from it, and nothing else
 * changes because the picker already converts to an absolute instant before
 * the value leaves it.
 */
function displayTimeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone;
}

export function Messages({ clusterId, topic }: { clusterId: string; topic: string }) {
  const search = useSearch({ from: "/clusters/$clusterId/topics/$topic/messages" });
  const navigate = useNavigate();
  const isMobile = useIsMobile();

  const mode = search.mode;
  const config = SEEK_MODES[mode];

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
  });

  const bounds = usePartitionBounds(clusterId, topic);

  // Panel sizes in localStorage, which the constraints allow explicitly.
  // Message data never goes there — a buffer of a live topic is not something
  // to persist, and restoring one would show stale rows as if they were live.
  const layout = useDefaultLayout({ id: "messages-split", storage: localStorage });

  // The timestamp of the oldest record the topic still holds, which is what
  // bounds the calendar. Derived from a record rather than from a retention
  // setting: `retention.ms` says when a segment *becomes* eligible for
  // deletion, not when it went, so a topic routinely holds data older than its
  // retention says and a calendar built on the setting disables days that have
  // perfectly good records behind them.
  const oldest = useOldestTimestamp(clusterId, topic);
  const retentionStart = oldest ? new Date(oldest) : undefined;

  // The row as it was when it was picked. A live buffer is a moving window, so
  // by the time someone reads the payload the row itself may be gone; keeping
  // the metadata is what lets the panel keep its header instead of blanking.
  const [retained, setRetained] = useState<StreamRow | undefined>();
  const rowsById = useMemo(() => {
    const index = new Map<string, StreamRow>();
    for (const row of stream.rows) index.set(row.id, row);
    return index;
  }, [stream.rows]);

  const selectedId = search.selected;
  useEffect(() => {
    if (!selectedId) {
      setRetained(undefined);
      return;
    }
    const present = rowsById.get(selectedId);
    if (present) setRetained(present);
  }, [selectedId, rowsById]);

  const setSearch = useCallback(
    (next: Partial<typeof search>) => {
      void navigate({
        to: "/clusters/$clusterId/topics/$topic/messages",
        params: { clusterId, topic },
        // The router types `previous` as the *pre-validation* shape, where
        // `mode` is optional; at runtime the schema has already defaulted it.
        // Falling back to the mode on screen keeps the reducer total without
        // inventing a different window.
        search: (previous) => ({
          ...previous,
          ...next,
          mode: next.mode ?? previous.mode ?? mode,
          visibility: next.visibility ?? previous.visibility ?? "all",
        }),
        replace: true,
      });
    },
    [navigate, clusterId, topic, mode],
  );

  const onSelect = useCallback((id: string) => setSearch({ selected: id }), [setSearch]);

  const onApply = useCallback(
    (next: { mode: SeekMode; offset?: number; timestamp?: number }) => {
      // Changing mode clears the buffer and the selection. The store is keyed
      // on the URL, so a new mode builds a new store; the selection has to be
      // dropped explicitly because a row id from one window means nothing in
      // another.
      setSearch({ ...next, selected: undefined });
    },
    [setSearch],
  );

  return (
    <div className="flex h-[calc(100vh-var(--header-height,3rem))] min-h-0 flex-col">
      <header className="flex shrink-0 items-center gap-2 border-b border-line px-4 py-2">
        <Button asChild variant="ghost" size="sm" className="-ml-2">
          <Link to="/clusters/$clusterId/topics/$topic" params={{ clusterId, topic }}>
            <ChevronLeft className="size-4" aria-hidden />
            {topic}
          </Link>
        </Button>
        <h1 className="text-sm font-medium">Messages</h1>
        <StreamStatus
          phase={stream.phase}
          live={config.live}
          reconnecting={stream.reconnecting}
          rows={stream.rows.length}
        />
      </header>

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
        onFilterChange={(filter) => setSearch({ filter, selected: undefined })}
        onPartitionsChange={(partitions) => setSearch({ partitions, selected: undefined })}
        onVisibilityChange={(visibility) => setSearch({ visibility, selected: undefined })}
        onRestart={stream.restart}
      />

      <Notices
        dropped={stream.dropped}
        progress={stream.progress}
        resolved={stream.resolved}
        error={stream.error}
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
            <ResizablePanel id="messages-detail" defaultSize="38" minSize="22" className="min-h-0">
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
        <Sheet open={!!selectedId} onOpenChange={(open) => !open && setSearch({ selected: undefined })}>
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
  );
}

function StreamStatus({
  phase,
  live,
  reconnecting,
  rows,
}: {
  phase: string | null;
  live: boolean;
  reconnecting: boolean;
  rows: number;
}) {
  return (
    <div className="ml-auto flex items-center gap-2 text-[11px] text-ink-muted">
      <span className="tabular-nums">{count(rows)} buffered</span>
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
      ) : phase === "done" ? (
        <Badge variant="outline">window read</Badge>
      ) : null}
    </div>
  );
}

/** Everything the stream wants to say that is not a row. */
function Notices({
  dropped,
  progress,
  resolved,
  error,
}: {
  dropped: number;
  progress: StreamProgress | null;
  resolved: ResolvedSeek | null;
  error: { message: string } | null;
}) {
  const showBar = progress?.fraction !== null && progress?.fraction !== undefined;
  const noticeCount =
    (dropped > 0 ? 1 : 0) +
    (error ? 1 : 0) +
    (resolved?.unresolved ? 1 : 0) +
    (progress?.orderingDegraded ? 1 : 0);
  if (!noticeCount && !showBar) return null;

  return (
    <div className="shrink-0 border-b border-line">
      {showBar ? (
        <Progress value={(progress?.fraction ?? 0) * 100} className="h-0.5 rounded-none" />
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
            {count(dropped)} message(s) were dropped to keep the stream ahead of this browser.
          </p>
        ) : null}
        {resolved?.unresolved ? (
          <p className="flex items-start gap-2 text-[11px] text-warn-ink">
            <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
            <span>
              This cluster resolved {new Date(resolved.timestamp).toISOString()} to no offset on
              any of its {resolved.partitions.length} partitions, so the window is empty. Brokers
              that keep no timestamp index answer a time seek this way; seeking by offset still
              works.
            </span>
          </p>
        ) : null}
        {progress?.orderingDegraded ? (
          <p className="text-[11px] text-ink-muted">
            Approximately ordered across partitions — records may be up to{" "}
            {count(progress.reorderWindow)} apart. Within a partition the order is exact.
          </p>
        ) : null}
      </div>
    </div>
  );
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
  clusterId: string;
  topic: string;
  rows: StreamRow[];
  mode: SeekMode;
  search: { offset?: number; timestamp?: number; partitions?: string; filter?: string; limit?: number };
  onAppend(rows: StreamRow[]): void;
}) {
  const [loading, setLoading] = useState(false);
  const [exhausted, setExhausted] = useState(false);
  const anchor = useRef<number | null>(null);

  const config = SEEK_MODES[mode];
  const offsets = rows.map((row) => row.offset);
  // Guarded: `Math.min()` of nothing is `Infinity`, which would go on the wire
  // as an offset and come back as a confusing 400.
  const next = offsets.length
    ? config.sort === "desc"
      ? Math.min(...offsets) - 1
      : Math.max(...offsets) + 1
    : null;

  async function loadMore() {
    const from = anchor.current ?? next;
    if (from === null) return;
    setLoading(true);
    try {
      const params = new URLSearchParams({
        // "More" of a backward window is the next window further back; of a
        // forward one, the next window further on. Either way it is an
        // offset-anchored page, which is why `toOffset`/`fromOffset` back it
        // whatever mode is on screen.
        mode: config.sort === "desc" ? "toOffset" : "fromOffset",
        offset: String(from),
        limit: String(search.limit ?? 500),
      });
      if (search.partitions) params.set("partitions", search.partitions);
      if (search.filter) params.set("filter", search.filter);

      const page = await fetchMessagePage(clusterId, topic, params);
      if (!page.items.length) {
        setExhausted(true);
        return;
      }
      onAppend(withIds(page.items));
      anchor.current = page.nextOffset;
      if (!page.hasMore || page.nextOffset === null) setExhausted(true);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div
      className="flex items-center justify-center gap-3 border-t border-line text-[11px] text-ink-faint"
      style={{ height: ROW_HEIGHT }}
    >
      <span>
        End of window — {count(rows.length)} message{rows.length === 1 ? "" : "s"}
      </span>
      {exhausted ? (
        <span>nothing further in this direction</span>
      ) : (
        <Button size="sm" variant="outline" className="h-6 text-[11px]" onClick={() => void loadMore()} disabled={loading}>
          {loading ? "loading…" : "Load more"}
        </Button>
      )}
    </div>
  );
}
