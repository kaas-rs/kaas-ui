import { AlertTriangle, Check, CircleDashed, X } from "lucide-react"

import { cn } from "@/lib/utils"
import type { ClusterStatus, RegistryStatus } from "@/api/types"

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
