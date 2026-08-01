import { useEffect, useState, type ReactNode } from "react";
import type {
  ClusterCard,
  ClusterStatus,
  FeatureEntry,
  Lag,
  Partition,
  ResourceError,
} from "./api/types";

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

export function Card({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={`bg-surface-raised border border-line rounded-md ${className}`}
    >
      {children}
    </div>
  );
}

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
      <div className="flex items-baseline justify-between gap-4 mb-3">
        <h2 className="text-[15px] font-semibold tracking-[-0.01em]">{title}</h2>
        {actions}
      </div>
      {children}
    </section>
  );
}

export function Mono({ children }: { children: ReactNode }) {
  // Everything a broker said verbatim is mono; everything kaas-ui wrote is
  // sans. The split tells the reader which strings they can paste into
  // kafka-configs.sh.
  return <span className="font-mono text-[13px] text-ink-muted">{children}</span>;
}

export function Empty({ children }: { children: ReactNode }) {
  return (
    <div className="text-ink-muted text-[13px] py-8 text-center border border-dashed border-line rounded-md">
      {children}
    </div>
  );
}

export function Spinner({ label = "loading" }: { label?: string }) {
  return <div className="text-ink-faint text-[13px] py-8">{label}…</div>;
}

/* ------------------------------------------------------------------ tables */

export function Table({ children }: { children: ReactNode }) {
  return (
    <div className="overflow-x-auto border border-line rounded-md">
      <table className="w-full text-[13px] border-collapse">{children}</table>
    </div>
  );
}

export function Th({
  children,
  align = "left",
}: {
  children: ReactNode;
  align?: "left" | "right";
}) {
  return (
    <th
      className={`${align === "right" ? "text-right" : "text-left"} font-semibold text-ink-muted bg-surface-sunken px-3 py-2 border-b border-line whitespace-nowrap`}
    >
      {children}
    </th>
  );
}

export function Td({
  children,
  align = "left",
  className = "",
}: {
  children: ReactNode;
  align?: "left" | "right";
  className?: string;
}) {
  return (
    <td
      className={`${align === "right" ? "text-right" : "text-left"} px-3 py-2 border-b border-line align-top ${className}`}
    >
      {children}
    </td>
  );
}

/* ------------------------------------------------------------------ status */

const STATUS_TONE: Record<ClusterStatus, string> = {
  ready: "bg-ok",
  connecting: "bg-warn",
  unreachable: "bg-danger",
};

/** Never colour alone: a red/green dashboard is useless to ~8% of men. */
const STATUS_GLYPH: Record<ClusterStatus, string> = {
  ready: "●",
  connecting: "◐",
  unreachable: "✕",
};

export function StatusBadge({ status }: { status: ClusterStatus }) {
  return (
    <span className="inline-flex items-center gap-1.5 text-[12px] font-medium">
      <span
        aria-hidden
        className={`inline-block w-2 h-2 rounded-full ${STATUS_TONE[status]}`}
      />
      <span>
        {STATUS_GLYPH[status]} {status}
      </span>
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

export function ClusterChip({
  id,
  labels,
  size = "normal",
}: {
  id: string;
  labels?: Record<string, string>;
  size?: "normal" | "small";
}) {
  // prod must not look like anything else, whatever its id hashes to.
  const isProd = labels?.env === "prod";
  const tone = CHIP_RAMP[hash(id) % CHIP_RAMP.length]!;
  const style = isProd
    ? { background: "var(--color-danger-soft)", color: "var(--color-danger)", borderColor: "var(--color-danger)" }
    : { background: tone.bg, color: tone.fg, borderColor: "transparent" };

  return (
    <span
      style={style}
      className={`inline-flex items-center gap-1.5 border rounded-sm font-medium ${
        size === "small" ? "text-[11px] px-1.5 py-0.5" : "text-[12px] px-2 py-1"
      }`}
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
    <span
      className={`text-[12px] ${stale ? "text-warn-ink font-medium" : "text-ink-faint"}`}
      title={stale ? "older than this cluster's staleness ceiling" : undefined}
    >
      as of {duration(age)} ago
    </span>
  );
}

/* ------------------------------------------------------------------ errors */

/** `ErrorCode::Unknown(30000)` — the number is the only searchable thing. */
export function UnknownCodeChip({ code }: { code: number }) {
  return (
    <span
      className="font-mono text-[12px] px-1.5 py-0.5 rounded-sm"
      style={{ background: "var(--color-warn-soft)", color: "var(--color-warn-ink)" }}
      title="this build has no name for this error code"
    >
      code {code}
    </span>
  );
}

/**
 * The per-resource failures that rode along with a successful request.
 *
 * These are data, not a failed request: the page renders and these say which
 * parts of it did not.
 */
export function ErrorChips({ errors }: { errors: ResourceError[] }) {
  if (errors.length === 0) return null;
  return (
    <div className="flex flex-wrap gap-2 mb-4">
      {errors.map((error, index) => (
        <span
          key={`${error.resource}-${index}`}
          title={error.message}
          className="inline-flex items-center gap-2 text-[12px] px-2 py-1 rounded-sm border"
          style={{
            background: "var(--color-warn-soft)",
            color: "var(--color-warn-ink)",
            borderColor: "var(--color-accent-edge)",
          }}
        >
          <span className="font-mono">{error.resource}</span>
          {error.code ? (
            <span className="font-mono opacity-80">{error.code}</span>
          ) : error.codeNumber !== null ? (
            <UnknownCodeChip code={error.codeNumber} />
          ) : (
            <span className="opacity-80">{error.kind}</span>
          )}
        </span>
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
    <Card className="p-5 max-w-2xl">
      <div className="flex items-baseline justify-between gap-4 border-b border-line pb-2 mb-3">
        <h3 className="font-semibold text-[15px]">{api}</h3>
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
      <p className="text-[13px] text-ink-muted mt-4">
        {broker === null
          ? `This cluster does not answer ${api}, so ${what ?? "this view"} has nothing behind it. The same URL against a cluster that does will render normally.`
          : ours === null
            ? `This build of kaas-ui has no schema for ${api}. The cluster is ahead of the codec; upgrading kaas-ui is what fixes it.`
            : `The versions do not overlap: the cluster speaks ${range(broker)} and kaas-ui speaks ${range(ours)}.`}
      </p>
    </Card>
  );
}

/* --------------------------------------------------------------- kafka bits */

/** Three states that must not all render as `0`, plus "not known". */
export function LagCell({ lag }: { lag: Lag }) {
  switch (lag.state) {
    case "noCommit":
      return (
        <span className="text-ink-faint" title="the group has never committed here">
          —
        </span>
      );
    case "emptyPartition":
      return (
        <span className="text-ink-faint" title="the partition is empty">
          ∅
        </span>
      );
    case "caughtUp":
      return (
        <span className="text-ok font-medium" title="committed at the log end">
          0
        </span>
      );
    case "lagging":
      return (
        <span className="font-mono text-warn-ink" title="records behind the log end">
          {count(lag.records)}
        </span>
      );
    case "unknown":
      return (
        <span className="text-ink-faint" title="the log end could not be read">
          ?
        </span>
      );
  }
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
        style: { background: "var(--color-danger-soft)", color: "var(--color-danger)" },
        title: "offline replica",
      };
    }
    if (!partition.isr.includes(broker)) {
      return {
        label: "△",
        style: { background: "var(--color-warn-soft)", color: "var(--color-warn-ink)" },
        title: "out of sync",
      };
    }
    if (partition.leader === broker) {
      return {
        label: "L",
        style: { background: "var(--color-accent)", color: "#3B2E2A" },
        title: "leader",
      };
    }
    return {
      label: "·",
      style: { background: "var(--color-ok-soft)", color: "var(--color-ok)" },
      title: "in-sync follower",
    };
  };

  return (
    <div className="overflow-x-auto">
      <table className="border-collapse text-[12px]">
        <thead>
          <tr>
            <th className="text-left text-ink-muted font-semibold px-2 py-1">p</th>
            {brokerIds.map((broker) => (
              <th key={broker} className="px-2 py-1 text-ink-muted font-mono font-normal">
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
                      className="w-6 h-5 grid place-items-center rounded-[2px] font-mono"
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
        <p className="text-[12px] text-ink-faint mt-2">
          showing the first {shown.length} of {partitions.length} partitions
        </p>
      ) : null}
      <div className="flex gap-4 mt-3 text-[12px] text-ink-muted">
        <Legend fill="var(--color-accent)" glyph="L" label="leader" />
        <Legend fill="var(--color-ok-soft)" glyph="·" label="in sync" />
        <Legend fill="var(--color-warn-soft)" glyph="△" label="out of sync" />
        <Legend fill="var(--color-danger-soft)" glyph="✕" label="offline" />
      </div>
    </div>
  );
}

function Legend({
  fill,
  glyph,
  label,
}: {
  fill: string;
  glyph: string;
  label: string;
}) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span
        style={{ background: fill }}
        className="w-4 h-4 grid place-items-center rounded-[2px] font-mono text-[10px]"
      >
        {glyph}
      </span>
      {label}
    </span>
  );
}

/** The summary line a fleet card and a cluster header both want. */
export function ClusterCounts({ card }: { card: ClusterCard }) {
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
  const colour =
    tone === "danger" ? "text-danger" : tone === "warn" ? "text-warn-ink" : "";
  return (
    <div>
      <dt className="text-[12px] text-ink-muted">{label}</dt>
      <dd className={`font-mono text-[15px] ${colour}`}>
        {value}
        {note ? <span className="text-[11px] text-ink-faint ml-1.5">{note}</span> : null}
      </dd>
    </div>
  );
}

/** Look a feature up in the capability answer. */
export function featureState(
  features: FeatureEntry[] | undefined,
  feature: FeatureEntry["feature"],
): FeatureEntry | undefined {
  return features?.find((entry) => entry.feature === feature);
}
