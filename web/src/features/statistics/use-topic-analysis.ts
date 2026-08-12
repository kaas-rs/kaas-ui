import { useCallback, useEffect, useRef, useState } from "react"
import { useQueryClient } from "@tanstack/react-query"

import type {
  AnalysisProgress,
  ResourceError,
  TopicAnalysis,
  TopicDetail,
} from "@/api/types"
import { withBase } from "@/api/base"

import type { Phase } from "./topic-statistics"

function analysisUrl(
  envId: string,
  clusterId: string,
  topic: string,
  caps: { limit: number | null; maxMinutes: number }
): string {
  const params = new URLSearchParams()
  if (caps.limit !== null) params.set("limit", String(caps.limit))
  params.set("maxMinutes", String(caps.maxMinutes))
  // `EventSource` takes a URL, not a path handed to `fetch`, so the base
  // prefix is applied here — the same reason `streamUrl` does.
  return withBase(
    `/api/environments/${encodeURIComponent(envId)}/clusters/${encodeURIComponent(
      clusterId
    )}/topics/${encodeURIComponent(topic)}/analysis?${params}`
  )
}

/** The server's ceiling on `maxMinutes`, mirrored for the input's own check. */
export const MAX_MINUTES = 30
const DEFAULT_MINUTES = 10

export function useTopicAnalysis({
  envId,
  clusterId,
  topic,
  info,
}: {
  envId: string
  clusterId: string
  topic: string
  info: TopicDetail
}) {
  const queryClient = useQueryClient()
  const cacheKey = ["analysis", envId, clusterId, topic]
  // A revisit opens on the cached result rather than on the start button —
  // the whole reason the result is allowed into the cache.
  const [phase, setPhase] = useState<Phase>(() => {
    const cached = queryClient.getQueryData<TopicAnalysis>(cacheKey)
    return cached ? { kind: "done", result: cached } : { kind: "idle" }
  })
  // The caps, as typed. Records defaults to the topic's current retained
  // count — the same read as "everything", but a finish line the progress
  // bar can honestly reach on a topic that is still being produced to.
  // Blank means uncapped.
  const [recordCap, setRecordCap] = useState(() =>
    info.messageCount === null ? "" : String(info.messageCount)
  )
  const [minuteCap, setMinuteCap] = useState(String(DEFAULT_MINUTES))
  const source = useRef<EventSource | null>(null)

  const stop = useCallback(() => {
    source.current?.close()
    source.current = null
  }, [])

  // Leaving the tab unmounts this panel, and an analysis nobody is watching
  // is a full-topic read that should not be running.
  useEffect(() => stop, [stop])

  const start = useCallback(() => {
    stop()
    setPhase({ kind: "running", progress: null })
    const limit = Math.floor(Number(recordCap))
    const minutes = Math.floor(Number(minuteCap))
    const es = new EventSource(
      analysisUrl(envId, clusterId, topic, {
        limit: Number.isFinite(limit) && limit > 0 ? limit : null,
        maxMinutes:
          Number.isFinite(minutes) && minutes > 0
            ? Math.min(minutes, MAX_MINUTES)
            : DEFAULT_MINUTES,
      })
    )
    source.current = es

    es.addEventListener("progress", (event) => {
      const body = parse<AnalysisProgress>((event as MessageEvent).data)
      if (body) setPhase({ kind: "running", progress: body })
    })
    es.addEventListener("result", (event) => {
      const body = parse<TopicAnalysis>((event as MessageEvent).data)
      if (!body) return
      queryClient.setQueryData(cacheKey, body)
      setPhase({ kind: "done", result: body })
    })
    es.addEventListener("error", (event) => {
      // A `MessageEvent` with data is the server's own error event; a bare
      // `Event` is the connection dropping. The route sends `phase: done`
      // after every result, so a drop mid-scan without a result is a failure
      // worth saying — EventSource would silently rerun the whole scan.
      const data = (event as MessageEvent).data
      if (typeof data === "string") {
        const body = parse<ResourceError>(data)
        if (body) {
          setPhase((previous) =>
            previous.kind === "done"
              ? previous
              : { kind: "failed", error: body }
          )
        }
        return
      }
      stop()
      setPhase((previous) =>
        previous.kind === "running"
          ? {
              kind: "failed",
              error: {
                resource: topic,
                kind: "transport",
                code: null,
                codeNumber: null,
                message:
                  "the connection dropped before a result arrived; the scan was cancelled",
                retriable: true,
              },
            }
          : previous
      )
    })
    es.addEventListener("phase", (event) => {
      const body = parse<{ phase: string }>((event as MessageEvent).data)
      // `done` is deliberate — the result (or the error) is already on
      // screen, and EventSource must not reconnect and rerun the read.
      if (body?.phase === "done") stop()
    })
    // The dependency list names the identity of the analysis, not the cache
    // key array, whose reference changes per render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [envId, clusterId, topic, recordCap, minuteCap, stop, queryClient])

  return {
    phase,
    setPhase,
    start,
    stop,
    recordCap,
    setRecordCap,
    minuteCap,
    setMinuteCap,
  }
}

function parse<T>(raw: string): T | null {
  try {
    return JSON.parse(raw) as T
  } catch {
    return null
  }
}
