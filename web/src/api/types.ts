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
