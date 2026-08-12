import { Link } from "@tanstack/react-router"
import { ArrowRight, RefreshCw } from "lucide-react"

import { useFleet } from "@/api/client"
import type { ClusterCard as ClusterCardData } from "@/api/types"
import {
  ClusterChip,
  ClusterCounts,
  SnapshotAge,
  StatusBadge,
} from "@/components/domain"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardFooter, CardHeader } from "@/components/ui/card"

export function FleetCard({ card }: { card: ClusterCardData }) {
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

      {/* `mt-auto` is what makes equal heights readable rather than merely
          equal: the card stretches to its row, and the slack goes above this
          line instead of below it, so the footers of a row sit on one
          baseline whatever each card had to say. */}
      <CardFooter className="mt-auto border-t px-4 pt-3 [.border-t]:pt-3">
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
