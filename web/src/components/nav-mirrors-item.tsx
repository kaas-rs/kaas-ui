import { Link, useRouterState } from "@tanstack/react-router"
import { Repeat } from "lucide-react"

import { useMirrors } from "@/api/client"
import { SidebarMenuButton, SidebarMenuItem } from "@/components/ui/sidebar"

/**
 * The environment's MirrorMaker flows: one row, drawn only when there are
 * any. A flow is not a resource anyone configured — it is found, from the
 * Connect clusters and the clusters' topics — so the row follows the finding.
 */
export function NavMirrorsItem({ envId }: { envId: string }) {
  const mirrors = useMirrors(envId)
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  })
  const flows = mirrors.data?.flows.length ?? 0
  if (flows === 0) return null

  return (
    <SidebarMenuItem>
      <SidebarMenuButton
        asChild
        isActive={pathname.startsWith(`/environments/${envId}/mirrors`)}
        tooltip={`${flows} MirrorMaker flow${flows === 1 ? "" : "s"}`}
      >
        <Link to="/environments/$envId/mirrors" params={{ envId }}>
          <Repeat aria-hidden />
          <span className="truncate">MirrorMaker</span>
          <span className="ml-auto text-[11px] text-ink-faint">{flows}</span>
        </Link>
      </SidebarMenuButton>
    </SidebarMenuItem>
  )
}
