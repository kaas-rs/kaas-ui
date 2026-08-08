// The nav: one environment's clusters, and what sits beside them.
//
// Shaped like shadcn's nav-main block — a group and one collapsible menu item
// per cluster — with three departures, each of which is a fact about this
// application:
//
//   * **The row navigates and the chevron folds.** The block's trigger is the
//     whole button, which is right when the parent is only a heading. Here it
//     is a page, so the name is a `Link` and the fold is a `SidebarMenuAction`
//     beside it. One row, two intentions, and a header that did both on one
//     click would do the wrong one half the time.
//   * **The sub-items are computed, not passed.** Which of topics / groups /
//     configs / capabilities exist depends on what the broker can answer and
//     what the caller may see, so the item list is derived per cluster rather
//     than handed in as data. Put two clusters side by side and the differing
//     item sets are a conformance report.
//   * **It shows one environment.** Whichever one you are in — opening a
//     cluster sets it, and the breadcrumb's environment crumb changes it. The
//     fleet page is where all of them are at once.
//
// Everything in the environment is listed, not only what can be navigated to —
// see `ResourceItem`. A cluster that is down joins them: same weight, and no
// fold, because everything inside it leads to the same "not connected".

import { Link, useRouterState } from "@tanstack/react-router"
import { useEffect, useState } from "react"
import { ChevronRight } from "lucide-react"

import { useCapabilities, useFleet } from "@/api/client"
import type {
  ClusterCard,
  EnvironmentRegistry,
  EnvironmentSection,
  Feature,
  Resource,
} from "@/api/types"
import {
  CLUSTER_ICON as ClusterIcon,
  RESOURCE_KINDS,
  byResourceKind,
} from "@/components/domain"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import {
  SidebarGroup,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
} from "@/components/ui/sidebar"
import {
  chooseEnvironment,
  pickEnvironment,
  useChosenEnvironment,
} from "@/lib/environment"
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
  // registry is one row beside the clusters now; see `RegistryItem`.
]

/**
 * One cluster: the row that opens it, and the items inside it.
 *
 * Items are rendered from the capability answer, so a cluster that cannot
 * answer `ListGroups` has **no groups item** rather than one that errors on
 * click. The routes still exist, so a URL shared from one cluster and opened
 * against another degrades into an explanation rather than a dead end.
 */
function ClusterItem({ card, active }: { card: ClusterCard; active: boolean }) {
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
function RegistryItem({
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

/**
 * One thing in the environment that is not a cluster.
 *
 * Rendered as text rather than as a button, on purpose: kaas-ui has no page
 * for a schema registry or an MQTT broker, and a nav row that does nothing on
 * click teaches people to stop trusting the rows that do. They are in the list
 * because "what is in staging" is a question the nav should answer, and the
 * answer is not only the brokers.
 *
 * It survives the collapse to icons. These are not targets, so the rail is not
 * strictly a set of targets any more — but a nav that answers "what is in
 * staging" only at full width answers it in the one state where the page had
 * room to answer it anyway. It keeps its glyph and gains the tooltip every
 * other collapsed row has.
 *
 * `SidebarMenuButton asChild` around a plain `div` is what buys that tooltip:
 * the styling and the collapsed-only tooltip come from the primitive, while
 * the element stays something that cannot be clicked, focused or announced as
 * a control. Hover is neutralised for the same reason.
 */
function ResourceItem({
  resource,
}: {
  resource: EnvironmentSection["resources"][number]
}) {
  const kind = RESOURCE_KINDS[resource.kind]
  const Icon = kind.icon

  return (
    <SidebarMenuItem>
      <SidebarMenuButton
        asChild
        // The endpoint and the kind, for the row that cannot lead anywhere.
        // "not probed" is the fleet card's job to say; a nav that repeated it
        // on every row would be nagging.
        tooltip={
          resource.endpoint
            ? `${resource.name} — ${resource.endpoint}`
            : resource.name
        }
        className="text-sidebar-foreground/55 hover:bg-transparent hover:text-sidebar-foreground/55"
      >
        <div
          title={
            resource.endpoint
              ? `${kind.label} — ${resource.endpoint}`
              : kind.label
          }
        >
          <Icon aria-hidden />
          <span className="truncate">{resource.name}</span>
        </div>
      </SidebarMenuButton>
    </SidebarMenuItem>
  )
}

/**
 * The clusters of the environment being looked at.
 *
 * No fleet item at the top: the mark in the sidebar header is the way back to
 * it, and a nav whose first row is "everything" competes with the list of
 * everything directly beneath it.
 */
export function NavMain() {
  const fleet = useFleet()
  const chosen = useChosenEnvironment()
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  })
  const activeCluster = pathname.match(/^\/clusters\/([^/]+)/)?.[1]

  const sections = fleet.data?.items ?? []
  const routed = sections.find((section) =>
    section.clusters.some((card) => card.id === activeCluster)
  )

  // Opening a cluster follows it into its environment, however you got there —
  // a pasted URL and a bookmark included. Keyed on the routed id alone so that
  // *choosing* an environment while looking at a cluster in another one is not
  // undone on the next render: the switcher is an intention, and this is only
  // the nav keeping up with where you already are.
  useEffect(() => {
    if (routed) chooseEnvironment(routed.id)
  }, [routed?.id])

  const active = pickEnvironment(sections, chosen)

  // An inventory card and a live registry usually describe one service — the
  // operator typed the endpoint under `environments:` beside the registry that
  // decodes. Showing both is the same registry twice, once as a link and once
  // as dead text, which reads as two registries of which one is broken.
  const registries = active?.schemaRegistries ?? []
  const urls = new Set(registries.map((entry) => entry.registry.url))
  const inventory = (active?.resources ?? []).filter(
    (resource) => !(resource.endpoint && urls.has(resource.endpoint))
  )

  if (!active) return null

  return (
    <SidebarGroup>
      {/* No label. The switcher directly above already names the environment,
          and a heading under it would be the same word twice.

          One list, not one per kind. What is in an environment is a single
          answer, and the clusters lead it because they are the only rows that
          go anywhere — everything after them is grouped by kind so that three
          registries read as three of one thing rather than three surprises. */}
      <SidebarMenu>
        {active.clusters.map((card) => (
          <ClusterItem
            key={card.id}
            card={card}
            active={card.id === activeCluster}
          />
        ))}
        {registries.map((entry) => (
          <RegistryItem
            key={entry.registry.id}
            envId={active.id}
            entry={entry}
          />
        ))}
        {byResourceKind(inventory).map((resource) => (
          <ResourceItem key={resource.id} resource={resource} />
        ))}
      </SidebarMenu>
    </SidebarGroup>
  )
}
