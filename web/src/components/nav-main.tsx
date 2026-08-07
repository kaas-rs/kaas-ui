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
}[] = [
  {
    label: "topics",
    to: "/clusters/$clusterId/topics",
    resource: "topic",
  },
  {
    label: "groups",
    to: "/clusters/$clusterId/groups",
    feature: "consumerGroups",
    resource: "consumer",
  },
  {
    label: "configs",
    to: "/clusters/$clusterId/configs",
    feature: "configs",
    resource: "cluster_config",
  },
  {
    label: "capabilities",
    to: "/clusters/$clusterId/capabilities",
    resource: "cluster_config",
  },
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
  const capabilities = useCapabilities(card.id)
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
    if (unreachable) return false
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
          isActive={pathname === `/clusters/${card.id}`}
          tooltip={`${card.id} — ${card.status}`}
          // Dimmed to the same weight as the resources below, because it now
          // holds the same amount: a name and nothing to open. It stays a link
          // — its page is where the transport error and the retry are, which
          // is the one thing worth clicking on a cluster that is down.
          className={cn(unreachable && "text-sidebar-foreground/55")}
        >
          {/* Following the name is also asking to see what is under it. */}
          <Link
            to="/clusters/$clusterId"
            params={{ clusterId: card.id }}
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
                  const href = item.to.replace("$clusterId", card.id)
                  return (
                    <SidebarMenuSubItem key={item.label}>
                      <SidebarMenuSubButton
                        asChild
                        isActive={pathname.startsWith(href)}
                      >
                        <Link to={item.to} params={{ clusterId: card.id }}>
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
        {byResourceKind(active.resources).map((resource) => (
          <ResourceItem key={resource.id} resource={resource} />
        ))}
      </SidebarMenu>
    </SidebarGroup>
  )
}
