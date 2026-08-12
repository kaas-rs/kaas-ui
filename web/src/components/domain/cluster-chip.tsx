import { cn } from "@/lib/utils"

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
