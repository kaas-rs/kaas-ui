// Layer 1 of three: the wire.
//
// Parses SSE and calls into the store. Contains no React, deliberately and
// permanently — `setState` from an `onmessage` handler is the one change that
// would undo the whole design, and the easiest way not to write it is for this
// file to have no way to.

import type {
  ResolvedSeek,
  ResourceError,
  StreamPhase,
  StreamProgress,
  StreamRowData,
} from "@/api/types"
import type { MessageStore } from "./message-store"
import { withIds } from "./rows"

export interface StreamHandlers {
  onProgress(progress: StreamProgress): void
  onResolved(resolved: ResolvedSeek): void
  onError(error: ResourceError): void
  /** The connection itself failed, as opposed to the cluster answering badly. */
  onDisconnect(): void
  /**
   * The server is talking, so whatever was wrong is not wrong now.
   *
   * Called from the phase events rather than from every batch: a reconnect
   * always replays `seeking` and `streaming`, phases are rare, and the rule
   * that record traffic never reaches React state stays intact.
   */
  onConnected(): void
}

export interface StreamHandle {
  close(): void
}

/**
 * Open a message stream and feed a store from it.
 *
 * `EventSource` reconnects on its own and replays `Last-Event-ID`, which the
 * server honours for a single-partition stream and deliberately ignores for a
 * wider one — one id cannot restore a cursor per partition, and resuming some
 * partitions while restarting others would lose records silently. See
 * `resume_floor` in `stream.rs`.
 *
 * That reconnection is for a connection that *failed*. A stream the server
 * ended on purpose says so with `phase: done`, and is closed below.
 */
export function openMessageStream(
  url: string,
  store: MessageStore,
  handlers: StreamHandlers
): StreamHandle {
  const source = new EventSource(url)

  source.addEventListener("messages", (event) => {
    const rows = parse<StreamRowData[]>(event.data)
    // The id is attached here and nowhere else — see `rows.ts`.
    if (rows) store.push(withIds(rows))
  })

  source.addEventListener("phase", (event) => {
    const body = parse<{ phase: StreamPhase }>(event.data)
    if (!body) return
    handlers.onConnected()
    store.setPhase(body.phase)

    // `done` ends the stream, and ending it here is the whole point:
    // `EventSource` reconnects whenever the response completes, and it cannot
    // tell "the server finished" from "the network dropped". Left alone, a
    // bounded window reconnects the instant it is read, runs again, and — on a
    // topic anyone is producing to — comes back with records that were not in
    // the first window. They arrive as a second batch, so they land at the end
    // of the list rather than in their own order, the badge says
    // "reconnecting", and a snapshot behaves like a bad live stream.
    //
    // The server only sends `done` deliberately: the window is read, or the
    // process is shutting down. Neither is a case for retrying.
    if (body.phase === "done") source.close()
  })

  source.addEventListener("dropped", (event) => {
    const body = parse<{ count: number }>(event.data)
    if (body) store.setDropped(body.count)
  })

  source.addEventListener("progress", (event) => {
    const body = parse<StreamProgress>(event.data)
    if (body) handlers.onProgress(body)
  })

  source.addEventListener("resolved", (event) => {
    const body = parse<ResolvedSeek>(event.data)
    if (body) handlers.onResolved(body)
  })

  source.addEventListener("error", (event) => {
    // Two unrelated things arrive on this name. A `MessageEvent` with data is
    // the server's own `error` event — a cluster answered badly, and the
    // payload says how. A bare `Event` is `EventSource` reporting that the
    // *connection* dropped, which it will retry by itself.
    const data = (event as MessageEvent).data
    if (typeof data === "string") {
      const body = parse<ResourceError>(data)
      if (body) handlers.onError(body)
      return
    }
    handlers.onDisconnect()
  })

  return {
    close() {
      // Dropping the response is the whole cancellation story: the server's
      // pump selects on the reader going away and drops the scan with it.
      source.close()
    },
  }
}

function parse<T>(raw: string): T | null {
  try {
    return JSON.parse(raw) as T
  } catch {
    // A payload this end cannot read is worth ignoring rather than tearing the
    // stream down: the next event is very likely fine, and a debugging tool
    // that dies on one bad frame is the wrong trade.
    return null
  }
}
