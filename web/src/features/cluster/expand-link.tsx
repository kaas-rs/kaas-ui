import type { ReactNode } from "react"

import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

/**
 * The link in a broker row that opens a panel below the table. One style for
 * every such column, so a reader learns once that a muted cell value can be
 * clicked; `hide` replaces it while its panel is the open one.
 */
export function ExpandLink({
  open,
  onToggle,
  className,
  title,
  children,
}: {
  open: boolean
  onToggle: () => void
  className?: string
  title?: string
  children: ReactNode
}) {
  return (
    <Button
      variant="link"
      size="sm"
      className={cn("h-auto p-0 text-[12px] text-ink-muted", className)}
      onClick={onToggle}
      title={title}
    >
      {open ? "hide" : children}
    </Button>
  )
}
