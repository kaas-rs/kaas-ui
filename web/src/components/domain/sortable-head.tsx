import { TableHead } from "@/components/ui/table"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"

/**
 * The sorting half of [`HintHead`]: a header that reorders the table and says
 * what its column means.
 *
 * Two components rather than one with an optional `onClick`, because the
 * element differs — a `<button>` where the header is a control and a `<span>`
 * where it is a label — and a header that looks clickable and is not is worse
 * than either. Everything else is deliberately identical: the same dotted
 * underline, the same one line on hover, so a reader learns the affordance once
 * and it means the same thing on every table.
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
