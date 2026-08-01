import { useEffect, useState, type ReactNode } from "react";
import { AlertTriangle, Check, CircleDashed, X } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type {
  ClusterCard as ClusterCardData,
  ClusterStatus,
  FeatureEntry,
  Lag,
  Partition,
  ResourceError,
} from "@/api/types";

/* ------------------------------------------------------------------ format */

export function bytes(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—";
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size < 10 && unit > 0 ? size.toFixed(1) : Math.round(size)} ${units[unit]}`;
}

export function count(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—";
  return value.toLocaleString();
}

export function duration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)}ms`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)}s`;
  const minutes = seconds / 60;
  if (minutes < 60) return `${Math.round(minutes)}m`;
  return `${Math.round(minutes / 60)}h`;
}

/* -------------------------------------------------------------- primitives */

export function Section({
  title,
  actions,
  children,
}: {
  title: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="mb-8">
      <div className="mb-3 flex items-baseline justify-between gap-4">
        <h2 className="text-[15px] font-semibold tracking-[-0.01em]">{title}</h2>
        {actions}
      </div>
      {children}
    </section>
  );
}

/**
 * Everything a broker said verbatim is mono; everything kaas-ui wrote is sans.
 *
 * That split does real work: it tells the reader at a glance which strings they
 * can paste into `kafka-configs.sh`.
 */
export function Mono({ children }: { children: ReactNode }) {
  return <span className="font-mono text-[13px] text-ink-muted">{children}</span>;
}

export function Empty({ children }: { children: ReactNode }) {
  return (
    <div className="rounded-md border border-dashed py-8 text-center text-[13px] text-ink-muted">
      {children}
    </div>
  );
}

export function Spinner({ label = "loading" }: { label?: string }) {
  return <div className="py-8 text-[13px] text-ink-faint">{label}…</div>;
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
};

export function StatusBadge({ status }: { status: ClusterStatus }) {
  const { dot, icon: Icon, label } = STATUS[status];
  return (
    <span className="inline-flex items-center gap-1.5 text-[12px] font-medium">
      <span aria-hidden className={cn("inline-block size-2 rounded-full", dot)} />
      <Icon aria-hidden className="size-3.5" />
      {label}
    </span>
  );
}

/** Colour derived from the cluster id, so identity is stable across pages. */
const CHIP_RAMP = [
  { bg: "#D8C6AE", fg: "#4A3418" },
  { bg: "#C9CDB4", fg: "#33391C" },
  { bg: "#E0C2A8", fg: "#553017" },
  { bg: "#C4CBD1", fg: "#28323A" },
  { bg: "#D5BFC4", fg: "#4A2A31" },
  { bg: "#CBC3D6", fg: "#332A46" },
];

function hash(text: string): number {
  let value = 0;
  for (const character of text) {
    value = (value * 31 + character.codePointAt(0)!) >>> 0;
  }
  return value;
}

export function clusterTone(id: string, labels?: Record<string, string>) {
  // prod must not look like anything else, whatever its id happens to hash to.
  if (labels?.env === "prod") {
    return { bg: "var(--danger-soft)", fg: "var(--danger)", edge: "var(--danger)" };
  }
  const tone = CHIP_RAMP[hash(id) % CHIP_RAMP.length]!;
  return { bg: tone.bg, fg: tone.fg, edge: "transparent" };
}

export function ClusterChip({
  id,
  labels,
  size = "normal",
}: {
  id: string;
  labels?: Record<string, string>;
  size?: "normal" | "small";
}) {
  const tone = clusterTone(id, labels);
  return (
    <span
      style={{ background: tone.bg, color: tone.fg, borderColor: tone.edge }}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-sm border font-medium",
        size === "small" ? "px-1.5 py-0.5 text-[11px]" : "px-2 py-1 text-[12px]",
      )}
    >
      <span aria-hidden>●</span>
      <span className="font-mono">{id}</span>
      {labels?.kind ? <span className="opacity-70">{labels.kind}</span> : null}
    </span>
  );
}

/** "as of 4s ago", ticking, warm past the configured staleness ceiling. */
export function SnapshotAge({
  ageMs,
  maxStalenessMs,
}: {
  ageMs: number | null | undefined;
  maxStalenessMs?: number;
}) {
  const [now, setNow] = useState(() => Date.now());
  const [base] = useState(() => Date.now());

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);

  if (ageMs === null || ageMs === undefined) return null;
  const age = ageMs + (now - base);
  const stale = maxStalenessMs !== undefined && age > maxStalenessMs;

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className={cn(
            "text-[12px]",
            stale ? "font-medium text-warn-ink" : "text-ink-faint",
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
  );
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
      <TooltipContent>this build has no name for this error code</TooltipContent>
    </Tooltip>
  );
}

/**
 * The per-resource failures that rode along with a successful request.
 *
 * These are data, not a failed request: the page renders, and these say which
 * parts of it did not.
 */
export function ErrorChips({ errors }: { errors: ResourceError[] }) {
  if (errors.length === 0) return null;
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
                <span className="font-mono opacity-80">code {error.codeNumber}</span>
              ) : (
                <span className="opacity-80">{error.kind}</span>
              )}
            </span>
          </TooltipTrigger>
          <TooltipContent className="max-w-md">{error.message}</TooltipContent>
        </Tooltip>
      ))}
    </div>
  );
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
  api: string;
  apiKey: number;
  broker: [number, number] | null;
  ours: [number, number] | null;
  what?: string;
}) {
  const range = (value: [number, number] | null) =>
    value ? `v${value[0]} – v${value[1]}` : null;

  return (
    <Card className="max-w-2xl gap-0 p-5">
      <div className="mb-3 flex items-baseline justify-between gap-4 border-b pb-2">
        <h3 className="text-[15px] font-semibold">{api}</h3>
        <Mono>api key {apiKey}</Mono>
      </div>
      <dl className="grid grid-cols-[10rem_1fr] gap-y-2 text-[13px]">
        <dt className="text-ink-muted">this cluster</dt>
        <dd className="font-mono">
          {range(broker) ?? <span className="text-danger">does not implement it</span>}
        </dd>
        <dt className="text-ink-muted">kaas-ui speaks</dt>
        <dd className="font-mono">
          {range(ours) ?? <span className="text-warn-ink">no schema in this build</span>}
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
  );
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
        };
      case "emptyPartition":
        return { text: "∅", className: "text-ink-faint", why: "the partition is empty" };
      case "caughtUp":
        return {
          text: "0",
          className: "font-medium text-ok",
          why: "committed at the log end",
        };
      case "lagging":
        return {
          text: count(lag.records),
          className: "font-mono text-warn-ink",
          why: "records behind the log end",
        };
      case "unknown":
        return {
          text: "?",
          className: "text-ink-faint",
          why: "the log end could not be read",
        };
    }
  };

  const { text, className, why } = render();
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className={className}>{text}</span>
      </TooltipTrigger>
      <TooltipContent>{why}</TooltipContent>
    </Tooltip>
  );
}

/**
 * Partitions down, brokers across. Four states, four fills, no legend needed
 * after five seconds of looking — and under-replicated is not the same colour
 * as offline, because a short ISR and a replica on a dead broker are different
 * problems.
 */
export function PartitionGrid({
  partitions,
  brokerIds,
}: {
  partitions: Partition[];
  brokerIds: number[];
}) {
  if (brokerIds.length === 0) return null;
  const shown = partitions.slice(0, 200);

  const cell = (partition: Partition, broker: number) => {
    if (!partition.replicas.includes(broker)) {
      return { label: "", style: {}, title: "no replica" };
    }
    if (partition.offlineReplicas.includes(broker)) {
      return {
        label: "✕",
        style: { background: "var(--danger-soft)", color: "var(--danger)" },
        title: "offline replica",
      };
    }
    if (!partition.isr.includes(broker)) {
      return {
        label: "△",
        style: { background: "var(--warn-soft)", color: "var(--warn-ink)" },
        title: "out of sync",
      };
    }
    if (partition.leader === broker) {
      return {
        label: "L",
        style: { background: "var(--rust)", color: "#3B2E2A" },
        title: "leader",
      };
    }
    return {
      label: "·",
      style: { background: "var(--ok-soft)", color: "var(--ok)" },
      title: "in-sync follower",
    };
  };

  return (
    <div className="overflow-x-auto">
      <table className="border-collapse text-[12px]">
        <thead>
          <tr>
            <th className="px-2 py-1 text-left font-semibold text-ink-muted">p</th>
            {brokerIds.map((broker) => (
              <th
                key={broker}
                className="px-2 py-1 font-mono font-normal text-ink-muted"
              >
                {broker}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {shown.map((partition) => (
            <tr key={partition.partition}>
              <td className="px-2 py-0.5 font-mono text-ink-muted">
                {partition.partition}
              </td>
              {brokerIds.map((broker) => {
                const { label, style, title } = cell(partition, broker);
                return (
                  <td key={broker} className="p-0.5">
                    <div
                      title={`p${partition.partition} on broker ${broker}: ${title}`}
                      style={style}
                      className="grid h-5 w-6 place-items-center rounded-[2px] font-mono"
                    >
                      {label}
                    </div>
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      {partitions.length > shown.length ? (
        <p className="mt-2 text-[12px] text-ink-faint">
          showing the first {shown.length} of {partitions.length} partitions
        </p>
      ) : null}
      <div className="mt-3 flex gap-4 text-[12px] text-ink-muted">
        <Legend fill="var(--rust)" glyph="L" label="leader" />
        <Legend fill="var(--ok-soft)" glyph="·" label="in sync" />
        <Legend fill="var(--warn-soft)" glyph="△" label="out of sync" />
        <Legend fill="var(--danger-soft)" glyph="✕" label="offline" />
      </div>
    </div>
  );
}

function Legend({ fill, glyph, label }: { fill: string; glyph: string; label: string }) {
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
  );
}

/** The summary a fleet card and a cluster header both want. */
export function ClusterCounts({ card }: { card: ClusterCardData }) {
  return (
    <dl className="grid grid-cols-3 gap-x-4 gap-y-2 text-[13px]">
      <Stat label="brokers" value={count(card.brokerCount)} />
      <Stat
        label="topics"
        value={count(card.topicCount)}
        note={card.internalTopicCount ? `${card.internalTopicCount} internal` : undefined}
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
  );
}

export function Stat({
  label,
  value,
  note,
  tone,
}: {
  label: string;
  value: string;
  note?: string;
  tone?: "warn" | "danger";
}) {
  return (
    <div>
      <dt className="text-[12px] text-ink-muted">{label}</dt>
      <dd
        className={cn(
          "font-mono text-[15px]",
          tone === "danger" && "text-danger",
          tone === "warn" && "text-warn-ink",
        )}
      >
        {value}
        {note ? <span className="ml-1.5 text-[11px] text-ink-faint">{note}</span> : null}
      </dd>
    </div>
  );
}

/** A feature the cluster does or does not have. */
export function FeatureBadge({ entry }: { entry: FeatureEntry }) {
  if (entry.state === "available") {
    return (
      <Badge variant="outline" className="text-ok">
        <Check aria-hidden className="size-3" />
        available
      </Badge>
    );
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
        , kaas-ui {entry.ours ? `v${entry.ours[0]}–v${entry.ours[1]}` : "has no schema"}
      </TooltipContent>
    </Tooltip>
  );
}

/** Look a feature up in the capability answer. */
export function featureState(
  features: FeatureEntry[] | undefined,
  feature: FeatureEntry["feature"],
): FeatureEntry | undefined {
  return features?.find((entry) => entry.feature === feature);
}
