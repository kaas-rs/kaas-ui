import type { ReactNode } from "react"
import { ChevronDown } from "lucide-react"

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"

/** A crumb whose name opens a menu of the things beside it. */
export function CrumbMenu({
  label,
  mono,
  current,
  children,
}: {
  label: string
  mono?: boolean
  current?: boolean
  children: ReactNode
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        className={[
          "flex min-w-0 cursor-pointer items-center gap-0.5 rounded-sm hover:text-ink hover:underline",
          mono ? "font-mono" : "",
          current ? "text-ink" : "text-ink-muted",
        ].join(" ")}
        {...(current ? { "aria-current": "page" as const } : {})}
      >
        <span className="truncate">{label}</span>
        <ChevronDown aria-hidden className="size-3 shrink-0 opacity-60" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-48">
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
