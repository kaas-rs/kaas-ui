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
  /** The Connect clusters in it this caller may see, in configured order. */
  connectClusters: ConnectClusterCard[]
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
  /**
   * Which subjects name this topic. Null until `?schemas=true` answers, on a
   * cluster that reads no registry, and where the registry would not answer —
   * a topic with nothing registered is an object with both sides null, which
   * is a different fact and has to look like one.
   */
  schemas: TopicSchemas | null
}

/**
 * The subjects of one topic, by the side of the record they decode.
 *
 * Read from the subject names, so `TopicNameStrategy` and nothing else: a
 * `{topic}-{record}` subject hides its seam in the schema, and the topic page's
 * schema card is where that one is answered.
 */
export interface TopicSchemas {
  registry: string
  key: string | null
  value: string | null
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
  /** Every non-future copy summed. Null until `?size=true` answers. */
  replicatedBytes: number | null
  /**
   * The leader's copy alone. Null until `?size=true` answers, and on a
   * partition no broker reported a leader's copy for — never 0 for that case,
   * because 0 is a claim that the partition is empty.
   *
   * The figure to compare against `segment.bytes`: a segment counts bytes in
   * one log, so `replicatedBytes` is wrong by the replication factor.
   */
  logicalBytes: number | null
  /**
   * The worst follower's offset lag. Null until `?size=true` answers, and on
   * a partition with no followers; 0 is a claim that every follower is
   * caught up.
   */
  maxFollowerLag: number | null
}

export interface TopicDetail {
  name: string
  topicId: string | null
  internal: boolean
  partitions: Partition[]
  brokerIds: number[]
  /** The smallest replica count across partitions — the same rule the list uses. */
  replicationFactor: number
  offlinePartitionCount: number
  underReplicatedPartitionCount: number
  /** Retained records. Null when any partition is missing an offset end — absent, not smaller. */
  messageCount: number | null
  /** Bytes on disk for one copy. Null until `?size=true` answers. */
  logicalBytes: number | null
  /** Bytes on disk across replicas. Null is "not asked, or not answered" — never zero. */
  replicatedBytes: number | null
  /** Log-directory entries holding a copy: one per replica per directory. */
  logDirEntryCount: number | null
}

/** One analysis progress frame, off the `progress` SSE event. */
export interface AnalysisProgress {
  startedAt: number
  /** 0..1 where the offset range is known; an upper bound under compaction. */
  fraction?: number
  msgsScanned: number
  bytesScanned: number
  offsetsConsumed: number
  offsetsTotal: number
  malformedBatches: number
  elapsedMs: number
}

/**
 * Size statistics for one side of the record. `sum`/`min`/`max`/`avg` are
 * exact; the percentiles are sketch estimates (±~4%) and must be labelled so.
 */
export interface SizeStats {
  sum: number
  min: number
  max: number
  avg: number
  p50?: number
  p75?: number
  p95?: number
  p99?: number
  p999?: number
}

export interface HourCount {
  hourStart: number
  count: number
}

/** One accumulator's numbers: the topic's (partition null) or one partition's. */
export interface AnalysisStats {
  partition?: number
  totalMsgs: number
  minOffset?: number
  maxOffset?: number
  minTimestamp?: number
  maxTimestamp?: number
  missingTimestamps: number
  nullKeys: number
  /** On a compacted topic this is the tombstone count. */
  nullValues: number
  /** Estimated (cardinality sketch, ±~1.6%). */
  approxUniqKeys: number
  approxUniqValues: number
  keySize?: SizeStats
  valueSize?: SizeStats
  hourlyMsgCounts: HourCount[]
  hourlyTruncated: boolean
  malformedBatches: number
}

/** What ended an analysis scan. Everything but `error` is a valid result. */
export type AnalysisStop = "end" | "messageCap" | "timeCap" | "error"

/** The terminal `result` event of one analysis. */
export interface TopicAnalysis {
  startedAt: number
  finishedAt: number
  /** False when the scan was cut short — the numbers cover what was read. */
  complete: boolean
  /** What ended the scan — the reason `complete` is what it is. */
  stoppedBy: AnalysisStop
  scannedFraction?: number
  /** Which clock stamped the timestamps: createTime, logAppendTime or mixed. */
  clock?: string
  totalStats: AnalysisStats
  partitionStats: AnalysisStats[]
  errors: ResourceError[]
  /**
   * What the numbers above imply about how this topic is configured.
   *
   * Attached after the fold, from two describes the scan's own cost makes
   * free by comparison. Absent when neither answered — the statistics stand
   * on their own, and a missing recommendation is not a missing result.
   */
  sizing?: SizingAdvice
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

/** How confident a sizing diagnostic is, not how much it matters. */
export type Severity = "warn" | "info"

/**
 * What a sizing diagnostic is about.
 *
 * The stable half of a finding: the prose moves, this does not, so a chip or
 * a link may key on it.
 */
export type DiagnosticCode =
  | "retentionOutlivesItsSegment"
  | "retentionBytesSmallerThanSegment"
  | "noRetentionAtAll"
  | "compactedTailNeverCleaned"
  | "segmentAboveIndexCeiling"
  | "skewedPartitions"

export interface Diagnostic {
  code: DiagnosticCode
  severity: Severity
  summary: string
  /** The derivation. Every number in it came from the cluster. */
  detail: string
  /** The partitions the evidence came from. Absent when it came from configuration alone. */
  partitions?: number[]
}

export interface SettingValue {
  name: string
  value: string | null
  isExplicit: boolean
}

export interface SizeEvidence {
  logicalBytes: number
  smallestPartitionBytes: number
  largestPartitionBytes: number
  /** Against `Topology.partitions`, how much of the topic the evidence covers. */
  partitionsMeasured: number
}

/** The topic's shape, which is metadata rather than measurement. */
export interface Topology {
  partitions: number
  replicationFactor: number
  brokers: number
}

/**
 * What the scan measured, in the units the advice reasons in.
 *
 * Every field is null where its divisor was zero rather than 0 — an idle topic
 * has no write rate, and a zero would put a one-partition recommendation on a
 * topic nobody has produced to yet.
 */
export interface Measured {
  records: number
  complete: boolean
  windowMs: number | null
  /** The resolution the peak was measured at: the scan buckets by hour. */
  bucketMs: number
  meanRecordsPerSec: number | null
  peakRecordsPerSec: number | null
  peakToMean: number | null
  bytesPerRecord: number | null
  bytesPerRecordSource: string
  payloadBytesPerRecord: number | null
  /** Payload over on-disk bytes. Above 1 is compression working. */
  compressionRatio: number | null
  meanBytesPerSec: number | null
  peakBytesPerSec: number | null
  partitionSkew: number | null
  keyedFraction: number | null
  distinctKeyFraction: number | null
  tombstoneFraction: number | null
  clock?: string
}

/** The numbers nothing in a log can supply. Defaults, and labelled as such. */
export interface Assumptions {
  produceMibPerSecPerPartition: number
  consumeMibPerSecPerConsumer: number
  headroomPercent: number
  segmentsPerRetention: number
  throughputSegmentsPerRetention: number
  compactedRollMinutes: number
  overshootPercent: number
  minSegmentMib: number
  targetReplication: number
  burstRatio: number
  highThroughputMibPerSec: number
  precisionWindowHours: number
  smallRecordKib: number
}

/** What a topic is for, which nothing in its log records. */
export type Profile =
  | "balanced"
  | "compactedChangelog"
  | "highThroughput"
  | "lowLatency"
  | "bursty"
  | "streamsInternal"
  | "retentionPrecision"

/** Which way a row points. */
export type Change = "increase" | "decrease" | "keep" | "review"

export interface Recommendation {
  setting: string
  current: string | null
  /** Null when the measurement did not support a number; `why` says which input was missing. */
  recommended: string | null
  change: Change
  why: string
  /** What is irreversible about making the change. */
  caution?: string
}

export interface ProfileAdvice {
  profile: Profile
  label: string
  optimises: string
  /** Whether this topic's signals point here. */
  matched: boolean
  /** Why it matched, or what would make it match. */
  signal: string
  rows: Recommendation[]
}

export interface SizingAdvice {
  topic: string
  topology: Topology
  settings: SettingValue[]
  /** Absent when log dirs did not answer — never because the topic is empty. */
  sizes?: SizeEvidence
  measured: Measured
  assumptions: Assumptions
  /** The profile the signals point at. The card opens here and says why. */
  suggested: Profile
  suggestedBecause: string
  /** All seven, so switching between them costs no request. */
  profiles: ProfileAdvice[]
  diagnostics: Diagnostic[]
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

/**
 * One broker's TLS session, as kaas-ui's own connection to it negotiated.
 *
 * Per broker because that is all there is: Kafka has no api that returns a
 * certificate, so the only one kaas-ui knows is the one presented to it, on
 * the listener it dials. A broker that refused the handshake has no row — it
 * is an `errors` entry naming what was rejected.
 */
export interface BrokerTls {
  nodeId: number
  /** The advertised address dialled. */
  peer: string
  /** `null` on a plaintext listener: a configuration, not a failure. */
  session: TlsSession | null
}

export interface TlsSession {
  protocolVersion: string
  cipherSuite: string
  /** What the leaf was verified against. Compare SANs to this, not to the host. */
  serverName: string
  /** Leaf first, as the broker sent it. */
  chain: Certificate[]
}

export interface Certificate {
  subject: string
  issuer: string
  serial: string
  /** Milliseconds since the epoch. */
  notBefore: number
  notAfter: number
  dnsNames: string[]
  ipAddresses: string[]
  /** Empty when the extension is absent. */
  keyUsage: string[]
  /** Empty when the extension is absent — which means "any purpose". */
  extendedKeyUsage: string[]
  /** `null` when basicConstraints is absent: not the same claim as `false`. */
  isCa: boolean | null
  publicKey: string
  signatureAlgorithm: string
  selfSigned: boolean
  sha256Fingerprint: string
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
  /**
   * Whether the read filled its budget — not whether the page is full.
   *
   * The payload filter runs after the decode, so a page of three rows may
   * have read five hundred records with half the topic still to walk.
   */
  hasMore: boolean
  nextOffset: number | null
  resolved?: ResolvedSeek
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
  /**
   * The fully-qualified name the schema declares — an Avro or Protobuf name, a
   * JSON Schema title. `null` where it declares none, which costs only the
   * topic link.
   */
  recordName: string | null
  references: SchemaReference[]
}

/**
 * Which of the three subject naming strategies formed a subject name.
 *
 * `unrecognized` is not a failure: a subject registered by hand fits none of
 * them, and saying so beats splitting it at a guessed `-`.
 */
export type NamingStrategy =
  "topicName" | "topicRecordName" | "recordName" | "unrecognized"

export interface SubjectNaming {
  strategy: NamingStrategy
  /**
   * The topic the subject names, where the strategy carries one — so under
   * `topicName` and `topicRecordName`, and never under the other two.
   */
  topic: string | null
  recordName: string | null
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
  /**
   * How the name was formed, and the topic that recovers from it.
   *
   * Present without `?details=true` as well, read from the name alone — which
   * resolves the two suffix strategies and nothing else. With details it is
   * read from the newest schema, and only then can `{topic}-{record}` be told
   * from a longer topic's `-value`.
   */
  naming: SubjectNaming
}

export interface SubjectList {
  /** `null` where the cluster references no registry, which is normal. */
  registry: RegistryCard | null
  subjects: SubjectRow[]
  /** How many subjects matched before paging. */
  total: number
  /**
   * How many distinct topics they name, read off the names alone.
   *
   * So `TopicNameStrategy` and nothing else — `{topic}-{record}` needs the
   * record name to find its seam, and a record-named subject never had a topic
   * in it. Describes the whole listing rather than the page, which is what
   * makes `?limit=0` a cheap way to ask what a registry holds.
   */
  topics: number
  /**
   * How many matched subjects name a topic that is on no cluster reading this
   * registry.
   *
   * A schema outlives the topic it described: deleting a topic does not touch
   * the registry, so the subject stays registered and stays a well-formed
   * name. Counted in *subjects*, so a deleted topic with a key and a value
   * schema is two.
   *
   * `null` where the question has no answer — no cluster here references the
   * registry, or one that does is disconnected and its topics are unknown.
   * Not `0`, which would read as "all fine" while a cluster is down.
   */
  dangling: number | null
  /** The registry-wide default, when details were asked for. */
  compatibility: string | null
}

export interface SubjectDetail {
  registry: RegistryCard | null
  subject: string
  /** Read from the newest version, because the name is the schema's. */
  naming: SubjectNaming
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

// --- The read-only admin surface -----------------------------------------
//
// Phase 7. Every one of these is a describe; nothing in this section has a
// mutating counterpart in the client, because there is no route to reach one.

/** One ACL binding, as the authorizer stores it. */
export interface Acl {
  /** `topic`, `group`, `cluster`, `transactionalId`, `delegationToken`, `user`. */
  resourceType: string
  /** The name, or `*` for every resource of that type. */
  resourceName: string
  /** `literal` covers one name; `prefixed` covers a namespace. */
  patternType: string
  /** `User:alice`, as the authorizer spells it. */
  principal: string
  host: string
  /** An operation this build has no name for arrives as `unknown(99)`. */
  operation: string
  /** `allow` or `deny` — denies win, so this is a column and not a colour. */
  permission: string
}

/** One quota entity and the limits configured for it. */
export interface ClientQuota {
  entity: QuotaComponent[]
  values: QuotaValue[]
}

export interface QuotaComponent {
  /** `user`, `client-id` or `ip`. */
  entityType: string
  /** Null is the *default* entity of that type, not an unset name. */
  name: string | null
}

export interface QuotaValue {
  key: string
  value: number
}

/** Who can authenticate with SCRAM — never how. */
export interface ScramUser {
  user: string
  credentials: ScramCredential[]
}

export interface ScramCredential {
  mechanism: string
  iterations: number
}

/** One partition being moved. */
export interface Reassignment {
  topic: string
  partition: number
  /** Holds the added and the removed until the move completes. */
  replicas: number[]
  adding: number[]
  removing: number[]
}

/** One transaction. Everything past `state` needs `?details=true`. */
export interface Transaction {
  transactionalId: string
  producerId: number
  /** The broker's own vocabulary, passed through. */
  state: string
  /**
   * When the current transaction started, in epoch milliseconds.
   *
   * The start, not the duration: how long it has been open is the number that
   * matters and it is wrong the moment it is serialised, so the browser ticks
   * it the way it ticks `snapshotAgeMs`.
   */
  startTimeMs: number | null
  timeoutMs: number | null
  producerEpoch: number | null
  partitions: TransactionPartitions[]
}

export interface TransactionPartitions {
  topic: string
  partitions: number[]
}

/** One producer writing to one partition. */
export interface Producer {
  topic: string
  partition: number
  producerId: number
  /** Not a leader epoch: this bumps when a producer is fenced. */
  producerEpoch: number
  lastSequence: number
  lastTimestamp: number
  coordinatorEpoch: number
  /** Set only where this producer has a transaction open. */
  currentTxnStartOffset: number | null
}

// --- Kafka Connect -----------------------------------------------------------
//
// A Connect cluster is a peer of a Kafka cluster inside an environment, dialled
// over its REST API — read-only: the server's client has no method that sends
// anything but GET, and connector configs are redacted before they leave it.

/** Whether a Connect cluster has been reached. `unprobed` is a real state:
    nothing is asked until a page needs it. */
export type ConnectStatus =
  "unprobed" | "ready" | "unreachable" | "misconfigured"

/** What `GET /` says about the workers. The version is displayed, never compared. */
export interface ServerInfo {
  version: string | null
  commit: string | null
  /** The Kafka cluster the workers keep their state in. */
  kafkaClusterId: string | null
}

export interface ConnectClusterCard {
  id: string
  name: string
  url: string
  /** The Kafka cluster this entry says its workers use. */
  kafkaCluster: string | null
  status: ConnectStatus
  error: string | null
  info: ServerInfo | null
}

export type KafkaClusterCheckState =
  "verified" | "mismatch" | "unverified" | "notConfigured"

export interface KafkaClusterCheck {
  state: KafkaClusterCheckState
  /** The id kaas-ui sees for the referenced Kafka cluster. */
  expected: string | null
  /** The id the workers report. */
  actual: string | null
}

export interface ConnectorCounts {
  total: number
  running: number
  degraded: number
  failed: number
  paused: number
  stopped: number
  other: number
}

export interface ConnectClusterDetail {
  card: ConnectClusterCard
  kafkaClusterCheck: KafkaClusterCheck
  counts: ConnectorCounts | null
  pluginCount: number | null
  errors: ResourceError[]
  /** Creating a connector here, where the ceiling lists it. */
  create: WriteOption | null
}

export type ConnectorType = "source" | "sink" | "unknown"

/**
 * A connector's state folded with its tasks'. Paused is not failed — somebody
 * chose it — and a running connector with no tasks is its own state.
 */
export type ConnectorStatus =
  | "running"
  | "degraded"
  | "failed"
  | "paused"
  | "stopped"
  | "unassigned"
  | "restarting"
  | "noTasks"
  | "unknown"

export interface TaskStatus {
  id: number
  /** RUNNING, FAILED, PAUSED, UNASSIGNED, RESTARTING — verbatim. */
  state: string | null
  workerId: string | null
  trace: string | null
  version: string | null
}

export interface ConnectorSummary {
  name: string
  class: string | null
  type: ConnectorType
  status: ConnectorStatus
  /** The connector's own state, verbatim. */
  state: string | null
  workerId: string | null
  trace: string | null
  version: string | null
  tasks: TaskStatus[]
  tasksRunning: number
  tasksFailed: number
  /** The group a sink commits to; null for a source. */
  consumerGroup: string | null
}

export interface ConnectorList {
  card: ConnectClusterCard
  connectors: ConnectorSummary[]
  total: number
}

export type Redaction = "declared" | "patternsOnly"

export interface ConnectorConfigEntry {
  key: string
  /** `******` where withheld. */
  value: string
  redacted: boolean
  /** A config-provider reference — a pointer, not a secret. */
  provider: boolean
  group: string | null
  documentation: string | null
}

export interface RedactedConfig {
  entries: ConnectorConfigEntry[]
  redaction: Redaction
}

export interface TaskConfig {
  id: number
  config: RedactedConfig
}

export interface ConnectorDetail {
  summary: ConnectorSummary
  config: RedactedConfig
  taskConfigs: TaskConfig[]
}

/** Both halves are maps the connector defines. */
export interface StoredOffset {
  partition: Record<string, unknown>
  /** null: the stored position was cleared — not a position of zero. */
  offset: Record<string, unknown> | null
}

export interface ConnectorPage {
  card: ConnectClusterCard
  connector: ConnectorDetail
  topics: string[] | null
  /** Why `topics` is absent, where it is. */
  topicsAbsent: string | null
  offsets: StoredOffset[] | null
  offsetsAbsent: string | null
  errors: ResourceError[]
  /** Every write on the Connect cluster's ceiling, checked for this caller.
      Empty without a ceiling: the read-only default draws no write UI. */
  writes: WriteOption[]
  /** Set when Git owns this connector. */
  managedInGit: Owned | null
}

export interface Plugin {
  class: string
  type: ConnectorType
  version: string | null
}

export interface PluginList {
  card: ConnectClusterCard
  plugins: Plugin[]
}

/** One connector that has used a topic, from its worker's active-topic list. */
export interface TopicConnector {
  connectCluster: string
  connectName: string
  name: string
  type: ConnectorType
  status: ConnectorStatus
}

export interface TopicConnectors {
  connectors: TopicConnector[]
  /** Connect clusters or connectors that could not be asked: the list is then
      a lower bound. */
  errors: ResourceError[]
}

// --- Phase 10: writes behind the gate ----------------------------------------

/** A catalogue operation, spelled as `writes.allow` spells it. */
export type Operation =
  | "connector.restart"
  | "connector.restart_task"
  | "connector.pause"
  | "connector.resume"
  | "connector.stop"
  | "connector.reset_topics"
  | "connector.edit"
  | "connector.create"
  | "connector.delete"
  | "connector.reset_offsets"
  | "connector.alter_offsets"

/** Operate: not state, allowed on what Git owns. Edit: declared state.
    Destroy: loses data or position. */
export type Tier = "operate" | "edit" | "destroy"

export type Refusal = "anonymous" | "notEnabled" | "forbidden" | "owned"

/** What a write would be in Git, or as the REST call it performs. */
export interface Equivalent {
  language: "yaml" | "shell" | string
  text: string
  note: string
}

export interface Owned {
  /** Where the owned manifests live, as configured. */
  source: string | null
}

export interface WriteOption {
  operation: Operation
  tier: Tier
  /** Every gate agrees for this caller, now. */
  allowed: boolean
  refusedBy: Refusal | null
  reason: string | null
  equivalent: Equivalent
}

export interface WriteRequest {
  includeTasks?: boolean
  onlyFailed?: boolean
  /** edit: the whole config; `******` keeps a withheld value. */
  config?: Record<string, string>
  offsets?: StoredOffset[]
  /** Destroy tier: the connector's name, typed. */
  confirm?: string
  /** edit: validate against the plugin, apply nothing. */
  dryRun?: boolean
}

export interface CreateRequest {
  name: string
  config: Record<string, string>
  dryRun?: boolean
}

export interface ValidationError {
  key: string
  messages: string[]
}

export interface Validation {
  errorCount: number
  errors: ValidationError[]
}

export interface WriteResult {
  /** Joins the intent and outcome lines in the audit. null for a dry run. */
  auditId: string | null
  /** Whether anything changed. false for a dry run. */
  applied: boolean
  operation: Operation
  connector: string
  equivalent: Equivalent
  validation: Validation | null
  /** For an edit or a create: the config as sent, redacted. */
  config: RedactedConfig | null
}
