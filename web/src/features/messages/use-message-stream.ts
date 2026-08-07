// Layer 3's doorway: the one hook that owns a store, a transport and their
// lifetimes.
//
// The store is *not* in the TanStack Query cache and must never be. Query is
// for request/response; a stream pushed into it grows without bound and
// invalidates in ways nobody can reason about. This is PLAN.md §7's first
// "thing to get right", and it is much harder to undo than to do.

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react"

import type { ResolvedSeek, ResourceError, StreamProgress } from "@/api/types"
import { withBase } from "@/api/base"
import { SEEK_MODES, insertsAtTop, type SeekMode } from "./seek-modes"
import { createMessageStore, type MessageStoreState } from "./message-store"
import { openMessageStream } from "./transport"

export interface MessageStreamQuery {
  clusterId: string
  topic: string
  mode: SeekMode
  offset?: number
  timestamp?: number
  partitions?: string
  filter?: string
  visibility?: "all" | "committed"
  limit?: number
}

export interface MessageStreamResult extends MessageStoreState {
  progress: StreamProgress | null
  resolved: ResolvedSeek | null
  error: ResourceError | null
  /** The connection dropped and `EventSource` is retrying by itself. */
  reconnecting: boolean
  /** Tell the store whether the reader is parked where new rows arrive. */
  setAtEdge(atEdge: boolean): void
  /** Append rows fetched by "load more", which do not come over the stream. */
  append(rows: MessageStoreState["rows"]): void
  /** Throw the buffer away and reopen. */
  restart(): void
}

export function streamUrl(query: MessageStreamQuery): string {
  const params = new URLSearchParams({ mode: query.mode })
  if (query.offset !== undefined) params.set("offset", String(query.offset))
  if (query.timestamp !== undefined)
    params.set("timestamp", String(query.timestamp))
  if (query.partitions?.trim())
    params.set("partitions", query.partitions.trim())
  if (query.filter?.trim()) params.set("filter", query.filter.trim())
  if (query.visibility) params.set("visibility", query.visibility)
  if (query.limit !== undefined && !SEEK_MODES[query.mode].live) {
    params.set("limit", String(query.limit))
  }
  // `EventSource` takes a URL, not a path handed to `fetch`, so it needs the
  // prefix applied here rather than inheriting it from the client.
  return withBase(
    `/api/clusters/${encodeURIComponent(query.clusterId)}/topics/${encodeURIComponent(
      query.topic
    )}/messages/stream?${params}`
  )
}

export function useMessageStream(
  query: MessageStreamQuery
): MessageStreamResult {
  const url = streamUrl(query)
  // The same fact the list's scroll compensation reads, from the same place.
  // When these two disagree the symptom is a window rendered upside down.
  const prepends = insertsAtTop(query.mode)

  // Bumped to force a teardown and a fresh connection without changing the
  // URL, which is what the toolbar's restart does.
  const [generation, setGeneration] = useState(0)

  // These three ride outside the store because they arrive rarely and do not
  // need the flush timer's protection. Putting them in the store would mean
  // publishing a snapshot for a progress event.
  const [progress, setProgress] = useState<StreamProgress | null>(null)
  const [resolved, setResolved] = useState<ResolvedSeek | null>(null)
  const [error, setError] = useState<ResourceError | null>(null)
  const [reconnecting, setReconnecting] = useState(false)

  // One store per (url, generation). Recreating it on a mode change is the
  // point: sort order and semantics differ between modes, so merging two
  // windows is meaningless, and the spec makes clearing mandatory.
  const store = useMemo(
    () => createMessageStore(prepends),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- url and generation identify the stream; prepends is derived from it
    [url, generation]
  )
  const storeRef = useRef(store)
  storeRef.current = store

  useEffect(() => {
    setProgress(null)
    setResolved(null)
    setError(null)
    setReconnecting(false)

    const handle = openMessageStream(url, store, {
      onProgress: setProgress,
      onResolved: setResolved,
      onError: (next) => {
        setError(next)
        setReconnecting(false)
      },
      onDisconnect: () => setReconnecting(true),
      onConnected: () => setReconnecting(false),
    })

    return () => {
      handle.close()
      store.destroy()
    }
  }, [url, store])

  const state = useSyncExternalStore(
    store.subscribe,
    store.getSnapshot,
    store.getSnapshot
  )

  const setAtEdge = useCallback((atEdge: boolean) => {
    storeRef.current.setAtEdge(atEdge)
  }, [])

  const append = useCallback((rows: MessageStoreState["rows"]) => {
    storeRef.current.push(rows)
  }, [])

  const restart = useCallback(() => {
    setGeneration((value) => value + 1)
  }, [])

  return {
    ...state,
    progress,
    resolved,
    error,
    reconnecting,
    setAtEdge,
    append,
    restart,
  }
}
