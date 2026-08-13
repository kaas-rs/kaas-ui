import { cn } from "@/lib/utils"
import type { ClusterStatus, RegistryStatus } from "@/api/types"

/**
 * Never colour alone: a red/green dashboard is useless to ~8% of men.
 *
 * The word carries that, which is why there is no glyph beside it. A dot, a
 * tick and the word "ready" are three renderings of one fact, and the two that
 * are not text are the two that add nothing for a reader who has the text —
 * the dot stays because it is what makes a column of cards scannable at a
 * distance the words are not readable from.
 */
const STATUS: Record<ClusterStatus, { dot: string; label: string }> = {
  ready: { dot: "bg-ok", label: "ready" },
  connecting: { dot: "bg-warn", label: "connecting" },
  unreachable: { dot: "bg-danger", label: "unreachable" },
}

export function StatusBadge({ status }: { status: ClusterStatus }) {
  const { dot, label } = STATUS[status]
  return (
    <span className="inline-flex items-center gap-1.5 text-[12px] font-medium">
      <span
        aria-hidden
        className={cn("inline-block size-2 rounded-full", dot)}
      />
      {label}
    </span>
  )
}

/**
 * The same badge for a registry, whose states are four rather than three.
 *
 * Deliberately the same shape as [`StatusBadge`] — dot, word — because the two
 * sit side by side on the fleet, and a reader scanning a row of cards should
 * not have to learn a second vocabulary of health. That is also why the glyph
 * left both at once: a badge with one and a badge without, in the same row, is
 * a difference that looks like it means something. The *states* differ because
 * a registry's do: `unprobed` is nothing having needed it yet, which no cluster
 * has, and `unreachable` and `misconfigured` are kept apart here in colour as
 * they are everywhere else — one is somebody else's outage and heals on its
 * own, the other is a line in the configuration file and does not.
 */
const REGISTRY_STATUS: Record<
  RegistryStatus,
  { dot: string; label: string; tone?: string }
> = {
  ready: { dot: "bg-ok", label: "ready" },
  unprobed: {
    dot: "bg-ink-faint/40",
    label: "unprobed",
    tone: "text-ink-faint",
  },
  unreachable: {
    dot: "bg-warn",
    label: "unreachable",
    tone: "text-warn-ink",
  },
  misconfigured: {
    dot: "bg-danger",
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
  const { dot, label, tone } = REGISTRY_STATUS[status]
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
      {label}
    </span>
  )
}
