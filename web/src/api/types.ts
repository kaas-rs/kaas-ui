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
  | "other"

export interface UnsupportedApiDetail {
  api: string
  apiKey: number
  /** `null` means the cluster does not implement it at all. */
  broker: [number, number] | null
  /** `null` means this build has no schema for it. */
  ours: [number, number] | null
}

export interface ResourceError {
  resource: string
  kind: ErrorKind
  code: string | null
  /** Present even when `code` is null: against a newer broker it is all there is. */
  codeNumber: number | null
  message: string
  unsupportedApi?: UnsupportedApiDetail
  retriable: boolean
}

export interface Envelope<T> {
  items: T[]
  /** Non-empty is still a successful request. */
  errors: ResourceError[]
  snapshotAgeMs: number | null
  total?: number
}

export type ClusterStatus = "connecting" | "ready" | "unreachable"

export interface ClusterCard {
  /** The environment holding it — the first segment of every URL that reaches it. */
  environment: string
  /** The configured id, unique within that environment. */
  id: string
  name: string
  labels: Record<string, string>
  status: ClusterStatus
  error: string | null
  attempts: number
  clusterId: string | null
  controllerId: number | null
  brokerCount: number
  topicCount: number
  internalTopicCount: number
  partitionCount: number
  offlinePartitionCount: number
  underReplicatedPartitionCount: number
  snapshotAgeMs: number | null
  maxStalenessMs: number
  /**
   * What this caller may do on this cluster.
   *
   * Per cluster, not per session: `metadata` on prod and `messages` on dev is
   * one person with two answers. The UI hides what it must not offer — a
   * messages tab that 403s on click is worse than no messages tab — which is
   * the same rule the capability projection follows for what a *broker*
   * cannot answer.
   */
  grants: Partial<Record<Resource, Action[]>>
  /**
   * The schema registry this cluster references, by its configured id.
   *
   * `null` is a normal path, not a degraded one — and it is what the sidebar
   * reads to decide whether a schemas item exists at all.
   */
  schemaRegistry: string | null
}

/** What a non-cluster resource is. Decides the icon and the wording. */
export type ResourceKind =
  "schema_registry" | "mqtt_broker" | "kafka_connect" | "rest_proxy" | "other"

/**
 * One thing in an environment that is not a Kafka cluster.
 *
 * **No status field, deliberately.** kaas-ui dials none of these, so it knows
 * one is configured and not that it is up. The card says "not probed" because
 * that is the whole truth — a green badge earned by a correctly typed URL is
 * the one thing a fleet view must never show.
 */
export interface ResourceCard {
  id: string
  name: string
  kind: ResourceKind
  endpoint: string | null
  note: string | null
  labels: Record<string, string>
}

/** One section of the fleet: an environment and everything in it. */
export interface EnvironmentSection {
  /** The configured id, and the first segment of every URL beneath it. */
  id: string
  name: string
  description: string | null
  clusters: ClusterCard[]
  /** The registries in it this caller may read. */
  schemaRegistries: EnvironmentRegistry[]
  resources: ResourceCard[]
}

/**
 * A schema registry inside an environment.
 *
 * Not the `ResourceCard` beside it — that is an inventory line with its own id
 * and nothing behind it. This one is addressable:
 * `/environments/{env}/schema-registries/{id}`. The id is scoped to the
 * environment, so it can lead a route without becoming a namespace anyone can
 * enumerate.
 */
export interface EnvironmentRegistry {
  registry: RegistryCard
  /** The clusters here that decode against it. Only the visible ones. */
  usedBy: string[]
}

/**
 * What a permission is about. Mirrors kafbat-ui's resource list, minus the
 * ones this application has no code for.
 *
 * Snake case, unlike every other field on the wire: these are the words the
 * config file uses, and the API echoes them verbatim so that what the UI hides
 * and what an operator typed are visibly the same vocabulary.
 */
export type Resource = "cluster_config" | "topic" | "consumer"

/**
 * What may be done to it.
 *
 * Two, because reading is the only verb kaas-ui has: `create`, `edit`,
 * `delete`, `messagesProduce` and the rest of kafbat-ui's vocabulary describe
 * writes, and there is no code path here that could perform one.
 */
export type Action = "view" | "messages_read"

/** `GET /api/me` — who the request is from. */
export interface Identity {
  authenticated: boolean
  subject: string
  displayName: string
  roles: string[]
  /** Whether this deployment applies roles at all. */
  enforcing: boolean
  /**
   * Whether an identity provider is configured.
   *
   * Distinct from `enforcing`: this decides whether to offer a sign-in
   * button, while `enforcing` decides whether being signed out means seeing
   * nothing.
   */
  loginAvailable: boolean
  /**
   * The named ways to sign in, if this deployment lists any.
   *
   * Empty means one unlabelled button and the provider asks which connector —
   * Dex serves its own chooser page when it has more than one. A non-empty
   * list is the deployment saying it would rather ask that itself, and the
   * sign-in screen draws one button per entry.
   */
  connectors: LoginConnector[]
}

/** One way to sign in. The id is opaque and only Dex knows what it means. */
export interface LoginConnector {
  /** Sent back as `/auth/login?connector=<id>`. */
  id: string
  /** What the button says. */
  name: string
}

/**
 * `GET /health` — liveness, and the only place the build names itself.
 *
 * Outside `/api` because a liveness probe is not part of the API surface, and
 * mirrored here because the header reads `version` from it. That field is
 * `CARGO_PKG_VERSION`, so it is the *binary's* number rather than the image
 * tag it was pulled under — which is exactly what makes it worth showing. The
 * two disagreeing is the drift the endpoint exists to expose.
 */
export interface Health {
  status: string
  version: string
  auth: "open" | "enforcing"
}

export interface Broker {
  nodeId: number
  host: string
  port: number
  rack: string | null
  isController: boolean
  /** `null` where the cluster does not implement DescribeCluster: unknown, not false. */
  isFenced: boolean | null
  leaderPartitionCount: number
  replicaPartitionCount: number
}

export interface ClusterDescription {
  clusterId: string
  controllerId: number | null
}

export interface ClusterDetail {
  cluster: ClusterCard
  brokers: Broker[]
  description: ClusterDescription | null
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
  | "messages"

export type FeatureEntry =
  | { feature: Feature; state: "available" }
  | {
      feature: Feature
      state: "unsupported"
      api: string
      apiKey: number
      broker: [number, number] | null
      ours: [number, number] | null
    }

export interface ApiKeyEntry {
  name: string
  key: number
  broker: [number, number] | null
  ours: [number, number] | null
  negotiated: number | null
  brokerAhead: boolean
}

export interface Capabilities {
  features: FeatureEntry[]
  /** Interim: the table is per connection, so the UI says which broker answered. */
  source: { kind: "broker"; nodeId: number | null; peer: string }
  apiKeys: ApiKeyEntry[]
  brokerAheadCount: number
}

export interface TopicSummary {
  name: string
  /** Absent on a cluster that reports no topic ids — omit the column, do not show zeroes. */
  topicId: string | null
  internal: boolean
  partitionCount: number
  replicationFactor: number
  offlinePartitionCount: number
  underReplicatedPartitionCount: number
  /** Retained records across every partition. Null until `?metrics=true` answers, and on a topic where any partition failed to. */
  messageCount: number | null
  logicalBytes: number | null
  replicatedBytes: number | null
}

export interface Partition {
  partition: number
  leader: number | null
  leaderEpoch: number
  replicas: number[]
  isr: number[]
  offlineReplicas: number[]
  underReplicated: boolean
  error: string | null
  earliestOffset: number | null
  latestOffset: number | null
}

export interface TopicDetail {
  name: string
  topicId: string | null
  internal: boolean
  partitions: Partition[]
  brokerIds: number[]
}

export interface ConfigEntry {
  name: string
  value: string | null
  source: string
  isExplicit: boolean
  isSensitive: boolean
  readOnly: boolean
  documentation: string | null
}

export interface ConfigResourceEntry {
  resource: string
  resourceType: string
  name: string
  entries: ConfigEntry[]
}

export interface LogDirReplica {
  topic: string
  partition: number
  sizeBytes: number
  offsetLag: number
  isFuture: boolean
}

export interface LogDir {
  path: string
  totalBytes: number | null
  usableBytes: number | null
  replicas: LogDirReplica[]
  error: string | null
}

export interface GroupSummary {
  groupId: string
  state: string
  groupType: string
  protocolType: string
  describable: boolean
}

export interface GroupMember {
  memberId: string
  instanceId: string | null
  clientId: string
  clientHost: string
  rackId: string | null
  memberEpoch: number | null
  subscribedTopics: string[]
  assignment: { topic: string; partitions: number[] }[]
}

/** Four kinds, not one struct with optional fields. */
export type GroupDetail =
  | {
      kind: "classic"
      groupId: string
      state: string
      protocolType: string
      protocol: string
      members: GroupMember[]
    }
  | {
      kind: "consumer" | "share"
      groupId: string
      state: string
      groupEpoch: number
      assignmentEpoch: number
      assignor: string
      members: GroupMember[]
    }
  | { kind: "unrecognized"; groupId: string; groupType: string; state: string }

/** Four states, and they must not all render as `0`. */
export type Lag =
  | { state: "noCommit" }
  | { state: "emptyPartition" }
  | { state: "caughtUp" }
  | { state: "lagging"; records: number }
  | { state: "unknown" }

export interface GroupOffset {
  topic: string
  partition: number
  committedOffset: number | null
  latestOffset: number | null
  metadata: string | null
  lag: Lag
}

/**
 * How a payload was read.
 *
 * The chip in the message list **is** this value, and the chip is the override
 * control rather than a label: auto-detection that cannot be corrected is
 * worse than none. The last three cannot be chosen — nothing can invent a
 * schema id for a payload that does not carry one — so the picker offers the
 * first four and the server refuses the rest with a reason.
 */
export type Codec =
  "auto" | "string" | "hex" | "json" | "avro" | "protobuf" | "jsonSchema"

/** The four a reader may ask for. Falling back never needs a schema. */
export const CHOOSABLE_CODECS: Codec[] = ["auto", "string", "hex", "json"]

export type SchemaFormat = "avro" | "protobuf" | "json"

/** Which schema decoded a payload, and which registry answered. */
export interface SchemaRef {
  id: number
  format: SchemaFormat
  /** The configured registry id. A schema id means nothing without it. */
  registry: string
  subject: string | null
  version: number | null
  /** The record or message name inside the schema. */
  name: string | null
}

/**
 * Why a payload is not what the reader asked for.
 *
 * Five causes, kept apart because they want five different things done about
 * them — and because the alternative is a topic that silently renders as hex
 * with no way to tell a broken registry from a broken URL.
 */
export type NoteKind =
  | "decodeError"
  | "registryUnavailable"
  | "registryAbsent"
  | "registryMisconfigured"
  | "overrideRefused"
  | "nonConforming"

export interface PayloadNote {
  kind: NoteKind
  message: string
}

export interface Payload {
  codec: Codec
  encoding: "utf8" | "hex" | "json"
  text: string
  bytes: number
  truncated: boolean
  /**
   * The bytes as they arrived, hex-encoded.
   *
   * Present only for a registry-backed decode, which is the only rendering the
   * original cannot be recovered from — and it is what makes dropping to hex
   * or string a client-side change rather than a refetch.
   */
  raw?: { hex: string; truncated: boolean }
  schema?: SchemaRef
  note?: PayloadNote
}

export interface Message {
  partition: number
  offset: number
  timestamp: number
  timestampType: string
  key: Payload | null
  /** `null` is a tombstone, which is not the same as an empty value. */
  value: Payload | null
  headers: { name: string; value: Payload | null }[]
  transactional: boolean
  sizeBytes: number
}

export interface PartitionOffsets {
  partition: number
  earliestOffset: number | null
  latestOffset: number | null
  records: number | null
}

// --- the message stream ---------------------------------------------------

/** A decoded record, previewed rather than whole. */
export interface StreamRecord {
  kind: "record"
  partition: number
  offset: number
  /** Epoch milliseconds. */
  timestamp: number
  timestampType: string
  key: Payload | null
  /** `null` is a tombstone, which is not the same as an empty value. */
  value: Payload | null
  transactional: boolean
}

/**
 * A batch that would not decode, as a row.
 *
 * A row and never an error: kaas-lib's decoder keeps going past it, and
 * surfacing it is the entire reason that design exists.
 */
export interface MalformedRow {
  kind: "malformed"
  partition: number
  offset: number
  lastOffset: number
  reason: string
}

export type StreamRowData = StreamRecord | MalformedRow

/**
 * A row with the id everything keys on.
 *
 * `{partition}-{offset}`, attached once on arrival: the virtualizer's
 * `getItemKey`, the React key, the selection state and the detail query key
 * are all the same string, and computing it in four places is how they end up
 * disagreeing.
 */
export type StreamRow = StreamRowData & { id: string }

export type StreamPhase = "seeking" | "streaming" | "done"

export interface StreamProgress {
  /** `null` for a live tail, which has no end to be a fraction of. */
  fraction: number | null
  recordsEmitted: number
  recordsScanned: number
  malformedBatches: number
  partitionsActive: number
  orderingDegraded: boolean
  /** Roughly how far apart two partitions may be reordered. `0` means exact. */
  reorderWindow: number
}

export interface ResolvedPartition {
  partition: number
  /** `null` where the broker reported no offset at or after the instant. */
  offset: number | null
  timestamp: number | null
  error: string | null
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
  timestamp: number
  partitions: ResolvedPartition[]
  unresolved: boolean
}

/** One page of a window, for "load more". */
export interface MessagePage {
  items: StreamRowData[]
  errors: ResourceError[]
  hasMore: boolean
  nextOffset: number | null
  resolved?: ResolvedSeek
  predicate?: PredicateStats
}

/**
 * What the JS predicate has done.
 *
 * Rendered because a filter that dropped a thousand records for exceeding its
 * budget looks exactly like a filter that matched nothing.
 */
export interface PredicateStats {
  evaluated: number
  matched: number
  /** Killed by the per-record budget. Nobody knows whether these matched. */
  timedOut: number
  failed: number
  lastError?: string
}

// --- the schema registry --------------------------------------------------

export type RegistryStatus =
  "unprobed" | "ready" | "unreachable" | "misconfigured"

/**
 * The registry answering for a cluster.
 *
 * Named on every page it backs, because a registry serves an *environment*:
 * two clusters showing the same subjects are not a coincidence to explain
 * away, they are the same registry answering both.
 */
export interface RegistryCard {
  id: string
  name: string
  url: string
  status: RegistryStatus
  error: string | null
}

export interface SchemaReference {
  name: string
  subject: string
  version: number
}

export interface SubjectSchema {
  subject: string
  version: number
  id: number
  format: SchemaFormat
  schema: string
  references: SchemaReference[]
}

/**
 * One row of the subject table.
 *
 * Everything but the name is null until `?details=true` answers: the name came
 * free with the listing, the rest is a call per subject.
 */
export interface SubjectRow {
  subject: string
  id: number | null
  format: SchemaFormat | null
  version: number | null
  compatibility: string | null
  /** True when the mode is the registry's default rather than this subject's own. */
  compatibilityInherited: boolean
}

export interface SubjectList {
  /** `null` where the cluster references no registry, which is normal. */
  registry: RegistryCard | null
  subjects: SubjectRow[]
  /** How many subjects matched before paging. */
  total: number
  /** The registry-wide default, when details were asked for. */
  compatibility: string | null
}

export interface SubjectDetail {
  registry: RegistryCard | null
  subject: string
  compatibility: string | null
  versions: SubjectSchema[]
  errors: ResourceError[]
}

/** The full payload of one record. */
export type MessageDetail =
  | ({ kind: "record" } & Message)
  | {
      kind: "malformed"
      partition: number
      offset: number
      lastOffset: number
      reason: string
      /** The batch as it is on disk. */
      raw: Payload
    }
