import type { Partition } from "@/api/types"

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
