import { useEffect, useState, type ReactNode } from "react"
import {
  AlertTriangle,
  Boxes,
  Cable,
  Check,
  CircleDashed,
  Database,
  Globe,
  Radio,
  Server,
  X,
} from "lucide-react"
import type { LucideIcon } from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"
import type {
  ClusterCard as ClusterCardData,
  ClusterStatus,
  FeatureEntry,
  Lag,
  Partition,
  RegistryStatus,
  ResourceError,
  ResourceKind,
} from "@/api/types"

/**
 * What a Kafka cluster looks like wherever one is listed.
 *
 * Here rather than at each call site so the nav and the fleet cannot drift
 * into two different pictures of the same thing — and so that the one glyph
 * that means "cluster" is never quietly reused for a resource below.
 */
export const CLUSTER_ICON: LucideIcon = Server

/**
 * Icon and wording per non-cluster resource kind.
 *
 * One table, two readers: the fleet card and the sidebar. A registry that is a
 * cylinder on one screen and a box on the other is a second thing to learn.
 */
export const RESOURCE_KINDS: Record<
  ResourceKind,
  { icon: LucideIcon; label: string }
> = {
  schema_registry: { icon: Database, label: "schema registry" },
  mqtt_broker: { icon: Radio, label: "MQTT broker" },
  kafka_connect: { icon: Cable, label: "Kafka Connect" },
  rest_proxy: { icon: Globe, label: "REST proxy" },
  other: { icon: Boxes, label: "resource" },
}

/**
 * The order kinds appear in, nearest the brokers first: the registry every
 * cluster in the environment reads, then the two things that sit directly in
 * front of one, then a broker of another protocol entirely, then the rest.
 */
const KIND_ORDER: ResourceKind[] = [
  "schema_registry",
  "kafka_connect",
  "rest_proxy",
  "mqtt_broker",
  "other",
]

/**
 * Group a mixed list so like kinds sit together.
 *
 * Stable, so the configured order survives *within* a kind — three registries
 * stay in the order someone wrote them. Used by both the nav and the fleet, so
 * the two never disagree about what comes after what.
 */
export function byResourceKind<T extends { kind: ResourceKind }>(
  resources: T[]
): T[] {
  const rank = (kind: ResourceKind) => {
    const index = KIND_ORDER.indexOf(kind)
    return index === -1 ? KIND_ORDER.length : index
  }
  return [...resources].sort((a, b) => rank(a.kind) - rank(b.kind))
}

/* ------------------------------------------------------------------ format */

export function bytes(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—"
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"]
  let size = value
  let unit = 0
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024
    unit += 1
  }
  return `${size < 10 && unit > 0 ? size.toFixed(1) : Math.round(size)} ${units[unit]}`
}

export function count(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—"
  return value.toLocaleString()
}

export function duration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)}ms`
  const seconds = ms / 1000
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)}s`
  const minutes = seconds / 60
  if (minutes < 60) return `${Math.round(minutes)}m`
  return `${Math.round(minutes / 60)}h`
}

/* -------------------------------------------------------------- primitives */

export function Section({
  title,
  actions,
  children,
}: {
  title: string
  actions?: ReactNode
  children: ReactNode
}) {
  return (
    <section className="mb-8">
      <div className="mb-3 flex items-baseline justify-between gap-4">
        <h2 className="text-[15px] font-semibold tracking-[-0.01em]">
          {title}
        </h2>
        {actions}
      </div>
      {children}
    </section>
  )
}

/**
 * Everything a broker said verbatim is mono; everything kaas-ui wrote is sans.
 *
 * That split does real work: it tells the reader at a glance which strings they
 * can paste into `kafka-configs.sh`.
 */
export function Mono({ children }: { children: ReactNode }) {
  return (
    <span className="font-mono text-[13px] text-ink-muted">{children}</span>
  )
}

export function Empty({ children }: { children: ReactNode }) {
  return (
    <div className="rounded-md border border-dashed py-8 text-center text-[13px] text-ink-muted">
      {children}
    </div>
  )
}

export function Spinner({ label = "loading" }: { label?: string }) {
  return <div className="py-8 text-[13px] text-ink-faint">{label}…</div>
}

/* ------------------------------------------------------------------ status */

/** Never colour alone: a red/green dashboard is useless to ~8% of men. */
const STATUS: Record<
  ClusterStatus,
  { dot: string; icon: typeof Check; label: string }
> = {
  ready: { dot: "bg-ok", icon: Check, label: "ready" },
  connecting: { dot: "bg-warn", icon: CircleDashed, label: "connecting" },
  unreachable: { dot: "bg-danger", icon: X, label: "unreachable" },
}

export function StatusBadge({ status }: { status: ClusterStatus }) {
  const { dot, icon: Icon, label } = STATUS[status]
  return (
    <span className="inline-flex items-center gap-1.5 text-[12px] font-medium">
      <span
        aria-hidden
        className={cn("inline-block size-2 rounded-full", dot)}
      />
      <Icon aria-hidden className="size-3.5" />
      {label}
    </span>
  )
}

/**
 * The same badge for a registry, whose states are four rather than three.
 *
 * Deliberately the same shape as [`StatusBadge`] — dot, glyph, word — because
 * the two sit side by side on the fleet, and a reader scanning a row of cards
 * should not have to learn a second vocabulary of health. The *states* differ
 * because a registry's do: `unprobed` is nothing having needed it yet, which
 * no cluster has, and `unreachable` and `misconfigured` are kept apart here in
 * colour as they are everywhere else — one is somebody else's outage and heals
 * on its own, the other is a line in the configuration file and does not.
 */
const REGISTRY_STATUS: Record<
  RegistryStatus,
  { dot: string; icon: typeof Check; label: string; tone?: string }
> = {
  ready: { dot: "bg-ok", icon: Check, label: "ready" },
  unprobed: {
    dot: "bg-ink-faint/40",
    icon: CircleDashed,
    label: "unprobed",
    tone: "text-ink-faint",
  },
  unreachable: {
    dot: "bg-warn",
    icon: AlertTriangle,
    label: "unreachable",
    tone: "text-warn-ink",
  },
  misconfigured: {
    dot: "bg-danger",
    icon: X,
    label: "misconfigured",
    tone: "text-danger",
  },
}

export function RegistryStatusBadge({
  status,
  title,
}: {
  status: RegistryStatus
  /** The fault, where there is one — the badge is where a reader points. */
  title?: string
}) {
  const { dot, icon: Icon, label, tone } = REGISTRY_STATUS[status]
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center gap-1.5 text-[12px] font-medium",
        tone
      )}
      title={title}
    >
      <span
        aria-hidden
        className={cn("inline-block size-2 rounded-full", dot)}
      />
      <Icon aria-hidden className="size-3.5" />
      {label}
    </span>
  )
}

/** Colour derived from the cluster id, so identity is stable across pages. */
const CHIP_RAMP = [
  { bg: "#D8C6AE", fg: "#4A3418" },
  { bg: "#C9CDB4", fg: "#33391C" },
  { bg: "#E0C2A8", fg: "#553017" },
  { bg: "#C4CBD1", fg: "#28323A" },
  { bg: "#D5BFC4", fg: "#4A2A31" },
  { bg: "#CBC3D6", fg: "#332A46" },
]

function hash(text: string): number {
  let value = 0
  for (const character of text) {
    value = (value * 31 + character.codePointAt(0)!) >>> 0
  }
  return value
}

export function clusterTone(id: string, labels?: Record<string, string>) {
  // prod must not look like anything else, whatever its id happens to hash to.
  if (labels?.env === "prod") {
    return {
      bg: "var(--danger-soft)",
      fg: "var(--danger)",
      edge: "var(--danger)",
    }
  }
  const tone = CHIP_RAMP[hash(id) % CHIP_RAMP.length]!
  return { bg: tone.bg, fg: tone.fg, edge: "transparent" }
}

export function ClusterChip({
  id,
  labels,
  size = "normal",
}: {
  id: string
  labels?: Record<string, string>
  size?: "normal" | "small"
}) {
  const tone = clusterTone(id, labels)
  return (
    <span
      style={{ background: tone.bg, color: tone.fg, borderColor: tone.edge }}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-sm border font-medium",
        size === "small" ? "px-1.5 py-0.5 text-[11px]" : "px-2 py-1 text-[12px]"
      )}
    >
      <span aria-hidden>●</span>
      <span className="font-mono">{id}</span>
      {labels?.kind ? <span className="opacity-70">{labels.kind}</span> : null}
    </span>
  )
}

/** "as of 4s ago", ticking, warm past the configured staleness ceiling. */
export function SnapshotAge({
  ageMs,
  asOfMs,
  maxStalenessMs,
}: {
  ageMs: number | null | undefined
  /**
   * When `ageMs` was true — the owning query's `dataUpdatedAt`. The tick is
   * anchored here, not at mount, so each refetch resets the display instead
   * of compounding with how long the component has been on screen.
   */
  asOfMs: number
  maxStalenessMs?: number
}) {
  const [now, setNow] = useState(() => Date.now())

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [])

  if (ageMs === null || ageMs === undefined) return null
  // `now` only advances once a second, so right after a refetch it can sit
  // behind `asOfMs`; clamp rather than briefly understate the age.
  const age = ageMs + Math.max(0, now - asOfMs)
  const stale = maxStalenessMs !== undefined && age > maxStalenessMs

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className={cn(
            "text-[12px]",
            stale ? "font-medium text-warn-ink" : "text-ink-faint"
          )}
        >
          as of {duration(age)} ago
        </span>
      </TooltipTrigger>
      <TooltipContent>
        {stale
          ? "older than this cluster's staleness ceiling"
          : "age of the metadata snapshot this was built from"}
      </TooltipContent>
    </Tooltip>
  )
}

/* ------------------------------------------------------------------ errors */

/** `ErrorCode::Unknown(30000)` — the number is the only searchable thing. */
export function UnknownCodeChip({ code }: { code: number }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className="rounded-sm px-1.5 py-0.5 font-mono text-[12px]"
          style={{ background: "var(--warn-soft)", color: "var(--warn-ink)" }}
        >
          code {code}
        </span>
      </TooltipTrigger>
      <TooltipContent>
        this build has no name for this error code
      </TooltipContent>
    </Tooltip>
  )
}

/**
 * The per-resource failures that rode along with a successful request.
 *
 * These are data, not a failed request: the page renders, and these say which
 * parts of it did not.
 */
export function ErrorChips({ errors }: { errors: ResourceError[] }) {
  if (errors.length === 0) return null
  return (
    <div className="mb-4 flex flex-wrap gap-2">
      {errors.map((error, index) => (
        <Tooltip key={`${error.resource}-${index}`}>
          <TooltipTrigger asChild>
            <span
              className="inline-flex items-center gap-2 rounded-sm border px-2 py-1 text-[12px]"
              style={{
                background: "var(--warn-soft)",
                color: "var(--warn-ink)",
                borderColor: "var(--rust-edge)",
              }}
            >
              <AlertTriangle aria-hidden className="size-3.5" />
              <span className="font-mono">{error.resource}</span>
              {error.code ? (
                <span className="font-mono opacity-80">{error.code}</span>
              ) : error.codeNumber !== null ? (
                <span className="font-mono opacity-80">
                  code {error.codeNumber}
                </span>
              ) : (
                <span className="opacity-80">{error.kind}</span>
              )}
            </span>
          </TooltipTrigger>
          <TooltipContent className="max-w-md">{error.message}</TooltipContent>
        </Tooltip>
      ))}
    </div>
  )
}

/**
 * The degradation component.
 *
 * Shows the api name and *both* version ranges, laid out as a comparison
 * rather than prose, because the pair is the diagnosis: no broker range means
 * the cluster does not implement it, no range of ours means this build has no
 * schema for it, and two disjoint ranges mean the cluster is behind.
 */
export function UnsupportedApiPanel({
  api,
  apiKey,
  broker,
  ours,
  what,
}: {
  api: string
  apiKey: number
  broker: [number, number] | null
  ours: [number, number] | null
  what?: string
}) {
  const range = (value: [number, number] | null) =>
    value ? `v${value[0]} – v${value[1]}` : null

  return (
    <Card className="max-w-2xl gap-0 p-5">
      <div className="mb-3 flex items-baseline justify-between gap-4 border-b pb-2">
        <h3 className="text-[15px] font-semibold">{api}</h3>
        <Mono>api key {apiKey}</Mono>
      </div>
      <dl className="grid grid-cols-[10rem_1fr] gap-y-2 text-[13px]">
        <dt className="text-ink-muted">this cluster</dt>
        <dd className="font-mono">
          {range(broker) ?? (
            <span className="text-danger">does not implement it</span>
          )}
        </dd>
        <dt className="text-ink-muted">kaas-ui speaks</dt>
        <dd className="font-mono">
          {range(ours) ?? (
            <span className="text-warn-ink">no schema in this build</span>
          )}
        </dd>
      </dl>
      <p className="mt-4 text-[13px] text-ink-muted">
        {broker === null
          ? `This cluster does not answer ${api}, so ${what ?? "this view"} has nothing behind it. The same URL against a cluster that does will render normally.`
          : ours === null
            ? `This build of kaas-ui has no schema for ${api}. The cluster is ahead of the codec; upgrading kaas-ui is what fixes it.`
            : `The versions do not overlap: the cluster speaks ${range(broker)} and kaas-ui speaks ${range(ours)}.`}
      </p>
    </Card>
  )
}

/* -------------------------------------------------------------- kafka bits */

/** Three states that must not all render as `0`, plus "not known". */
export function LagCell({ lag }: { lag: Lag }) {
  const render = () => {
    switch (lag.state) {
      case "noCommit":
        return {
          text: "—",
          className: "text-ink-faint",
          why: "the group has never committed here",
        }
      case "emptyPartition":
        return {
          text: "∅",
          className: "text-ink-faint",
          why: "the partition is empty",
        }
      case "caughtUp":
        return {
          text: "0",
          className: "font-medium text-ok",
          why: "committed at the log end",
        }
      case "lagging":
        return {
          text: count(lag.records),
          className: "font-mono text-warn-ink",
          why: "records behind the log end",
        }
      case "unknown":
        return {
          text: "?",
          className: "text-ink-faint",
          why: "the log end could not be read",
        }
    }
  }

  const { text, className, why } = render()
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className={className}>{text}</span>
      </TooltipTrigger>
      <TooltipContent>{why}</TooltipContent>
    </Tooltip>
  )
}

/**
 * One placement cell: what broker `broker` holds of partition `partition`.
 *
 * Four states, four fills, and under-replicated is deliberately not the same
 * colour as offline — a short ISR and a replica on a dead broker are different
 * problems with different fixes.
 *
 * `preferred` is the fifth thing, and it is why the table no longer needs a
 * `replicas` column. Kafka's replica list is **ordered**, and `replicas[0]` is
 * the *preferred* leader — the broker leadership returns to when it is
 * rebalanced. A grid of unordered glyphs cannot say that, so the preferred
 * broker's cell is outlined. When the outline and the `L` are on the same cell
 * the partition is where it wants to be; when they are apart, leadership has
 * moved and a preferred-leader election would move it back. That condition was
 * invisible in both of the views this replaced.
 */
export function placementCell(
  partition: Partition,
  broker: number
): {
  label: string
  style: Record<string, string>
  title: string
  preferred: boolean
} {
  const preferred = partition.replicas[0] === broker
  if (!partition.replicas.includes(broker)) {
    return { label: "", style: {}, title: "no replica", preferred: false }
  }
  if (partition.offlineReplicas.includes(broker)) {
    return {
      label: "✕",
      style: { background: "var(--danger-soft)", color: "var(--danger)" },
      title: "offline replica",
      preferred,
    }
  }
  if (!partition.isr.includes(broker)) {
    return {
      label: "△",
      style: { background: "var(--warn-soft)", color: "var(--warn-ink)" },
      title: "out of sync",
      preferred,
    }
  }
  if (partition.leader === broker) {
    return {
      label: "L",
      style: { background: "var(--rust)", color: "#3B2E2A" },
      title: preferred ? "leader, on its preferred broker" : "leader",
      preferred,
    }
  }
  return {
    label: "·",
    style: { background: "var(--ok-soft)", color: "var(--ok)" },
    title: preferred
      ? "in-sync follower, and the preferred leader"
      : "in-sync follower",
    preferred,
  }
}

/** What the glyphs in the placement columns mean. */
export function PlacementLegend() {
  return (
    <div className="text-ink-muted flex flex-wrap items-center gap-4 text-[12px]">
      <Legend fill="var(--rust)" glyph="L" label="leader" />
      <Legend fill="var(--ok-soft)" glyph="·" label="in sync" />
      <Legend fill="var(--warn-soft)" glyph="△" label="out of sync" />
      <Legend fill="var(--danger-soft)" glyph="✕" label="offline" />
      <span className="inline-flex items-center gap-1.5">
        <span className="border-ink-muted grid size-4 place-items-center rounded-[2px] border-2" />
        preferred leader
      </span>
      <span className="text-ink-faint">empty — no replica there</span>
    </div>
  )
}

function Legend({
  fill,
  glyph,
  label,
}: {
  fill: string
  glyph: string
  label: string
}) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span
        style={{ background: fill }}
        className="grid size-4 place-items-center rounded-[2px] font-mono text-[10px]"
      >
        {glyph}
      </span>
      {label}
    </span>
  )
}

/** The summary a fleet card and a cluster header both want. */
export function ClusterCounts({ card }: { card: ClusterCardData }) {
  return (
    <dl className="grid grid-cols-3 gap-x-4 gap-y-2 text-[13px]">
      <Stat label="brokers" value={count(card.brokerCount)} />
      <Stat
        label="topics"
        value={count(card.topicCount)}
        note={
          card.internalTopicCount
            ? `${card.internalTopicCount} internal`
            : undefined
        }
      />
      <Stat label="partitions" value={count(card.partitionCount)} />
      <Stat
        label="offline"
        value={count(card.offlinePartitionCount)}
        tone={card.offlinePartitionCount > 0 ? "danger" : undefined}
      />
      <Stat
        label="under-replicated"
        value={count(card.underReplicatedPartitionCount)}
        tone={card.underReplicatedPartitionCount > 0 ? "warn" : undefined}
      />
      <Stat
        label="controller"
        value={card.controllerId === null ? "—" : String(card.controllerId)}
      />
    </dl>
  )
}

/**
 * What a schema registry holds, in the shape [`ClusterCounts`] uses.
 *
 * Three numbers off one summary, which is why the card asks for `limit=0`: the
 * counts describe the whole listing and the page describes fifty rows, so
 * nothing here needs a single subject to travel. The columns that *do* need
 * one — id, format, version, compatibility — are two registry calls each and
 * stay on the page that has a table to put them in.
 *
 * `topics` is the interesting one: what these subjects cover, which is not how
 * many topics exist on any cluster that reads them.
 */
export function RegistryCounts({
  summary,
  pending,
}: {
  /** `null` until the listing answers, which is `·` rather than `0`. */
  summary: {
    total: number
    topics: number
    dangling: number | null
  } | null
  pending: boolean
}) {
  const show = (value: number | null | undefined) =>
    value === undefined || value === null ? (pending ? "·" : "—") : count(value)

  return (
    <dl className="grid grid-cols-3 gap-x-4 gap-y-2 text-[13px]">
      <Stat
        label="subjects"
        value={show(summary?.total)}
        hint="Everything registered here, across every naming strategy."
      />
      <Stat
        label="topics"
        value={show(summary?.topics)}
        hint="Distinct topics these subject names resolve to, read off the names alone."
      />
      {/* Not toned. A schema kept after its topic went is ordinary
          housekeeping on most fleets, and a number that is amber on every
          registry teaches people to stop reading the ones that are not. */}
      <Stat
        label="dangling"
        value={show(summary?.dangling)}
        hint="Subjects naming a topic that is on no cluster reading this registry — deleting a topic never touches the registry, so its schema stays. Shown as — while a cluster that reads this registry is disconnected, and where none does."
      />
    </dl>
  )
}

export function Stat({
  label,
  value,
  note,
  tone,
  hint,
}: {
  label: string
  value: string
  note?: string
  tone?: "warn" | "danger"
  /** One line on hover, for a label with a plausible wrong reading. */
  hint?: string
}) {
  return (
    <div>
      <dt className="text-[12px] text-ink-muted">
        {hint ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="cursor-help decoration-dotted underline-offset-4 hover:underline">
                {label}
              </span>
            </TooltipTrigger>
            <TooltipContent>{hint}</TooltipContent>
          </Tooltip>
        ) : (
          label
        )}
      </dt>
      <dd
        className={cn(
          "font-mono text-[15px]",
          tone === "danger" && "text-danger",
          tone === "warn" && "text-warn-ink"
        )}
      >
        {value}
        {note ? (
          <span className="ml-1.5 text-[11px] text-ink-faint">{note}</span>
        ) : null}
      </dd>
    </div>
  )
}

/** A feature the cluster does or does not have. */
export function FeatureBadge({ entry }: { entry: FeatureEntry }) {
  if (entry.state === "available") {
    return (
      <Badge variant="outline" className="text-ok">
        <Check aria-hidden className="size-3" />
        available
      </Badge>
    )
  }
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge variant="outline" className="font-mono text-ink-faint">
          <X aria-hidden className="size-3" />
          {entry.api}
        </Badge>
      </TooltipTrigger>
      <TooltipContent>
        {entry.api} (key {entry.apiKey}): broker{" "}
        {entry.broker
          ? `v${entry.broker[0]}–v${entry.broker[1]}`
          : "does not implement it"}
        , kaas-ui{" "}
        {entry.ours ? `v${entry.ours[0]}–v${entry.ours[1]}` : "has no schema"}
      </TooltipContent>
    </Tooltip>
  )
}

/** Look a feature up in the capability answer. */
export function featureState(
  features: FeatureEntry[] | undefined,
  feature: FeatureEntry["feature"]
): FeatureEntry | undefined {
  return features?.find((entry) => entry.feature === feature)
}
