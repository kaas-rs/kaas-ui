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
// see `NavResourceItem`. A cluster that is down joins them: same weight, and
// no fold, because everything inside it leads to the same "not connected".

import { useRouterState } from "@tanstack/react-router"
import { useEffect } from "react"

import { useFleet } from "@/api/client"
import { byResourceKind } from "@/components/domain"
import { NavClusterItem } from "@/components/nav-cluster-item"
import { NavRegistryItem } from "@/components/nav-registry-item"
import { NavResourceItem } from "@/components/nav-resource-item"
import { SidebarGroup, SidebarMenu } from "@/components/ui/sidebar"
import {
  chooseEnvironment,
  pickEnvironment,
  useChosenEnvironment,
} from "@/lib/environment"

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
          <NavClusterItem
            key={card.id}
            card={card}
            active={card.id === activeCluster}
          />
        ))}
        {registries.map((entry) => (
          <NavRegistryItem
            key={entry.registry.id}
            envId={active.id}
            entry={entry}
          />
        ))}
        {byResourceKind(inventory).map((resource) => (
          <NavResourceItem key={resource.id} resource={resource} />
        ))}
      </SidebarMenu>
    </SidebarGroup>
  )
}
