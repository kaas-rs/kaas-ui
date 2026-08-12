import type { ReactNode } from "react"

import { TableHead } from "@/components/ui/table"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"

/**
 * A table header that says what its column means on hover.
 *
 * Every column it labels is a term with a precise meaning and a plausible
 * wrong reading — `records` is what is *retained*, not what was ever written,
 * and `epoch` counts leadership changes rather than anything about data. One
 * line each, on hover, is cheaper than a legend nobody scrolls to.
 */
export function HintHead({
  label,
  hint,
  right,
  className,
}: {
  label: ReactNode
  hint: string
  right?: boolean
  className?: string
}) {
  return (
    <TableHead className={cn(right && "text-right", className)}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="cursor-help decoration-dotted underline-offset-4 hover:underline">
            {label}
          </span>
        </TooltipTrigger>
        <TooltipContent>{hint}</TooltipContent>
      </Tooltip>
    </TableHead>
  )
}
