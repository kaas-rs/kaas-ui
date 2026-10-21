import type { ConnectorStatus, ConnectorType } from "@/api/types"
import { cn } from "@/lib/utils"

/**
 * A connector's folded state, as dot and word.
 *
 * The same shape as `StatusBadge`, and the same rule: never colour alone. What
 * this one adds is the distinction a red/green rendering destroys — **paused
 * and stopped are not failures**, somebody chose them, so they are neutral
 * rather than red. Degraded is warm because some data is still moving; failed
 * is red because none is.
 */
const STATUS: Record<
  ConnectorStatus,
  { dot: string; label: string; tone?: string; hint: string }
> = {
  running: {
    dot: "bg-ok",
    label: "running",
    hint: "the connector and every task are running",
  },
  degraded: {
    dot: "bg-warn",
    label: "degraded",
    tone: "text-warn-ink",
    hint: "running, with some tasks failed — part of the data is not moving",
  },
  failed: {
    dot: "bg-danger",
    label: "failed",
    tone: "text-danger",
    hint: "the connector failed, or every task did",
  },
  paused: {
    dot: "bg-ink-faint/60",
    label: "paused",
    tone: "text-ink-muted",
    hint: "paused on purpose: tasks keep their resources and process nothing",
  },
  stopped: {
    dot: "bg-ink-faint/60",
    label: "stopped",
    tone: "text-ink-muted",
    hint: "stopped on purpose: tasks are shut down",
  },
  unassigned: {
    dot: "bg-warn",
    label: "unassigned",
    tone: "text-warn-ink",
    hint: "not on a worker yet — usually a rebalance in progress",
  },
  restarting: {
    dot: "bg-warn",
    label: "restarting",
    tone: "text-warn-ink",
    hint: "restarting",
  },
  noTasks: {
    dot: "bg-warn",
    label: "no tasks",
    tone: "text-warn-ink",
    hint: "running with zero tasks: configured, and doing nothing",
  },
  unknown: {
    dot: "bg-ink-faint/40",
    label: "unknown",
    tone: "text-ink-faint",
    hint: "the worker reported a state this build does not know",
  },
}

export function ConnectorStatusBadge({ status }: { status: ConnectorStatus }) {
  const { dot, label, tone, hint } = STATUS[status]
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center gap-1.5 text-[12px] font-medium",
        tone
      )}
      title={hint}
    >
      <span
        aria-hidden
        className={cn("inline-block size-2 rounded-full", dot)}
      />
      {label}
    </span>
  )
}

/** One task's state, verbatim from the worker, coloured the same way. */
export function TaskState({ state }: { state: string | null }) {
  const tone =
    state === "RUNNING"
      ? "text-ok"
      : state === "FAILED"
        ? "text-danger"
        : state === "PAUSED" || state === "STOPPED"
          ? "text-ink-muted"
          : "text-warn-ink"
  return (
    <span className={cn("font-mono text-[12px] font-medium", tone)}>
      {state ?? "—"}
    </span>
  )
}

export function connectorTypeLabel(type: ConnectorType): string {
  return type === "unknown" ? "—" : type
}

/** The class's last segment — what anyone actually calls a connector class. */
export function shortClass(name: string | null): string {
  if (!name) return "—"
  const last = name.split(".").pop()
  return last || name
}
