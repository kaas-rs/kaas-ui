import type { ComponentProps } from "react"

import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

/**
 * The link in a broker row that opens a panel below the table. One style for
 * every such column, so a reader learns once that a muted cell value can be
 * clicked; `hide` replaces it while its panel is the open one.
 *
 * The rest of a button's props pass through, so a `TooltipTrigger` can render
 * it and attach its ref and handlers.
 */
export function ExpandLink({
  open,
  onToggle,
  className,
  children,
  ...props
}: Omit<ComponentProps<typeof Button>, "onClick"> & {
  open: boolean
  onToggle: () => void
}) {
  return (
    <Button
      variant="link"
      size="sm"
      {...props}
      className={cn("h-auto p-0 text-[12px] text-ink-muted", className)}
      onClick={onToggle}
    >
      {open ? "hide" : children}
    </Button>
  )
}
