import { useRef, useState } from "react"

import { fetchMessagePage } from "@/api/client"
import type { StreamRow } from "@/api/types"
import { Button } from "@/components/ui/button"
import { count } from "@/lib/format"
import { ROW_HEIGHT } from "./message-list"
import { withIds } from "./rows"
import { SEEK_MODES, type SeekMode } from "./seek-modes"

/**
 * The end of a bounded window.
 *
 * Rendered rather than left blank: a list that simply stops looks exactly like
 * one that is still loading, and that is acceptance criterion 12.
 */
export function Terminal({
  envId,
  clusterId,
  topic,
  rows,
  mode,
  search,
  onAppend,
}: {
  envId: string
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
    keyCodec?: string
    valueCodec?: string
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
      // The same decode and filter rules as the stream that filled the list:
      // a page appended under different ones would sit in the same table
      // rendering the same values differently.
      if (search.keyCodec) params.set("keyCodec", search.keyCodec)
      if (search.valueCodec) params.set("valueCodec", search.valueCodec)

      const page = await fetchMessagePage(envId, clusterId, topic, params)
      // An empty page is not the end. The filter runs after the decode, so a
      // window can be read in full and match nothing at all; `hasMore` is
      // read from what the server *looked at*, and it is the only thing here
      // that knows the difference between "no more records" and "no more
      // matches in these five hundred".
      if (page.items.length) onAppend(withIds(page.items))
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
        <span>no further records found in this direction</span>
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
