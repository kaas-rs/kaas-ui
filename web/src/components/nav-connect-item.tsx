import { Link, useRouterState } from "@tanstack/react-router"

import type { ConnectClusterCard } from "@/api/types"
import { RESOURCE_KINDS } from "@/components/domain"
import { SidebarMenuButton, SidebarMenuItem } from "@/components/ui/sidebar"

/**
 * A Connect cluster: one row, and it goes somewhere.
 *
 * Built like the registry row and for the same reason — a thing kaas-ui
 * dials, beside the clusters rather than inside one. A MirrorMaker 2
 * deployment is one of these too, because it is a Connect cluster.
 */
export function NavConnectItem({
  envId,
  card,
}: {
  envId: string
  card: ConnectClusterCard
}) {
  const Icon = RESOURCE_KINDS.kafka_connect.icon
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  })
  const broken =
    card.status === "unreachable" || card.status === "misconfigured"

  return (
    <SidebarMenuItem>
      <SidebarMenuButton
        asChild
        isActive={pathname.startsWith(
          `/environments/${envId}/connect-clusters/${card.id}`
        )}
        tooltip={`${card.name} — ${card.url}`}
      >
        <Link
          to="/environments/$envId/connect-clusters/$connectId"
          params={{ envId, connectId: card.id }}
          search={{ listing: "connectors" }}
        >
          <Icon aria-hidden />
          <span className="truncate">{card.name}</span>
          {broken ? (
            <span
              className="text-warn-ink ml-auto text-[10px]"
              title={card.error ?? card.status}
            >
              {card.status === "misconfigured" ? "!" : "?"}
            </span>
          ) : null}
        </Link>
      </SidebarMenuButton>
    </SidebarMenuItem>
  )
}
