import type { EnvironmentSection as EnvironmentSectionData } from "@/api/types"
import { byResourceKind } from "@/components/domain"

import { FleetCard } from "@/features/fleet/fleet-card"
import { plural } from "@/features/fleet/plural"
import { RegistryTile } from "@/features/fleet/registry-tile"
import { ResourceTile } from "@/features/fleet/resource-tile"

/**
 * One environment: its heading, and everything in it.
 *
 * Exported because the environment page renders exactly this — the fleet is
 * every environment and the environment page is one of them, so a second
 * layout would be two things to keep in step for no reason.
 */
export function EnvironmentSection({
  section,
}: {
  section: EnvironmentSectionData
}) {
  // The same string `clusterTone` singles out. prod must not look like
  // anything else, and a section heading is the first place someone reads.
  const production = section.id === "prod"
  const unreachable = section.clusters.filter(
    (card) => card.status === "unreachable"
  ).length

  return (
    <section className="mb-10">
      <div
        className="mb-4 flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 border-b pb-2"
        style={production ? { borderColor: "var(--danger)" } : undefined}
      >
        <div className="min-w-0">
          <h2
            className="text-[15px] font-semibold tracking-[-0.01em]"
            style={production ? { color: "var(--danger)" } : undefined}
          >
            {section.name}
          </h2>
          {section.description ? (
            <p className="mt-0.5 text-[12px] text-ink-muted">
              {section.description}
            </p>
          ) : null}
        </div>

        <div className="flex shrink-0 items-baseline gap-3 text-[12px] text-ink-faint">
          {unreachable > 0 ? (
            <span className="text-danger">{unreachable} unreachable</span>
          ) : null}
          <span>
            {plural(section.clusters.length, "cluster")}
            {section.resources.length > 0
              ? ` · ${section.resources.length} other`
              : null}
          </span>
        </div>
      </div>

      {/*
        `min(20rem, 100%)` rather than a bare `20rem`: auto-fill would otherwise
        hold a 320px track on a 300px viewport and push the page into a
        horizontal scroll. Below that width it is one column, above it as many
        as fit, with no breakpoint to keep in sync.

        `auto-rows-fr` is what makes every card in the section one height rather
        than every card in a *row*. Stretching alone equalises siblings that
        happen to share a row, so a registry that wrapped onto the second row
        went back to being shorter than the clusters above it — the one place
        the difference is most visible, and the one it least means anything.
        With equal rows the grid is a grid, and what varies between cards is
        what they say.

        From `sm` up, because below it there is one column: equal heights buy
        nothing when every row holds one card, and would cost a phone a screen
        of blank space under each short one.
      */}
      <div className="grid gap-4 grid-cols-[repeat(auto-fill,minmax(min(20rem,100%),1fr))] sm:auto-rows-fr">
        {section.clusters.map((card) => (
          <FleetCard key={card.id} card={card} />
        ))}
        {/* Between the clusters and the inventory, because that is what it is:
            a thing kaas-ui talks to, like a cluster, that is not a cluster. */}
        {section.schemaRegistries.map((entry) => (
          <RegistryTile
            key={entry.registry.id}
            envId={section.id}
            entry={entry}
          />
        ))}
        {byResourceKind(section.resources).map((card) => (
          <ResourceTile key={card.id} card={card} />
        ))}
      </div>
    </section>
  )
}
