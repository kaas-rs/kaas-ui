import { Link } from "@tanstack/react-router"
import { ArrowRight, RefreshCw } from "lucide-react"

import { useSubjects } from "@/api/client"
import type { EnvironmentRegistry } from "@/api/types"
import {
  RESOURCE_KINDS,
  RegistryCounts,
  RegistryStatusBadge,
} from "@/components/domain"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardFooter, CardHeader } from "@/components/ui/card"

/**
 * A schema registry, which is the one non-cluster kaas-ui actually dials.
 *
 * So unlike [`ResourceTile`] it has a status worth rendering, numbers worth
 * counting and a page worth opening — which is why it is built like
 * [`FleetCard`] and not like the inventory tiles below it: same header, same
 * badge shape, same stat grid, same footer with the link out. A reader
 * scanning the row should be able to tell what a card *is* from its icon and
 * its stats, not from its layout.
 *
 * The numbers are the subject listing's summary, fetched here rather than
 * ridden along on `/api/fleet`. Two reasons, and both are why clusters connect
 * lazily: the fleet response must not wait on a registry that is not
 * answering, and one registry's outage must degrade one card. So this card
 * probes, and a registry nobody had decoded against yet stops reading
 * `unprobed` the moment somebody looks at the fleet — that is the honest
 * outcome of having asked, not a state being papered over.
 *
 * `usedBy` empty is a real answer — declared, and nothing decodes against it —
 * and saying so is the whole reason a registry nobody references is listed.
 */
export function RegistryTile({
  envId,
  entry,
}: {
  envId: string
  entry: EnvironmentRegistry
}) {
  const { registry, usedBy } = entry
  const Icon = RESOURCE_KINDS.schema_registry.icon
  // `limit: 0` — the counts, and not one row of the thing being counted. They
  // are computed over the whole listing server-side, which is where the names
  // already are; asking for them by downloading every subject name onto a card
  // would make the fleet page cost the size of the biggest registry on it.
  const subjects = useSubjects(envId, registry.id, { limit: 0 })

  // The card the *registry* just answered with beats the one the fleet was
  // assembled from: this component has since asked it a question, and a badge
  // saying `unprobed` beside a subject count would be reporting a state that
  // this very render disproved. It falls back while the listing is in flight,
  // and on a listing that failed — where the fleet's view is all there is.
  const live = subjects.data?.registry ?? registry
  const broken =
    live.status === "unreachable" || live.status === "misconfigured"
  // A 404 or a 5xx from our own API, which is not the registry being down —
  // that arrives as a `200` with an empty list and a card that says so.
  const error = subjects.error
    ? (subjects.error as Error).message
    : broken
      ? (live.error ?? live.status)
      : null

  return (
    <Card className="gap-3 py-4">
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
          <RegistryStatusBadge
            status={live.status}
            title={live.error ?? undefined}
          />
        </div>
        <div className="flex items-center gap-1.5 text-[12px] text-ink-muted">
          <Icon aria-hidden className="size-3.5" />
          schema registry
          <span
            className="truncate font-mono text-[11px] text-ink-faint"
            title={registry.url}
          >
            {registry.id}
          </span>
        </div>
      </CardHeader>

      <CardContent className="px-4">
        {error ? (
          // The same panel a cluster that will not answer gets, in the tone
          // the fault deserves: an outage is warm and heals on its own, a url
          // pointing at the wrong API is red and will still be wrong after
          // every retry.
          <div
            className="rounded-sm border p-3 text-[12px]"
            style={{
              background:
                live.status === "misconfigured"
                  ? "var(--danger-soft)"
                  : "var(--warn-soft)",
              borderColor:
                live.status === "misconfigured"
                  ? "var(--danger)"
                  : "var(--warn-ink)",
            }}
          >
            <p className="break-words font-mono">{error}</p>
            <div className="mt-2 flex flex-wrap items-center justify-between gap-3">
              <span className="text-ink-muted">
                {live.status === "misconfigured"
                  ? "check the url"
                  : "nothing decodes here until it answers"}
              </span>
              {/* Asking again is a GET, and the listing is what re-probes:
                  `subjects()` refetches once its TTL is out, so this is a
                  real attempt rather than a rerender. */}
              <Button
                variant="outline"
                size="sm"
                onClick={() => void subjects.refetch()}
              >
                <RefreshCw aria-hidden />
                retry now
              </Button>
            </div>
          </div>
        ) : (
          <RegistryCounts
            summary={subjects.data ?? null}
            pending={subjects.isPending}
          />
        )}
        <p
          className="mt-3 truncate font-mono text-[12px] text-ink-faint"
          title={registry.url}
        >
          {registry.url}
        </p>
      </CardContent>

      {/* `mt-auto` is what makes equal heights readable rather than merely
          equal: the card stretches to its row, and the slack goes above this
          line instead of below it, so the footers of a row sit on one
          baseline whatever each card had to say. */}
      <CardFooter className="mt-auto border-t px-4 pt-3 [.border-t]:pt-3">
        {/* Where the snapshot age sits on a cluster card, and answering the
            question that is this card's equivalent: not how fresh it is, but
            who is reading it. A registry serves the environment, so this is
            the line that stops it reading as one cluster's. */}
        <span className="min-w-0 truncate text-[12px] text-ink-faint">
          {usedBy.length > 0 ? (
            <>
              read by <span className="font-mono">{usedBy.join(", ")}</span>
            </>
          ) : (
            "no cluster here references it"
          )}
        </span>
        <Button variant="link" size="sm" asChild className="ml-auto h-auto p-0">
          <Link
            to="/environments/$envId/schema-registries/$registryId"
            params={{ envId, registryId: registry.id }}
          >
            subjects
            <ArrowRight aria-hidden />
          </Link>
        </Button>
      </CardFooter>
    </Card>
  )
}
