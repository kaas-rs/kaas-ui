import { useQuery } from "@tanstack/react-query";
import type {
  Capabilities,
  ClusterCard,
  ClusterDetail,
  ConfigResourceEntry,
  Envelope,
  GroupDetail,
  GroupOffset,
  GroupSummary,
  LogDir,
  Message,
  TopicDetail,
  TopicSummary,
} from "./types";

/** A request that produced no answer at all. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly kind?: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function get<T>(path: string): Promise<T> {
  const response = await fetch(path, { headers: { accept: "application/json" } });
  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`;
    let kind: string | undefined;
    try {
      const body = await response.json();
      if (typeof body?.message === "string") message = body.message;
      if (typeof body?.kind === "string") kind = body.kind;
    } catch {
      // A non-JSON error body is still an error; the status line stands.
    }
    throw new ApiError(response.status, message, kind);
  }
  return (await response.json()) as T;
}

const encode = encodeURIComponent;

/** How often a view that is backed by a metadata snapshot re-asks. */
const SNAPSHOT_REFRESH = 10_000;

export function useClusters() {
  return useQuery({
    queryKey: ["clusters"],
    queryFn: () => get<Envelope<ClusterCard>>("/api/clusters"),
    refetchInterval: 5_000,
  });
}

export function useCluster(id: string) {
  return useQuery({
    queryKey: ["cluster", id],
    queryFn: () => get<Envelope<ClusterDetail>>(`/api/clusters/${encode(id)}`),
    refetchInterval: SNAPSHOT_REFRESH,
  });
}

export function useCapabilities(id: string) {
  return useQuery({
    queryKey: ["capabilities", id],
    queryFn: () => get<Capabilities>(`/api/clusters/${encode(id)}/capabilities`),
    // Not cached forever: a rolling upgrade changes the answer, and the whole
    // point of the endpoint is that it is allowed to.
    staleTime: 30_000,
    retry: false,
  });
}

export function useLogDirs(id: string, node: number | null) {
  return useQuery({
    queryKey: ["log-dirs", id, node],
    queryFn: () =>
      get<Envelope<LogDir>>(`/api/clusters/${encode(id)}/brokers/${node}/log-dirs`),
    enabled: node !== null,
  });
}

export interface TopicListQuery {
  search?: string;
  internal?: boolean;
  sort?: string;
  order?: "asc" | "desc";
  limit?: number;
  offset?: number;
  sizes?: boolean;
}

export function useTopics(id: string, query: TopicListQuery) {
  const params = new URLSearchParams();
  // Filtering and sorting are server-side: a five-thousand-topic cluster is a
  // real number, and shipping all of it so the browser can hide most of it is
  // how a UI becomes unusable on exactly the cluster that needed one.
  if (query.search) params.set("search", query.search);
  if (query.internal) params.set("internal", "true");
  if (query.sort) params.set("sort", query.sort);
  if (query.order) params.set("order", query.order);
  if (query.limit !== undefined) params.set("limit", String(query.limit));
  if (query.offset) params.set("offset", String(query.offset));
  if (query.sizes) params.set("sizes", "true");

  return useQuery({
    queryKey: ["topics", id, params.toString()],
    queryFn: () =>
      get<Envelope<TopicSummary>>(`/api/clusters/${encode(id)}/topics?${params}`),
    refetchInterval: SNAPSHOT_REFRESH,
  });
}

export function useTopic(id: string, topic: string) {
  return useQuery({
    queryKey: ["topic", id, topic],
    queryFn: () =>
      get<Envelope<TopicDetail>>(
        `/api/clusters/${encode(id)}/topics/${encode(topic)}`,
      ),
    refetchInterval: SNAPSHOT_REFRESH,
  });
}

export function useTopicConfigs(id: string, topic: string) {
  return useQuery({
    queryKey: ["topic-configs", id, topic],
    queryFn: () =>
      get<Envelope<ConfigResourceEntry>>(
        `/api/clusters/${encode(id)}/topics/${encode(topic)}/configs`,
      ),
  });
}

export function useClusterConfigs(id: string, resource: string | null) {
  const query = resource ? `?resource=${encode(resource)}` : "";
  return useQuery({
    queryKey: ["cluster-configs", id, resource],
    queryFn: () =>
      get<Envelope<ConfigResourceEntry>>(
        `/api/clusters/${encode(id)}/configs${query}`,
      ),
  });
}

export function useGroups(id: string) {
  return useQuery({
    queryKey: ["groups", id],
    queryFn: () => get<Envelope<GroupSummary>>(`/api/clusters/${encode(id)}/groups`),
    refetchInterval: SNAPSHOT_REFRESH,
  });
}

export function useGroup(id: string, group: string) {
  return useQuery({
    queryKey: ["group", id, group],
    queryFn: () =>
      get<Envelope<GroupDetail>>(
        `/api/clusters/${encode(id)}/groups/${encode(group)}`,
      ),
  });
}

export function useGroupOffsets(id: string, group: string) {
  return useQuery({
    queryKey: ["group-offsets", id, group],
    queryFn: () =>
      get<Envelope<GroupOffset>>(
        `/api/clusters/${encode(id)}/groups/${encode(group)}/offsets`,
      ),
    refetchInterval: SNAPSHOT_REFRESH,
  });
}

export function useTail(
  id: string,
  topic: string,
  limit: number,
  partitions: string,
  enabled: boolean,
) {
  const params = new URLSearchParams({ limit: String(limit) });
  if (partitions.trim()) params.set("partitions", partitions.trim());

  return useQuery({
    queryKey: ["tail", id, topic, params.toString()],
    queryFn: () =>
      get<Envelope<Message>>(
        `/api/clusters/${encode(id)}/topics/${encode(topic)}/messages/tail?${params}`,
      ),
    enabled,
    // A tail is a point-in-time read, not a subscription. Refetching it behind
    // the reader's back would shuffle rows they are trying to read.
    refetchOnWindowFocus: false,
    staleTime: Infinity,
  });
}
