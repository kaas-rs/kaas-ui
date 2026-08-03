// The wire types.
//
// Hand-written against the utoipa document rather than generated from it: the
// generator is a step in `cargo xtask docs`, and until the schema stops moving
// a hand-written file that a human reads is easier to keep honest than a
// generated one nobody looks at. `kaas-ui --openapi` prints the document these
// mirror, and `cargo xtask docs` writes it to docs/openapi.json.

export type ErrorKind =
  | "transport"
  | "timeout"
  | "auth"
  | "authorization"
  | "broker"
  | "decode"
  | "unsupported"
  | "invalid"
  | "readOnly"
  | "other";

export interface UnsupportedApiDetail {
  api: string;
  apiKey: number;
  /** `null` means the cluster does not implement it at all. */
  broker: [number, number] | null;
  /** `null` means this build has no schema for it. */
  ours: [number, number] | null;
}

export interface ResourceError {
  resource: string;
  kind: ErrorKind;
  code: string | null;
  /** Present even when `code` is null: against a newer broker it is all there is. */
  codeNumber: number | null;
  message: string;
  unsupportedApi?: UnsupportedApiDetail;
  retriable: boolean;
}

export interface Envelope<T> {
  items: T[];
  /** Non-empty is still a successful request. */
  errors: ResourceError[];
  snapshotAgeMs: number | null;
  total?: number;
}

export type ClusterStatus = "connecting" | "ready" | "unreachable";

export interface ClusterCard {
  id: string;
  name: string;
  labels: Record<string, string>;
  status: ClusterStatus;
  error: string | null;
  attempts: number;
  clusterId: string | null;
  controllerId: number | null;
  brokerCount: number;
  topicCount: number;
  internalTopicCount: number;
  partitionCount: number;
  offlinePartitionCount: number;
  underReplicatedPartitionCount: number;
  snapshotAgeMs: number | null;
  maxStalenessMs: number;
  /**
   * What this caller may do on this cluster.
   *
   * Per cluster, not per session: `metadata` on prod and `messages` on dev is
   * one person with two answers. The UI hides what it must not offer — a
   * messages tab that 403s on click is worse than no messages tab — which is
   * the same rule the capability projection follows for what a *broker*
   * cannot answer.
   */
  grants: Grant[];
}

/** The two things a role can permit. Reading is the only verb. */
export type Grant = "metadata" | "messages";

/** `GET /api/me` — who the request is from. */
export interface Identity {
  authenticated: boolean;
  subject: string;
  displayName: string;
  roles: string[];
  /** Whether this deployment applies roles at all. */
  enforcing: boolean;
  /**
   * Whether an identity provider is configured.
   *
   * Distinct from `enforcing`: this decides whether to offer a sign-in
   * button, while `enforcing` decides whether being signed out means seeing
   * nothing.
   */
  loginAvailable: boolean;
}

export interface Broker {
  nodeId: number;
  host: string;
  port: number;
  rack: string | null;
  isController: boolean;
  /** `null` where the cluster does not implement DescribeCluster: unknown, not false. */
  isFenced: boolean | null;
  leaderPartitionCount: number;
  replicaPartitionCount: number;
}

export interface ClusterDescription {
  clusterId: string;
  controllerId: number | null;
}

export interface ClusterDetail {
  cluster: ClusterCard;
  brokers: Broker[];
  description: ClusterDescription | null;
}

export type Feature =
  | "clusterDescription"
  | "topicPartitions"
  | "logDirs"
  | "configs"
  | "consumerGroups"
  | "modernConsumerGroups"
  | "shareGroups"
  | "committedOffsets"
  | "acls"
  | "quotas"
  | "scramUsers"
  | "reassignments"
  | "transactions"
  | "producers"
  | "quorum"
  | "messages";

export type FeatureEntry =
  | { feature: Feature; state: "available" }
  | {
      feature: Feature;
      state: "unsupported";
      api: string;
      apiKey: number;
      broker: [number, number] | null;
      ours: [number, number] | null;
    };

export interface ApiKeyEntry {
  name: string;
  key: number;
  broker: [number, number] | null;
  ours: [number, number] | null;
  negotiated: number | null;
  brokerAhead: boolean;
}

export interface Capabilities {
  features: FeatureEntry[];
  /** Interim: the table is per connection, so the UI says which broker answered. */
  source: { kind: "broker"; nodeId: number | null; peer: string };
  apiKeys: ApiKeyEntry[];
  brokerAheadCount: number;
}

export interface TopicSummary {
  name: string;
  /** Absent on a cluster that reports no topic ids — omit the column, do not show zeroes. */
  topicId: string | null;
  internal: boolean;
  partitionCount: number;
  replicationFactor: number;
  offlinePartitionCount: number;
  underReplicatedPartitionCount: number;
  logicalBytes: number | null;
  replicatedBytes: number | null;
}

export interface Partition {
  partition: number;
  leader: number | null;
  leaderEpoch: number;
  replicas: number[];
  isr: number[];
  offlineReplicas: number[];
  underReplicated: boolean;
  error: string | null;
  earliestOffset: number | null;
  latestOffset: number | null;
}

export interface TopicDetail {
  name: string;
  topicId: string | null;
  internal: boolean;
  partitions: Partition[];
  brokerIds: number[];
}

export interface ConfigEntry {
  name: string;
  value: string | null;
  source: string;
  isExplicit: boolean;
  isSensitive: boolean;
  readOnly: boolean;
  documentation: string | null;
}

export interface ConfigResourceEntry {
  resource: string;
  resourceType: string;
  name: string;
  entries: ConfigEntry[];
}

export interface LogDirReplica {
  topic: string;
  partition: number;
  sizeBytes: number;
  offsetLag: number;
  isFuture: boolean;
}

export interface LogDir {
  path: string;
  totalBytes: number | null;
  usableBytes: number | null;
  replicas: LogDirReplica[];
  error: string | null;
}

export interface GroupSummary {
  groupId: string;
  state: string;
  groupType: string;
  protocolType: string;
  describable: boolean;
}

export interface GroupMember {
  memberId: string;
  instanceId: string | null;
  clientId: string;
  clientHost: string;
  rackId: string | null;
  memberEpoch: number | null;
  subscribedTopics: string[];
  assignment: { topic: string; partitions: number[] }[];
}

/** Four kinds, not one struct with optional fields. */
export type GroupDetail =
  | {
      kind: "classic";
      groupId: string;
      state: string;
      protocolType: string;
      protocol: string;
      members: GroupMember[];
    }
  | {
      kind: "consumer" | "share";
      groupId: string;
      state: string;
      groupEpoch: number;
      assignmentEpoch: number;
      assignor: string;
      members: GroupMember[];
    }
  | { kind: "unrecognized"; groupId: string; groupType: string; state: string };

/** Four states, and they must not all render as `0`. */
export type Lag =
  | { state: "noCommit" }
  | { state: "emptyPartition" }
  | { state: "caughtUp" }
  | { state: "lagging"; records: number }
  | { state: "unknown" };

export interface GroupOffset {
  topic: string;
  partition: number;
  committedOffset: number | null;
  latestOffset: number | null;
  metadata: string | null;
  lag: Lag;
}

export interface Payload {
  encoding: "utf8" | "hex";
  text: string;
  bytes: number;
  truncated: boolean;
}

export interface Message {
  partition: number;
  offset: number;
  timestamp: number;
  timestampType: string;
  key: Payload | null;
  /** `null` is a tombstone, which is not the same as an empty value. */
  value: Payload | null;
  headers: { name: string; value: Payload | null }[];
  transactional: boolean;
  sizeBytes: number;
}

export interface PartitionOffsets {
  partition: number;
  earliestOffset: number | null;
  latestOffset: number | null;
  records: number | null;
}

// --- the message stream ---------------------------------------------------

/** A decoded record, previewed rather than whole. */
export interface StreamRecord {
  kind: "record";
  partition: number;
  offset: number;
  /** Epoch milliseconds. */
  timestamp: number;
  timestampType: string;
  key: Payload | null;
  /** `null` is a tombstone, which is not the same as an empty value. */
  value: Payload | null;
  transactional: boolean;
}

/**
 * A batch that would not decode, as a row.
 *
 * A row and never an error: kaas-lib's decoder keeps going past it, and
 * surfacing it is the entire reason that design exists.
 */
export interface MalformedRow {
  kind: "malformed";
  partition: number;
  offset: number;
  lastOffset: number;
  reason: string;
}

export type StreamRowData = StreamRecord | MalformedRow;

/**
 * A row with the id everything keys on.
 *
 * `{partition}-{offset}`, attached once on arrival: the virtualizer's
 * `getItemKey`, the React key, the selection state and the detail query key
 * are all the same string, and computing it in four places is how they end up
 * disagreeing.
 */
export type StreamRow = StreamRowData & { id: string };

export type StreamPhase = "seeking" | "streaming" | "done";

export interface StreamProgress {
  /** `null` for a live tail, which has no end to be a fraction of. */
  fraction: number | null;
  recordsEmitted: number;
  recordsScanned: number;
  malformedBatches: number;
  partitionsActive: number;
  orderingDegraded: boolean;
  /** Roughly how far apart two partitions may be reordered. `0` means exact. */
  reorderWindow: number;
}

export interface ResolvedPartition {
  partition: number;
  /** `null` where the broker reported no offset at or after the instant. */
  offset: number | null;
  timestamp: number | null;
  error: string | null;
}

/**
 * What a timestamp seek actually landed on.
 *
 * A broker with no timestamp index answers a time seek with nothing, which is
 * a valid response and indistinguishable from "nothing was written since". The
 * `kaas` cluster does exactly this and Strimzi does not, so the answer is shown
 * rather than interpreted.
 */
export interface ResolvedSeek {
  timestamp: number;
  partitions: ResolvedPartition[];
  unresolved: boolean;
}

/** One page of a window, for "load more". */
export interface MessagePage {
  items: StreamRowData[];
  errors: ResourceError[];
  hasMore: boolean;
  nextOffset: number | null;
  resolved?: ResolvedSeek;
}

/** The full payload of one record. */
export type MessageDetail =
  | ({ kind: "record" } & Message)
  | {
      kind: "malformed";
      partition: number;
      offset: number;
      lastOffset: number;
      reason: string;
      /** The batch as it is on disk. */
      raw: Payload;
    };
