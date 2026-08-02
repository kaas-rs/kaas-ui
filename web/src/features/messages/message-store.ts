// The ring buffer behind the message list.
//
// Layer 2 of three, and the one that makes the whole thing possible. At ten
// thousand records a second the transport runs ten thousand times a second and
// the view renders about seven times a second. The only way to get that is for
// the transport to never touch React state: rows land in `pending`, and a
// timer publishes a new snapshot on a fixed interval whatever the rate.
//
// Two properties are load-bearing and easy to lose in a refactor:
//
//  1. `getSnapshot` returns the *same reference* between flushes, so
//     `useSyncExternalStore` is a no-op when nothing has been published. The
//     tempting `getSnapshot: () => [...items]` re-renders on every check and
//     throws away the entire design.
//  2. Nothing here imports React. It is a plain closure, which is what lets it
//     be tested and reasoned about without a renderer.

import type { StreamRow } from "@/api/types";
import type { StreamPhase } from "@/api/types";

/** How often the store publishes, in milliseconds. */
export const FLUSH_INTERVAL = 150;

/** How many rows the buffer holds before the oldest fall off the end. */
export const DEFAULT_CAP = 5000;

export interface MessageStoreState {
  /** The published rows, in display order. */
  rows: StreamRow[];
  /** How many the server dropped rather than stall the scan. */
  dropped: number;
  /** Where the stream is in its life. */
  phase: StreamPhase | null;
  /** How many rows arrived while the reader was scrolled away from the edge. */
  unseen: number;
}

export interface MessageStore {
  /** Queue rows. Called from the transport, never from React. */
  push(batch: StreamRow[]): void;
  setDropped(count: number): void;
  setPhase(phase: StreamPhase): void;
  /**
   * Whether the reader is parked at the edge new rows arrive at.
   *
   * The store needs to know because the "N new messages" count is only
   * meaningful when they are *not* — and the list must not re-render to tell
   * it, which is why this is a setter and not a prop.
   */
  setAtEdge(atEdge: boolean): void;
  /** Drop everything. A mode change is not a merge. */
  clear(): void;
  destroy(): void;
  subscribe(listener: () => void): () => void;
  getSnapshot(): MessageStoreState;
}

const EMPTY: MessageStoreState = {
  rows: [],
  dropped: 0,
  phase: null,
  unseen: 0,
};

export function createMessageStore(
  /** Which end rows arrive at. `desc` prepends, `asc` appends. */
  sort: "asc" | "desc",
  cap: number = DEFAULT_CAP,
): MessageStore {
  let rows: StreamRow[] = [];
  let pending: StreamRow[] = [];
  let dropped = 0;
  let phase: StreamPhase | null = null;
  let unseen = 0;
  let atEdge = true;
  let snapshot: MessageStoreState = EMPTY;
  let dirty = false;

  const listeners = new Set<() => void>();

  // `{partition}-{offset}` is unique per record, but a reconnect or an
  // overlapping "load more" page can deliver one twice. Two rows with the same
  // React key is a rendering error, not a cosmetic one.
  const seen = new Set<string>();

  function publish() {
    snapshot = { rows, dropped, phase, unseen };
    for (const listener of listeners) listener();
  }

  function flush() {
    if (!pending.length) {
      if (dirty) {
        dirty = false;
        publish();
      }
      return;
    }

    const batch = pending;
    pending = [];

    if (sort === "desc") {
      // Newest first: the batch arrives oldest-first within itself, so it is
      // reversed before going on the front.
      rows = [...batch.reverse(), ...rows];
      if (rows.length > cap) {
        for (const row of rows.slice(cap)) seen.delete(row.id);
        rows = rows.slice(0, cap);
      }
    } else {
      rows = [...rows, ...batch];
      if (rows.length > cap) {
        for (const row of rows.slice(0, rows.length - cap)) seen.delete(row.id);
        rows = rows.slice(rows.length - cap);
      }
    }

    // Only counted while the reader is away from the edge. Parked at the edge
    // they are already looking at them, and a pill reading "412 new messages"
    // over rows they can see is noise.
    if (!atEdge) unseen += batch.length;

    dirty = false;
    publish();
  }

  const timer = setInterval(flush, FLUSH_INTERVAL);

  return {
    push(batch) {
      for (const row of batch) {
        if (seen.has(row.id)) continue;
        seen.add(row.id);
        pending.push(row);
      }
    },
    setDropped(count) {
      if (count === dropped) return;
      dropped = count;
      dirty = true;
    },
    setPhase(next) {
      if (next === phase) return;
      phase = next;
      dirty = true;
    },
    setAtEdge(next) {
      atEdge = next;
      if (next && unseen !== 0) {
        unseen = 0;
        dirty = true;
      }
    },
    clear() {
      rows = [];
      pending = [];
      seen.clear();
      dropped = 0;
      phase = null;
      unseen = 0;
      atEdge = true;
      publish();
    },
    destroy() {
      clearInterval(timer);
      listeners.clear();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    // The same object between flushes. This is what makes
    // `useSyncExternalStore` cost nothing when nothing has changed.
    getSnapshot: () => snapshot,
  };
}
