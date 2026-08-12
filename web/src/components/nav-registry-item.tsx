import { Link, useRouterState } from "@tanstack/react-router"

import type { EnvironmentRegistry } from "@/api/types"
import { RESOURCE_KINDS } from "@/components/domain"
import { SidebarMenuButton, SidebarMenuItem } from "@/components/ui/sidebar"

/**
 * The environment's schema registry: one row, and it goes somewhere.
 *
 * The only resource kind kaas-ui has a page for, which is why it is the only
 * one that is a link. It sits beside the clusters rather than inside them
 * because that is what it is — every cluster in this environment that names it
 * reads these same subjects, from the same handle, through the same cache.
 *
 * It has its own URL now. The registry id is scoped to the environment and the
 * environment is only reachable when something in it is visible, so the id can
 * lead a route without becoming a namespace anyone can probe — which is what
 * routing through a cluster used to buy at the cost of a URL that named the
 * wrong thing.
 */
export function NavRegistryItem({
  envId,
  entry,
}: {
  envId: string
  entry: EnvironmentRegistry
}) {
  const Icon = RESOURCE_KINDS.schema_registry.icon
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  })
  const { registry } = entry
  const broken =
    registry.status === "unreachable" || registry.status === "misconfigured"

  return (
    <SidebarMenuItem>
      <SidebarMenuButton
        asChild
        // Any cluster's schema route is this registry: arriving on
        // `/clusters/strimzi/schemas` from a link someone sent must light the
        // same row as arriving through `kaas`, because it is the same page.
        isActive={pathname.startsWith(
          `/environments/${envId}/schema-registries/${registry.id}`
        )}
        tooltip={`${registry.name} — ${registry.url}`}
      >
        <Link
          to="/environments/$envId/schema-registries/$registryId"
          params={{ envId, registryId: registry.id }}
        >
          <Icon aria-hidden />
          <span className="truncate">{registry.name}</span>
          {/* A registry that is down still lists what it last knew, so the row
              stays a link and says so quietly rather than disappearing. */}
          {broken ? (
            <span
              className="text-warn-ink ml-auto text-[10px]"
              title={registry.error ?? registry.status}
            >
              {registry.status === "misconfigured" ? "!" : "?"}
            </span>
          ) : null}
        </Link>
      </SidebarMenuButton>
    </SidebarMenuItem>
  )
}
