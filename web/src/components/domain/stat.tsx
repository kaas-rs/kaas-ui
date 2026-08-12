import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"

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
