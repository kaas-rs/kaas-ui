import { TableHead } from "@/components/ui/table"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"

/**
 * A column header that says what its column means.
 *
 * Every one of these is a registry term with a precise meaning and a plausible
 * wrong reading — `id` is a counter shared with every other subject, not this
 * subject's own numbering, and `compatibility` is a rule about the *next*
 * version rather than a verdict on this one. One line each, on hover, is
 * cheaper than a legend nobody scrolls to. The same shape the partition table
 * uses, for the same reason.
 */
export function SortableHead({
  label,
  hint,
  right,
  onClick,
}: {
  label: string
  hint: string
  right?: boolean
  /** Present on the one column that sorts; the header becomes the control. */
  onClick: () => void
}) {
  return (
    <TableHead className={cn(right && "text-right")}>
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            onClick={onClick}
            className="cursor-pointer decoration-dotted underline-offset-4 hover:underline"
          >
            {label}
          </button>
        </TooltipTrigger>
        <TooltipContent>{hint}</TooltipContent>
      </Tooltip>
    </TableHead>
  )
}
