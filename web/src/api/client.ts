import { useQuery } from "@tanstack/react-query"
import type {
  Capabilities,
  ClusterCard,
  ClusterDetail,
  ConfigResourceEntry,
  Envelope,
  EnvironmentSection,
  GroupDetail,
  GroupOffset,
  GroupSummary,
  Identity,
  LogDir,
  MessageDetail,
  MessagePage,
  PartitionOffsets,
  TopicDetail,
  TopicSummary,
} from "./types"

import { parseRowId } from "@/features/messages/rows"
import { withBase } from "./base"

/** A request that produced no answer at all. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly kind?: string
  ) {
    super(message)
    this.name = "ApiError"
  }
}

async function get<T>(path: string): Promise<T> {
  const response = await fetch(withBase(path), {
    headers: { accept: "application/json" },
  })
  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`
    let kind: string | undefined
    try {
      const body = await response.json()
      if (typeof body?.message === "string") message = body.message
      if (typeof body?.kind === "string") kind = body.kind
    } catch {
      // A non-JSON error body is still an error; the status line stands.
    }
    throw new ApiError(response.status, message, kind)
  }
  return (await response.json()) as T
}

const encode = encodeURIComponent

/** How often a view that is backed by a metadata snapshot re-asks. */
const SNAPSHOT_REFRESH = 10_000

export function useClusters() {
  return useQuery({
    queryKey: ["clusters"],
    queryFn: () => get<Envelope<ClusterCard>>("/api/clusters"),
    refetchInterval: 5_000,
  })
}

/**
 * The fleet, sectioned by environment.
 *
 * The same cards `useClusters` returns, plus whatever else is configured in
 * each environment, in an order the server chose — declared environments come
 * in declared order, which no client can recover from the ids. The sidebar
 * keeps using the flat list: it is a switcher, not a map.
 */
export function useFleet() {
  return useQuery({
    queryKey: ["fleet"],
    queryFn: () => get<Envelope<EnvironmentSection>>("/api/fleet"),
    refetchInterval: 5_000,
  })
}

/**
 * Who the caller is.
 *
 * Not what they may do — that rides on each cluster's card as `grants`, so
 * there is one source for it and no second copy to go stale. This is the
 * header's business: who am I, and does this deployment even have a notion of
 * signing in.
 */
export function useIdentity() {
  return useQuery({
    queryKey: ["me"],
    queryFn: () => get<Identity>("/api/me"),
    staleTime: 5 * 60_000,
  })
}

export function useCluster(id: string) {
  return useQuery({
    queryKey: ["cluster", id],
    queryFn: () => get<Envelope<ClusterDetail>>(`/api/clusters/${encode(id)}`),
    refetchInterval: SNAPSHOT_REFRESH,
  })
}

export function useCapabilities(id: string) {
  return useQuery({
    queryKey: ["capabilities", id],
    queryFn: () =>
      get<Capabilities>(`/api/clusters/${encode(id)}/capabilities`),
    // Not cached forever: a rolling upgrade changes the answer, and the whole
    // point of the endpoint is that it is allowed to.
    staleTime: 30_000,
    retry: false,
  })
}

export function useLogDirs(id: string, node: number | null) {
  return useQuery({
    queryKey: ["log-dirs", id, node],
    queryFn: () =>
      get<Envelope<LogDir>>(
        `/api/clusters/${encode(id)}/brokers/${node}/log-dirs`
      ),
    enabled: node !== null,
  })
}

export interface TopicListQuery {
  search?: string
  internal?: boolean
  sort?: string
  order?: "asc" | "desc"
  limit?: number
  offset?: number
  sizes?: boolean
}

export function useTopics(id: string, query: TopicListQuery) {
  const params = new URLSearchParams()
  // Filtering and sorting are server-side: a five-thousand-topic cluster is a
  // real number, and shipping all of it so the browser can hide most of it is
  // how a UI becomes unusable on exactly the cluster that needed one.
  if (query.search) params.set("search", query.search)
  if (query.internal) params.set("internal", "true")
  if (query.sort) params.set("sort", query.sort)
  if (query.order) params.set("order", query.order)
  if (query.limit !== undefined) params.set("limit", String(query.limit))
  if (query.offset) params.set("offset", String(query.offset))
  if (query.sizes) params.set("sizes", "true")

  return useQuery({
    queryKey: ["topics", id, params.toString()],
    queryFn: () =>
      get<Envelope<TopicSummary>>(
        `/api/clusters/${encode(id)}/topics?${params}`
      ),
    refetchInterval: SNAPSHOT_REFRESH,
  })
}

export function useTopic(id: string, topic: string) {
  return useQuery({
    queryKey: ["topic", id, topic],
    queryFn: () =>
      get<Envelope<TopicDetail>>(
        `/api/clusters/${encode(id)}/topics/${encode(topic)}`
      ),
    refetchInterval: SNAPSHOT_REFRESH,
  })
}

export function useTopicConfigs(id: string, topic: string) {
  return useQuery({
    queryKey: ["topic-configs", id, topic],
    queryFn: () =>
      get<Envelope<ConfigResourceEntry>>(
        `/api/clusters/${encode(id)}/topics/${encode(topic)}/configs`
      ),
  })
}

export function useClusterConfigs(id: string, resource: string | null) {
  const query = resource ? `?resource=${encode(resource)}` : ""
  return useQuery({
    queryKey: ["cluster-configs", id, resource],
    queryFn: () =>
      get<Envelope<ConfigResourceEntry>>(
        `/api/clusters/${encode(id)}/configs${query}`
      ),
  })
}

export function useGroups(id: string) {
  return useQuery({
    queryKey: ["groups", id],
    queryFn: () =>
      get<Envelope<GroupSummary>>(`/api/clusters/${encode(id)}/groups`),
    refetchInterval: SNAPSHOT_REFRESH,
  })
}

export function useGroup(id: string, group: string) {
  return useQuery({
    queryKey: ["group", id, group],
    queryFn: () =>
      get<Envelope<GroupDetail>>(
        `/api/clusters/${encode(id)}/groups/${encode(group)}`
      ),
  })
}

export function useGroupOffsets(id: string, group: string) {
  return useQuery({
    queryKey: ["group-offsets", id, group],
    queryFn: () =>
      get<Envelope<GroupOffset>>(
        `/api/clusters/${encode(id)}/groups/${encode(group)}/offsets`
      ),
    refetchInterval: SNAPSHOT_REFRESH,
  })
}

/**
 * One record's full payload, fetched when a row is selected and never before.
 *
 * `staleTime: Infinity` is not a cache-tuning choice: a Kafka record at a
 * given offset is immutable, so re-selecting a row must cost no request at
 * all. The query key is the same `{partition}-{offset}` the list keys on.
 */
export function useMessageDetail(
  id: string,
  topic: string,
  rowId: string | undefined
) {
  const parsed = rowId ? parseRowId(rowId) : null
  return useQuery({
    queryKey: ["message", id, topic, rowId],
    queryFn: () =>
      get<MessageDetail>(
        `/api/clusters/${encode(id)}/topics/${encode(topic)}/messages/${parsed?.partition}/${parsed?.offset}`
      ),
    enabled: !!parsed,
    staleTime: Infinity,
    retry: false,
  })
}

/**
 * One page of a window, for "load more".
 *
 * Deliberately not a stream: a bounded page is request/response, and opening
 * an SSE connection to deliver 500 rows that are already known to exist would
 * be heavier and no more correct.
 */
export async function fetchMessagePage(
  id: string,
  topic: string,
  params: URLSearchParams
): Promise<MessagePage> {
  return get<MessagePage>(
    `/api/clusters/${encode(id)}/topics/${encode(topic)}/messages?${params}`
  )
}

/**
 * Both ends of every partition.
 *
 * Feeds the offset input's bounds and the calendar's disabled days. Partitions
 * that failed ride in `errors` rather than failing the request, so a partition
 * mid-election leaves the control usable and unclamped instead of blocking it.
 */
export function usePartitionBounds(id: string, topic: string) {
  return useQuery({
    queryKey: ["offsets", id, topic],
    queryFn: () =>
      get<Envelope<PartitionOffsets>>(
        `/api/clusters/${encode(id)}/topics/${encode(topic)}/offsets`
      ),
    staleTime: SNAPSHOT_REFRESH,
  })
}

/**
 * The timestamp of the oldest record the topic still holds.
 *
 * What actually bounds the calendar. Derived from a record rather than from
 * `retention.ms`: that setting says when a segment becomes *eligible* for
 * deletion, not when it went, so a topic routinely holds data older than its
 * retention claims and a calendar built on the setting greys out days with
 * perfectly good records behind them.
 */
export function useOldestTimestamp(id: string, topic: string) {
  const query = useQuery({
    queryKey: ["oldest", id, topic],
    queryFn: () =>
      get<MessagePage>(
        `/api/clusters/${encode(id)}/topics/${encode(topic)}/messages?mode=oldest&limit=1`
      ),
    staleTime: 5 * 60_000,
    retry: false,
  })
  const first = query.data?.items[0]
  return first && first.kind === "record" ? first.timestamp : undefined
}
