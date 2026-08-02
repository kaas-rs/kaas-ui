// Layer 3: the view.
//
// Absolute positioning rules out `<table>`, so this is an ARIA grid. Header
// and rows share one `grid-template-columns` through a CSS custom property,
// which is the only way the columns stay aligned when one of them is
// positioned and the other is not.
//
// Row height is **fixed**. That is what makes the scroll compensation in
// `useScrollCompensation` exact rather than approximate: with variable heights
// the correction has to measure, measuring happens after layout, and the list
// visibly jumps. If a requirement ever seems to need variable rows, raise it
// rather than adding `measureElement`.

import { useCallback, useEffect, useLayoutEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { AlertTriangle } from "lucide-react";

import type { StreamRow } from "@/api/types";
import { cn } from "@/lib/utils";
import { insertsAtTop, type SeekMode } from "./seek-modes";

/** Declared once, shared by the CSS and the virtualizer. */
export const ROW_HEIGHT = 36;

/**
 * The five columns, shared by the header and every row.
 *
 * The last two are `minmax(0, …)` rather than `minmax(110px, …)`: a grid
 * column will not shrink below its floor, so a floor is also a *minimum width
 * for the whole table*. The header row is in normal flow and carries that
 * minimum up to the panel, which is how a floor here becomes a horizontal
 * scrollbar there. Zero lets the two text columns shrink, and `truncate` is
 * what they do about it — which is what they were always meant to do.
 *
 * The three fixed ones are what their content actually needs: thirteen digits
 * of offset, three of partition, and `14:35:12.553`.
 */
const COLUMNS = "96px 56px 150px minmax(0, 1fr) minmax(0, 2.2fr)";

export interface MessageListProps {
  rows: StreamRow[];
  mode: SeekMode;
  selectedId?: string;
  onSelect(id: string): void;
  /** Told whether the reader is parked where new rows arrive. */
  onEdgeChange(atEdge: boolean): void;
  /** How many rows arrived while they were scrolled away from that edge. */
  unseen: number;
  /** Rendered after the last row when the window is exhausted. */
  terminal?: React.ReactNode;
}

export function MessageList({
  rows,
  mode,
  selectedId,
  onSelect,
  onEdgeChange,
  unseen,
  terminal,
}: MessageListProps) {
  const scrollRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    // Never the array index. An index key on a list that prepends re-associates
    // every row with different data on every flush, which is both wrong and the
    // most expensive thing React can be asked to do.
    getItemKey: (index) => rows[index]?.id ?? index,
    overscan: 8,
  });

  useScrollCompensation(scrollRef, rows.length, mode);
  useEdgeReporting(scrollRef, mode, onEdgeChange);
  useKeyboardSelection(scrollRef, rows, selectedId, onSelect, virtualizer.scrollToIndex);

  const items = virtualizer.getVirtualItems();

  return (
    <div
      role="grid"
      aria-rowcount={rows.length + 1}
      aria-label="Messages"
      // `min-w-0`, or the header row's intrinsic width becomes the floor for
      // everything above it.
      className="flex min-h-0 min-w-0 flex-1 flex-col"
      style={{ ["--message-columns" as string]: COLUMNS }}
    >
      <div
        role="row"
        aria-rowindex={1}
        className="grid shrink-0 items-center gap-3 border-b border-line px-4 py-1.5 text-[11px] tracking-[0.05em] text-ink-faint uppercase"
        style={{ gridTemplateColumns: "var(--message-columns)" }}
      >
        <span role="columnheader">Offset</span>
        <span role="columnheader">Part</span>
        <span role="columnheader">Timestamp</span>
        <span role="columnheader">Key</span>
        <span role="columnheader">Value</span>
      </div>

      {/* Inside the list because the list owns the scroller. Reaching for it
          from a sibling means finding it by class name, which breaks the first
          time someone restyles the container. */}
      <NewMessagesPill
        count={unseen}
        mode={mode}
        onJump={() => {
          const element = scrollRef.current;
          if (element) element.scrollTop = 0;
        }}
      />

      <div
        ref={scrollRef}
        tabIndex={0}
        className="relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
      >
        <div
          style={{ height: virtualizer.getTotalSize(), position: "relative", width: "100%" }}
        >
          {items.map((item) => {
            const row = rows[item.index];
            if (!row) return null;
            return (
              <Row
                key={item.key}
                row={row}
                rowIndex={item.index + 2}
                top={item.start}
                selected={row.id === selectedId}
                onSelect={onSelect}
              />
            );
          })}
        </div>
        {terminal}
      </div>
    </div>
  );
}

/**
 * "N new messages", when the reader is scrolled away from the edge.
 *
 * Informational only: the view has already been held still by the scroll
 * compensation, so this says how much went past rather than offering to stop
 * something moving.
 */
function NewMessagesPill({
  count,
  mode,
  onJump,
}: {
  count: number;
  mode: SeekMode;
  onJump(): void;
}) {
  if (count <= 0 || !insertsAtTop(mode)) return null;
  return (
    <button
      type="button"
      onClick={onJump}
      className="absolute top-10 left-1/2 z-10 -translate-x-1/2 rounded-full bg-rust px-3 py-1 text-xs font-medium text-rust-ink shadow-md"
    >
      {count.toLocaleString()} new message{count === 1 ? "" : "s"}
    </button>
  );
}

function Row({
  row,
  rowIndex,
  top,
  selected,
  onSelect,
}: {
  row: StreamRow;
  rowIndex: number;
  top: number;
  selected: boolean;
  onSelect(id: string): void;
}) {
  const common = {
    role: "row" as const,
    "aria-rowindex": rowIndex,
    "aria-selected": selected,
    onClick: () => onSelect(row.id),
    style: {
      position: "absolute" as const,
      top: 0,
      left: 0,
      width: "100%",
      height: ROW_HEIGHT,
      transform: `translateY(${top}px)`,
    },
  };

  if (row.kind === "malformed") {
    // The same height and the same grid, with the cells replaced by one
    // spanning cell. A warning rather than an error: the scan continued past
    // it, and the topic is fine either side.
    return (
      <div
        {...common}
        className={cn(
          "flex cursor-pointer items-center gap-2 border-b border-line/60 bg-warn-soft/50 px-4 text-xs text-warn-ink",
          selected && "bg-rust/25",
        )}
      >
        <AlertTriangle className="size-3.5 shrink-0" aria-hidden />
        <span role="gridcell" className="truncate">
          offsets {row.offset.toLocaleString()}–{row.lastOffset.toLocaleString()} did not decode
          {" — "}
          {row.reason}
        </span>
      </div>
    );
  }

  return (
    <div
      {...common}
      className={cn(
        "grid cursor-pointer items-center gap-3 border-b border-line/60 px-4 text-xs hover:bg-surface-raised",
        selected && "bg-rust/25",
      )}
      style={{ ...common.style, gridTemplateColumns: "var(--message-columns)" }}
    >
      <span role="gridcell" className="tabular-nums">
        {row.offset.toLocaleString()}
      </span>
      <span role="gridcell" className="tabular-nums text-ink-muted">
        {row.partition}
      </span>
      <span role="gridcell" className="font-mono text-[11px] text-ink-muted">
        {formatTimestamp(row.timestamp)}
      </span>
      <span role="gridcell" className="truncate font-mono text-[11px] text-ink-muted">
        {row.key ? row.key.text : <span className="text-ink-faint">—</span>}
      </span>
      <span role="gridcell" className="truncate font-mono text-[11px] text-ink-muted">
        {row.value === null ? (
          // A tombstone is not an empty value, and compaction turns on the
          // difference. Rendering both as blank loses the only thing worth
          // knowing about that record.
          <span className="text-warn-ink italic">tombstone</span>
        ) : (
          row.value.text
        )}
      </span>
    </div>
  );
}

function formatTimestamp(ms: number): string {
  const date = new Date(ms);
  return `${date.toLocaleTimeString("en-GB")}.${String(date.getMilliseconds()).padStart(3, "0")}`;
}

/**
 * Hold the viewport still while rows are inserted above it.
 *
 * Gated on **insertion direction**, never on a mode name. Only a live,
 * newest-first stream prepends; every other mode appends at the bottom or
 * fills once and stops, and applying this correction there visibly drags the
 * list downward as rows arrive. That is acceptance criterion 11.
 *
 * Because the row height is fixed the correction is exact, and running it in a
 * layout effect means it happens before paint — so the row under the cursor
 * does not move at all rather than moving and moving back.
 */
function useScrollCompensation(
  scrollRef: React.RefObject<HTMLDivElement | null>,
  count: number,
  mode: SeekMode,
) {
  const previous = useRef(count);
  const prepends = insertsAtTop(mode);

  useLayoutEffect(() => {
    const element = scrollRef.current;
    const added = count - previous.current;
    previous.current = count;
    if (!element || !prepends || added <= 0) return;
    // Parked at the top, the reader wants to follow the stream, so the
    // viewport should stay where it is and let new rows push in.
    if (element.scrollTop > 8) element.scrollTop += added * ROW_HEIGHT;
  }, [count, prepends, scrollRef]);
}

/** Report whether the reader is parked at the end rows arrive at. */
function useEdgeReporting(
  scrollRef: React.RefObject<HTMLDivElement | null>,
  mode: SeekMode,
  onEdgeChange: (atEdge: boolean) => void,
) {
  const prepends = insertsAtTop(mode);

  const check = useCallback(() => {
    const element = scrollRef.current;
    if (!element) return;
    const atEdge = prepends
      ? element.scrollTop <= 8
      : element.scrollHeight - element.scrollTop - element.clientHeight <= 8;
    onEdgeChange(atEdge);
  }, [onEdgeChange, prepends, scrollRef]);

  useEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    check();
    element.addEventListener("scroll", check, { passive: true });
    return () => element.removeEventListener("scroll", check);
  }, [check, scrollRef]);
}

/** `j`/`k` and the arrows move the selection; the list keeps focus. */
function useKeyboardSelection(
  scrollRef: React.RefObject<HTMLDivElement | null>,
  rows: StreamRow[],
  selectedId: string | undefined,
  onSelect: (id: string) => void,
  scrollToIndex: (index: number, options?: { align?: "auto" | "start" | "center" | "end" }) => void,
) {
  useEffect(() => {
    const element = scrollRef.current;
    if (!element) return;

    function onKeyDown(event: KeyboardEvent) {
      let delta = 0;
      if (event.key === "j" || event.key === "ArrowDown") delta = 1;
      else if (event.key === "k" || event.key === "ArrowUp") delta = -1;
      else return;

      event.preventDefault();
      if (!rows.length) return;
      const current = selectedId ? rows.findIndex((row) => row.id === selectedId) : -1;
      const next = Math.min(rows.length - 1, Math.max(0, current + delta));
      const row = rows[next];
      if (!row) return;
      onSelect(row.id);
      scrollToIndex(next, { align: "auto" });
    }

    element.addEventListener("keydown", onKeyDown);
    return () => element.removeEventListener("keydown", onKeyDown);
  }, [rows, selectedId, onSelect, scrollToIndex, scrollRef]);
}
