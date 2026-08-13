import { useEffect, useState } from "react"

/**
 * How long a transaction has been open, ticking.
 *
 * **The server sends the start timestamp, never the duration.** A duration is
 * computed against a `now`, and the server's `now` is wrong by the time the
 * response is read and wronger every second the page stays open — on the one
 * column this screen is sorted by. `SnapshotAge` made the same decision for the
 * same reason; this is that decision applied to the number an operator is
 * watching to decide whether to intervene.
 *
 * Past the configured timeout it turns: a transaction Kafka should have aborted
 * and has not is precisely the one holding the last stable offset back, and
 * every `read_committed` consumer behind it is stalled until it resolves.
 */
export function OpenFor({
  startTimeMs,
  timeoutMs,
}: {
  startTimeMs: number | null
  timeoutMs: number | null
}) {
  const now = useNow(startTimeMs !== null)

  if (startTimeMs === null) {
    return (
      <span className="text-ink-faint" title="no transaction in flight">
        —
      </span>
    )
  }

  // Clamped at zero rather than rendered negative: the broker's clock and this
  // browser's are two clocks, and a few seconds of skew must not read as a
  // transaction that starts in the future.
  const openFor = Math.max(0, now - startTimeMs)
  const late = timeoutMs !== null && openFor > timeoutMs

  return (
    <span
      className={late ? "font-medium text-warn-ink" : undefined}
      title={
        late
          ? `open longer than its ${duration(timeoutMs)} timeout`
          : "since the transaction started"
      }
    >
      {duration(openFor)}
    </span>
  )
}

/**
 * The current time, once a second, and only while something needs it.
 *
 * Disabled when there is no open transaction on the page: a timer that runs to
 * recompute a dash is a wakeup a second for nothing.
 */
function useNow(enabled: boolean): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!enabled) return
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [enabled])
  return now
}

/** `4s`, `3m 12s`, `2h 05m`. Coarse on purpose past an hour. */
export function duration(ms: number): string {
  const seconds = Math.floor(ms / 1000)
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60)
    return `${minutes}m ${String(seconds % 60).padStart(2, "0")}s`
  const hours = Math.floor(minutes / 60)
  return `${hours}h ${String(minutes % 60).padStart(2, "0")}m`
}
