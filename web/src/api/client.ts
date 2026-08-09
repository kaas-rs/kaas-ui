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
  Health,
  Identity,
  LogDir,
  MessageDetail,
  MessagePage,
  PartitionOffsets,
  SubjectDetail,
  SubjectList,
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

/**
 * The base of every per-cluster route.
 *
 * A cluster is addressed as `(environment, id)` — an id alone addresses
 * nothing, because two environments may each hold a `kafka`. One builder so
 * that no call site can assemble half of that.
 */
function cluster(env: string, id: string): string {
  return `/api/environments/${encode(env)}/clusters/${encode(id)}`
}

/** The base of every schema-registry route. A registry is a peer of a cluster. */
function schemaRegistry(env: string, id: string): string {
  return `/api/environments/${encode(env)}/schema-registries/${encode(id)}`
}

/** How often a view that is backed by a metadata snapshot re-asks. */
const SNAPSHOT_REFRESH = 10_000

/**
 * The running build.
 *
 * `staleTime: Infinity` is not a tuning choice: the binary answering this
 * request cannot change while the page is open. A new version means a new pod,
 * which means a new page. Re-asking would only ever return the same string.
 *
 * `/health` is not under `/api`, but it is under the deployment's base path,
 * so it still goes through `get` rather than a bare `fetch`.
 */
export function useHealth() {
  return useQuery({
    queryKey: ["health"],
    queryFn: () => get<Health>("/health"),
    staleTime: Infinity,
    retry: false,
  })
}

/**
 * Every cluster in one environment.
 *
 * Environment-scoped because a cluster id is: `kafka` in `dev` and `kafka` in
 * `prod` are two clusters, and a flat list could not name either.
 */
export function useClusters(env: string) {
  return useQuery({
    queryKey: ["clusters", env],
    queryFn: () =>
      get<Envelope<ClusterCard>>(`/api/environments/${encode(env)}/clusters`),
    refetchInterval: 5_000,
    enabled: !!env,
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
    queryFn: () => get<Envelope<EnvironmentSection>>("/api/environments"),
    refetchInterval: 5_000,
  })
}

/**
 * One environment, for the page that landed on it directly.
 *
 * Its own request rather than a filter over `useFleet`: the URL names an
 * environment, so "no such environment" and "an environment holding nothing
 * you can see" have to be answerable as a 404 rather than as an empty spot in
 * a list the client happened to fetch for another reason.
 */
export function useEnvironment(env: string) {
  return useQuery({
    queryKey: ["environment", env],
    queryFn: () =>
      get<Envelope<EnvironmentSection>>(`/api/environments/${encode(env)}`),
    refetchInterval: 5_000,
    enabled: !!env,
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

export function useCluster(env: string, id: string) {
  return useQuery({
    queryKey: ["cluster", env, id],
    queryFn: () => get<Envelope<ClusterDetail>>(cluster(env, id)),
    refetchInterval: SNAPSHOT_REFRESH,
  })
}

export function useCapabilities(env: string, id: string) {
  return useQuery({
    queryKey: ["capabilities", env, id],
    queryFn: () => get<Capabilities>(`${cluster(env, id)}/capabilities`),
    // Not cached forever: a rolling upgrade changes the answer, and the whole
    // point of the endpoint is that it is allowed to.
    staleTime: 30_000,
    retry: false,
  })
}

export function useLogDirs(env: string, id: string, node: number | null) {
  return useQuery({
    queryKey: ["log-dirs", env, id, node],
    queryFn: () =>
      get<Envelope<LogDir>>(`${cluster(env, id)}/brokers/${node}/log-dirs`),
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
  metrics?: boolean
}

/**
 * How often the metric columns refetch.
 *
 * Slower than the snapshot, because it is the only part of this page that
 * costs broker round trips — a `DescribeLogDirs` to every broker and a
 * `ListOffsets` to every leader. Ten seconds of that against a large cluster
 * is a load-bearing background job nobody asked for; a minute is a number that
 * still moves while you watch it.
 */
const METRICS_REFRESH = 60_000

function topicParams(query: TopicListQuery): URLSearchParams {
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
  if (query.metrics) params.set("metrics", "true")
  return params
}

export function useTopics(env: string, id: string, query: TopicListQuery) {
  const params = topicParams(query)

  return useQuery({
    queryKey: ["topics", env, id, params.toString()],
    queryFn: () =>
      get<Envelope<TopicSummary>>(`${cluster(env, id)}/topics?${params}`),
    refetchInterval: SNAPSHOT_REFRESH,
  })
}

/**
 * The same page again, with the two columns the brokers have to be asked for.
 *
 * A second request rather than a flag on the first, so the table paints from
 * the metadata snapshot at once and the counts arrive into it. The server
 * enriches only the rows this page asked for, so the payload is fifty topics'
 * partitions rather than the cluster's.
 *
 * Disabled when the sort *is* a metric: the server then has to compute the
 * column for every topic before it can order by it, so the first request
 * already carries the numbers and asking again would repeat the fan-out.
 */
export function useTopicMetrics(
  env: string,
  id: string,
  query: TopicListQuery
) {
  const isMetricSort = query.sort === "size" || query.sort === "messages"
  const params = topicParams({ ...query, metrics: true })

  return useQuery({
    queryKey: ["topic-metrics", env, id, params.toString()],
    queryFn: () =>
      get<Envelope<TopicSummary>>(`${cluster(env, id)}/topics?${params}`),
    enabled: !isMetricSort,
    refetchInterval: METRICS_REFRESH,
    // The previous page's numbers stay on screen while the next page's are in
    // flight, instead of every row blinking back to a placeholder on a sort.
    //
    // Except when disabled, where a placeholder is worse than nothing: the
    // first request already carries fresher numbers for these very rows, and
    // handing back the last page's would shadow them with older ones.
    placeholderData: isMetricSort ? undefined : (previous) => previous,
  })
}

export function useTopic(env: string, id: string, topic: string) {
  return useQuery({
    queryKey: ["topic", env, id, topic],
    queryFn: () =>
      get<Envelope<TopicDetail>>(`${cluster(env, id)}/topics/${encode(topic)}`),
    refetchInterval: SNAPSHOT_REFRESH,
  })
}

/**
 * The same topic again, for the one number the brokers have to be asked for.
 *
 * A second request rather than a flag on the first, exactly like the topic
 * list: the page paints from the describe at once and the size arrives into
 * it. `offsets=false` because the describe above already carried them, and
 * asking twice is a `ListOffsets` per leader for nothing.
 */
export function useTopicSize(env: string, id: string, topic: string) {
  return useQuery({
    queryKey: ["topic-size", env, id, topic],
    queryFn: () =>
      get<Envelope<TopicDetail>>(
        `${cluster(env, id)}/topics/${encode(topic)}?size=true&offsets=false`
      ),
    refetchInterval: METRICS_REFRESH,
  })
}

export function useTopicConfigs(env: string, id: string, topic: string) {
  return useQuery({
    queryKey: ["topic-configs", env, id, topic],
    queryFn: () =>
      get<Envelope<ConfigResourceEntry>>(
        `${cluster(env, id)}/topics/${encode(topic)}/configs`
      ),
  })
}

export function useClusterConfigs(
  env: string,
  id: string,
  resource: string | null
) {
  const query = resource ? `?resource=${encode(resource)}` : ""
  return useQuery({
    queryKey: ["cluster-configs", env, id, resource],
    queryFn: () =>
      get<Envelope<ConfigResourceEntry>>(`${cluster(env, id)}/configs${query}`),
  })
}

export function useGroups(env: string, id: string) {
  return useQuery({
    queryKey: ["groups", env, id],
    queryFn: () => get<Envelope<GroupSummary>>(`${cluster(env, id)}/groups`),
    refetchInterval: SNAPSHOT_REFRESH,
  })
}

export function useGroup(env: string, id: string, group: string) {
  return useQuery({
    queryKey: ["group", env, id, group],
    queryFn: () =>
      get<Envelope<GroupDetail>>(`${cluster(env, id)}/groups/${encode(group)}`),
  })
}

export function useGroupOffsets(env: string, id: string, group: string) {
  return useQuery({
    queryKey: ["group-offsets", env, id, group],
    queryFn: () =>
      get<Envelope<GroupOffset>>(
        `${cluster(env, id)}/groups/${encode(group)}/offsets`
      ),
    refetchInterval: SNAPSHOT_REFRESH,
  })
}

/**
 * One record's full payload, fetched when a row is selected and never before.
 *
 * `staleTime: Infinity` is not a cache-tuning choice: a Kafka record at a
 * given offset is immutable, so re-selecting a row must cost no request at
 * all. The query key is the same `{partition}-{offset}` the list keys on —
 * plus the codec override, because the *rendering* is not immutable: the same
 * record read as hex and read as text are two answers, and pinning one under
 * the other's key would show the wrong one forever.
 */
export function useMessageDetail(
  env: string,
  id: string,
  topic: string,
  rowId: string | undefined,
  codecs?: { keyCodec?: string; valueCodec?: string }
) {
  const parsed = rowId ? parseRowId(rowId) : null
  const params = new URLSearchParams()
  if (codecs?.keyCodec) params.set("keyCodec", codecs.keyCodec)
  if (codecs?.valueCodec) params.set("valueCodec", codecs.valueCodec)
  const query = params.toString()
  return useQuery({
    queryKey: [
      "message",
      id,
      topic,
      rowId,
      codecs?.keyCodec ?? null,
      codecs?.valueCodec ?? null,
    ],
    queryFn: () =>
      get<MessageDetail>(
        `${cluster(env, id)}/topics/${encode(topic)}/messages/${parsed?.partition}/${parsed?.offset}${query ? `?${query}` : ""}`
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
  env: string,
  id: string,
  topic: string,
  params: URLSearchParams
): Promise<MessagePage> {
  return get<MessagePage>(
    `${cluster(env, id)}/topics/${encode(topic)}/messages?${params}`
  )
}

/**
 * The subjects one schema registry holds.
 *
 * Addressed as a peer of a cluster inside its environment, which it was not:
 * it used to hang off a cluster route, because a registry id on its own would
 * have been a second enumerable namespace beside cluster ids. Nesting settles
 * that — the id is scoped to an environment, and the server still refuses a
 * caller who cannot see a cluster there that references it.
 */
export interface SubjectListQuery {
  search?: string
  order?: "asc" | "desc"
  limit?: number
  offset?: number
  details?: boolean
}

function subjectParams(query: SubjectListQuery): URLSearchParams {
  const params = new URLSearchParams()
  if (query.search) params.set("search", query.search)
  if (query.order) params.set("order", query.order)
  if (query.limit !== undefined) params.set("limit", String(query.limit))
  if (query.offset) params.set("offset", String(query.offset))
  if (query.details) params.set("details", "true")
  return params
}

export function useSubjects(
  env: string,
  id: string,
  query: SubjectListQuery = {}
) {
  const params = subjectParams(query)
  return useQuery({
    queryKey: ["schemas", env, id, params.toString()],
    queryFn: () =>
      get<SubjectList>(`${schemaRegistry(env, id)}/subjects?${params}`),
    // A subject registered a moment ago has to appear without a reload; the
    // server caches the listing briefly for the same reason.
    staleTime: 30_000,
  })
}

/**
 * The same page again, with the columns that cost a registry call per row.
 *
 * Split from the listing for the reason the topic table splits its metrics: a
 * name list is one cached call and the other four columns are two calls per
 * subject, so the table paints from the first and fills from the second.
 * Unlike the topic fan-out this cost scales with *rows*, which is why the
 * server describes only the page.
 */
export function useSubjectDetails(
  env: string,
  id: string,
  query: SubjectListQuery
) {
  const params = subjectParams({ ...query, details: true })
  return useQuery({
    queryKey: ["schema-details", env, id, params.toString()],
    queryFn: () =>
      get<SubjectList>(`${schemaRegistry(env, id)}/subjects?${params}`),
    // A cluster that references no registry has no id to ask under, and the
    // topic page calls this before it knows whether there is one.
    enabled: !!id,
    staleTime: 30_000,
    placeholderData: (previous) => previous,
  })
}

/**
 * Every registered version of one subject, with its text.
 *
 * Not `staleTime: Infinity` even though a registered version is immutable: the
 * *list* is not, and a subject gains versions. The server caches the text by
 * `(subject, version)` forever, so re-asking costs one listing call.
 */
export function useSubjectVersions(
  env: string,
  id: string,
  subject: string | undefined
) {
  return useQuery({
    queryKey: ["schema", env, id, subject],
    queryFn: () =>
      get<SubjectDetail>(
        `${schemaRegistry(env, id)}/subjects/${encode(subject ?? "")}/versions`
      ),
    enabled: !!subject,
    staleTime: 30_000,
  })
}

/**
 * Both ends of every partition.
 *
 * Feeds the offset input's bounds and the calendar's disabled days. Partitions
 * that failed ride in `errors` rather than failing the request, so a partition
 * mid-election leaves the control usable and unclamped instead of blocking it.
 */
export function usePartitionBounds(env: string, id: string, topic: string) {
  return useQuery({
    queryKey: ["offsets", id, topic],
    queryFn: () =>
      get<Envelope<PartitionOffsets>>(
        `${cluster(env, id)}/topics/${encode(topic)}/offsets`
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
export function useOldestTimestamp(env: string, id: string, topic: string) {
  const query = useQuery({
    queryKey: ["oldest", env, id, topic],
    queryFn: () =>
      get<MessagePage>(
        `${cluster(env, id)}/topics/${encode(topic)}/messages?mode=oldest&limit=1`
      ),
    staleTime: 5 * 60_000,
    retry: false,
  })
  const first = query.data?.items[0]
  return first && first.kind === "record" ? first.timestamp : undefined
}
