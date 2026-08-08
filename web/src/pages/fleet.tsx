import { Link } from "@tanstack/react-router"
import { ArrowRight, RefreshCw } from "lucide-react"

import { useFleet } from "@/api/client"
import type {
  ClusterCard as ClusterCardData,
  EnvironmentRegistry,
  EnvironmentSection,
  ResourceCard as ResourceCardData,
} from "@/api/types"
import {
  ClusterChip,
  ClusterCounts,
  Empty,
  RESOURCE_KINDS,
  byResourceKind,
  SnapshotAge,
  Spinner,
  StatusBadge,
} from "@/components/domain"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardFooter, CardHeader } from "@/components/ui/card"
import { PageTitle } from "@/components/page-title"
import { cn } from "@/lib/utils"

/**
 * The fleet, one section per environment.
 *
 * The sections arrive assembled, in order, from `/api/fleet` — declared
 * environments first, in declared order, because "dev, staging, prod" is not
 * recoverable from three strings by any sort this page could apply. An
 * environment holding nothing this caller may see is not in the response at
 * all, so there is no empty heading here to report that prod exists.
 */
export function Fleet() {
  const fleet = useFleet()

  if (fleet.isLoading) return <Spinner label="loading the fleet" />
  if (fleet.error) {
    return (
      <Card className="p-5 text-danger">
        the fleet could not be loaded: {String(fleet.error)}
      </Card>
    )
  }

  const sections = fleet.data?.items ?? []
  const clusters = sections.reduce(
    (total, section) => total + section.clusters.length,
    0
  )
  const resources = sections.reduce(
    (total, section) => total + section.resources.length,
    0
  )

  return (
    <>
      <PageTitle
        title="Fleet"
        subtitle={
          <>
            {plural(clusters, "cluster")}
            {resources > 0
              ? ` and ${plural(resources, "other resource")}`
              : null}
            {sections.length > 0
              ? ` across ${plural(sections.length, "environment")}`
              : null}
          </>
        }
      />

      {sections.length === 0 ? (
        <Empty>nothing configured is visible to you</Empty>
      ) : (
        sections.map((section) => (
          <Environment key={section.id} section={section} />
        ))
      )}
    </>
  )
}

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`
}

/**
 * One environment: its heading, and everything in it.
 *
 * Exported because the environment page renders exactly this — the fleet is
 * every environment and the environment page is one of them, so a second
 * layout would be two things to keep in step for no reason.
 */
export function Environment({ section }: { section: EnvironmentSection }) {
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
      */}
      <div className="grid gap-4 grid-cols-[repeat(auto-fill,minmax(min(20rem,100%),1fr))]">
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

function FleetCard({ card }: { card: ClusterCardData }) {
  const fleet = useFleet()
  const unreachable = card.status === "unreachable"

  return (
    <Card className="gap-3 py-4">
      <CardHeader className="gap-2 px-4">
        <div className="flex items-start justify-between gap-3">
          <Link
            to="/environments/$envId/clusters/$clusterId"
            params={{ envId: card.environment, clusterId: card.id }}
            className="font-semibold hover:underline"
            style={{ color: "var(--rust-ink)" }}
          >
            {card.name}
          </Link>
          <StatusBadge status={card.status} />
        </div>
        <div className="flex items-center gap-2">
          <ClusterChip id={card.id} labels={card.labels} size="small" />
          {card.clusterId ? (
            <span className="truncate font-mono text-[11px] text-ink-faint">
              {card.clusterId}
            </span>
          ) : null}
        </div>
      </CardHeader>

      <CardContent className="px-4">
        {unreachable ? (
          // Visually distinct without being the loudest thing on the page: a
          // fleet where one cluster is always down should not train anyone to
          // ignore red.
          <div
            className="rounded-sm border p-3 text-[12px]"
            style={{
              background: "var(--danger-soft)",
              borderColor: "var(--danger)",
            }}
          >
            <p className="break-words font-mono">{card.error}</p>
            <div className="mt-2 flex flex-wrap items-center justify-between gap-3">
              <span className="text-ink-muted">
                {card.attempts} failed attempt{card.attempts === 1 ? "" : "s"}
              </span>
              <Button
                variant="outline"
                size="sm"
                // Asking again is a GET. The server nudges its connector to
                // retry now rather than at the end of the backoff, which is how
                // a retry button exists in an application with no non-GET route.
                onClick={() => void fleet.refetch()}
              >
                <RefreshCw aria-hidden />
                retry now
              </Button>
            </div>
          </div>
        ) : (
          <ClusterCounts card={card} />
        )}
      </CardContent>

      <CardFooter className="border-t px-4 pt-3 [.border-t]:pt-3">
        <SnapshotAge
          ageMs={card.snapshotAgeMs}
          asOfMs={fleet.dataUpdatedAt}
          maxStalenessMs={card.maxStalenessMs}
        />
        <Button variant="link" size="sm" asChild className="ml-auto h-auto p-0">
          <Link
            to="/environments/$envId/clusters/$clusterId/topics"
            params={{ envId: card.environment, clusterId: card.id }}
          >
            topics
            <ArrowRight aria-hidden />
          </Link>
        </Button>
      </CardFooter>
    </Card>
  )
}

/**
 * Something in this environment that is not a Kafka cluster.
 *
 * Inventory, not monitoring. kaas-ui dials none of these, so the card carries
 * no status badge and says so — a green dot earned by a correctly typed URL
 * would be worse than no dot at all. `self-start` keeps it its own height
 * rather than stretching to match a cluster card three times as tall.
 */
/**
 * A schema registry, which is the one non-cluster kaas-ui actually dials.
 *
 * So unlike [`ResourceTile`] it has a status worth rendering and a page worth
 * opening. `usedBy` empty is a real answer — declared, and nothing decodes
 * against it — and saying so is the whole reason a registry nobody references
 * is listed at all.
 */
function RegistryTile({
  envId,
  entry,
}: {
  envId: string
  entry: EnvironmentRegistry
}) {
  const { registry, usedBy } = entry
  const Icon = RESOURCE_KINDS.schema_registry.icon
  const broken =
    registry.status === "unreachable" || registry.status === "misconfigured"

  return (
    <Card className="gap-3 self-start py-4">
      <CardHeader className="gap-2 px-4">
        <div className="flex items-start justify-between gap-3">
          <Link
            to="/environments/$envId/schema-registries/$registryId"
            params={{ envId, registryId: registry.id }}
            className="font-semibold hover:underline"
            style={{ color: "var(--rust-ink)" }}
          >
            {registry.name}
          </Link>
          <span
            className={cn(
              "shrink-0 rounded-sm border px-1.5 py-0.5 text-[11px]",
              registry.status === "ready" && "border-ok/50 text-ok-ink",
              registry.status === "unreachable" &&
                "border-warn-ink/50 text-warn-ink",
              registry.status === "misconfigured" &&
                "border-danger/50 text-danger",
              registry.status === "unprobed" && "border-dashed text-ink-faint"
            )}
            title={registry.error ?? undefined}
          >
            {registry.status}
          </span>
        </div>
        <div className="flex items-center gap-1.5 text-[12px] text-ink-muted">
          <Icon aria-hidden className="size-3.5" />
          schema registry
          <span className="font-mono text-[11px] text-ink-faint">
            {registry.id}
          </span>
        </div>
      </CardHeader>

      <CardContent className="px-4">
        <p className="break-all font-mono text-[12px] text-ink-muted">
          {registry.url}
        </p>
        <p className="mt-2 text-[12px] text-ink-muted">
          {usedBy.length > 0 ? (
            <>
              decoded against by{" "}
              <span className="font-mono">{usedBy.join(", ")}</span>
            </>
          ) : (
            <span className="text-ink-faint">
              no cluster here references it
            </span>
          )}
        </p>
        {broken && registry.error ? (
          <p
            className={cn(
              "mt-2 text-[11px]",
              registry.status === "misconfigured"
                ? "text-danger"
                : "text-warn-ink"
            )}
          >
            {registry.error}
          </p>
        ) : null}
      </CardContent>
    </Card>
  )
}

function ResourceTile({ card }: { card: ResourceCardData }) {
  const kind = RESOURCE_KINDS[card.kind]
  const Icon = kind.icon

  return (
    <Card className="gap-3 self-start py-4">
      <CardHeader className="gap-2 px-4">
        <div className="flex items-start justify-between gap-3">
          <span className="font-semibold">{card.name}</span>
          <span
            className="shrink-0 rounded-sm border border-dashed px-1.5 py-0.5 text-[11px] text-ink-faint"
            title="kaas-ui does not connect to this, so it has no health to report"
          >
            not probed
          </span>
        </div>
        <div className="flex items-center gap-1.5 text-[12px] text-ink-muted">
          <Icon aria-hidden className="size-3.5" />
          {kind.label}
          <span className="font-mono text-[11px] text-ink-faint">
            {card.id}
          </span>
        </div>
      </CardHeader>

      <CardContent className="px-4">
        {card.endpoint ? (
          <p className="break-all font-mono text-[12px] text-ink-muted">
            {card.endpoint}
          </p>
        ) : null}
        {card.note ? (
          <p className="mt-2 text-[12px] text-ink-muted">{card.note}</p>
        ) : null}
      </CardContent>
    </Card>
  )
}
