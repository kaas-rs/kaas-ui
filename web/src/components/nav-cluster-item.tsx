import { Link, useRouterState } from "@tanstack/react-router"
import { useEffect, useState } from "react"
import { ChevronRight } from "lucide-react"

import { useCapabilities } from "@/api/client"
import type { ClusterCard, Feature, Resource } from "@/api/types"
import { CLUSTER_ICON as ClusterIcon } from "@/components/domain"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import {
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
} from "@/components/ui/sidebar"
import { cn } from "@/lib/utils"

/**
 * A cluster's items, and the capability each one needs to exist.
 *
 * No overview item: the cluster's own name is the link to it. A row that is
 * also the section's landing page is one row rather than two saying nearly the
 * same thing.
 *
 * No icons either. These four are indented under a cluster, they are always
 * the same four in the same order, and the word is what anyone reads — a glyph
 * per row was four more shapes to learn for no bit of information. The icons
 * that remain are the ones that distinguish *kinds of thing*: a cluster from a
 * schema registry from an MQTT broker.
 */
const CLUSTER_NAV: {
  label: string
  to: string
  feature?: Feature
  /** The permission this item needs `view` on. */
  resource?: Resource
  /**
   * Whether the item is worth showing on a cluster that is not connected.
   *
   * True for everything the *brokers* answer, which is everything but one: a
   * schema registry serves an environment and knows nothing about brokers, so
   * its subjects stay browsable while the cluster you arrived through is down.
   */
  needsConnection?: boolean
}[] = [
  {
    label: "topics",
    to: "/environments/$envId/clusters/$clusterId/topics",
    resource: "topic",
  },
  {
    label: "groups",
    to: "/environments/$envId/clusters/$clusterId/groups",
    feature: "consumerGroups",
    resource: "consumer",
  },
  {
    label: "configs",
    to: "/environments/$envId/clusters/$clusterId/configs",
    feature: "configs",
    resource: "cluster_config",
  },
  {
    label: "capabilities",
    to: "/environments/$envId/clusters/$clusterId/capabilities",
    resource: "cluster_config",
  },
  // No schemas item. A registry serves the *environment*, so two clusters
  // referencing `dev` gave two rows opening the same subject list — the nav
  // said "these are kaas's schemas" twice, and neither time was it true. The
  // registry is one row beside the clusters now; see `NavRegistryItem`.
]

/**
 * One cluster: the row that opens it, and the items inside it.
 *
 * Items are rendered from the capability answer, so a cluster that cannot
 * answer `ListGroups` has **no groups item** rather than one that errors on
 * click. The routes still exist, so a URL shared from one cluster and opened
 * against another degrades into an explanation rather than a dead end.
 */
export function NavClusterItem({
  card,
  active,
}: {
  card: ClusterCard
  active: boolean
}) {
  const capabilities = useCapabilities(card.environment, card.id)
  const [open, setOpen] = useState(active)
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  })

  useEffect(() => {
    if (active) setOpen(true)
  }, [active])

  // A third reason, and the bluntest: there is no connection. Every item under
  // an unreachable cluster leads to the same "not connected" answer, so the
  // fold goes with them and the row is left saying the one true thing it can.
  const unreachable = card.status === "unreachable"

  const items = CLUSTER_NAV.filter((item) => {
    if (unreachable && item.needsConnection !== false) return false
    // Two reasons an item can be absent, and they are different claims: the
    // broker cannot answer it, or this caller may not ask. Both end as "no
    // item" rather than an item that errors on click.
    if (
      item.resource &&
      card.grants[item.resource]?.includes("view") !== true
    ) {
      return false
    }
    if (!item.feature) return true
    // Until the answer arrives, show it: an item that appears under the cursor
    // is worse than one that errors once.
    const state = capabilities.data?.features.find(
      (feature) => feature.feature === item.feature
    )
    return state === undefined || state.state === "available"
  })

  return (
    <Collapsible
      open={open}
      onOpenChange={setOpen}
      asChild
      className="group/cluster"
    >
      <SidebarMenuItem>
        <SidebarMenuButton
          asChild
          isActive={
            pathname === `/environments/${card.environment}/clusters/${card.id}`
          }
          tooltip={`${card.id} — ${card.status}`}
          // Dimmed to the same weight as the resources below, because it now
          // holds the same amount: a name and nothing to open. It stays a link
          // — its page is where the transport error and the retry are, which
          // is the one thing worth clicking on a cluster that is down.
          className={cn(unreachable && "text-sidebar-foreground/55")}
        >
          {/* Following the name is also asking to see what is under it. */}
          <Link
            to="/environments/$envId/clusters/$clusterId"
            params={{ envId: card.environment, clusterId: card.id }}
            onClick={() => setOpen(true)}
          >
            <ClusterIcon aria-hidden />
            <span>{card.id}</span>
          </Link>
        </SidebarMenuButton>

        {items.length > 0 ? (
          <>
            <CollapsibleTrigger asChild>
              <SidebarMenuAction
                aria-label={`${open ? "Collapse" : "Expand"} ${card.id}`}
              >
                <ChevronRight className="transition-transform duration-200 group-data-[state=open]/cluster:rotate-90" />
              </SidebarMenuAction>
            </CollapsibleTrigger>

            <CollapsibleContent>
              <SidebarMenuSub>
                {items.map((item) => {
                  const href = item.to
                    .replace("$envId", card.environment)
                    .replace("$clusterId", card.id)
                  return (
                    <SidebarMenuSubItem key={item.label}>
                      <SidebarMenuSubButton
                        asChild
                        isActive={pathname.startsWith(href)}
                      >
                        <Link
                          to={item.to}
                          params={{
                            envId: card.environment,
                            clusterId: card.id,
                          }}
                        >
                          <span>{item.label}</span>
                        </Link>
                      </SidebarMenuSubButton>
                    </SidebarMenuSubItem>
                  )
                })}
              </SidebarMenuSub>
            </CollapsibleContent>
          </>
        ) : null}
      </SidebarMenuItem>
    </Collapsible>
  )
}
