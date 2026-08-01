import { Link } from "@tanstack/react-router";
import { ArrowRight, RefreshCw } from "lucide-react";

import { useClusters } from "@/api/client";
import type { ClusterCard as ClusterCardData } from "@/api/types";
import {
  ClusterChip,
  ClusterCounts,
  SnapshotAge,
  Spinner,
  StatusBadge,
} from "@/components/domain";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardFooter, CardHeader } from "@/components/ui/card";
import { PageTitle } from "@/shell";

/** Group by `env`, then `kind` — the two labels the fleet is organised by. */
function groupKey(card: ClusterCardData): string {
  const env = card.labels.env ?? "unlabelled";
  const kind = card.labels.kind;
  return kind ? `${env} · ${kind}` : env;
}

export function Fleet() {
  const clusters = useClusters();

  if (clusters.isLoading) return <Spinner label="loading the fleet" />;
  if (clusters.error) {
    return (
      <Card className="p-5 text-danger">
        the fleet could not be loaded: {String(clusters.error)}
      </Card>
    );
  }

  const cards = clusters.data?.items ?? [];
  const groups = new Map<string, ClusterCardData[]>();
  for (const card of cards) {
    const key = groupKey(card);
    groups.set(key, [...(groups.get(key) ?? []), card]);
  }

  return (
    <>
      <PageTitle
        title="Fleet"
        subtitle={`${cards.length} configured cluster${cards.length === 1 ? "" : "s"}`}
      />

      {[...groups.entries()].map(([group, members]) => (
        <section key={group} className="mb-8">
          <h2 className="mb-3 text-[12px] uppercase tracking-wide text-ink-faint">
            {group}
          </h2>
          <div className="grid gap-4 grid-cols-[repeat(auto-fill,minmax(20rem,1fr))]">
            {members.map((card) => (
              <FleetCard key={card.id} card={card} />
            ))}
          </div>
        </section>
      ))}
    </>
  );
}

function FleetCard({ card }: { card: ClusterCardData }) {
  const clusters = useClusters();
  const unreachable = card.status === "unreachable";

  return (
    <Card className="gap-3 py-4">
      <CardHeader className="gap-2 px-4">
        <div className="flex items-start justify-between gap-3">
          <Link
            to="/clusters/$clusterId"
            params={{ clusterId: card.id }}
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
            style={{ background: "var(--danger-soft)", borderColor: "var(--danger)" }}
          >
            <p className="break-words font-mono">{card.error}</p>
            <div className="mt-2 flex items-center justify-between gap-3">
              <span className="text-ink-muted">
                {card.attempts} failed attempt{card.attempts === 1 ? "" : "s"}
              </span>
              <Button
                variant="outline"
                size="sm"
                // Asking again is a GET. The server nudges its connector to
                // retry now rather than at the end of the backoff, which is how
                // a retry button exists in an application with no non-GET route.
                onClick={() => void clusters.refetch()}
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
        <SnapshotAge ageMs={card.snapshotAgeMs} maxStalenessMs={card.maxStalenessMs} />
        <Button variant="link" size="sm" asChild className="ml-auto h-auto p-0">
          <Link to="/clusters/$clusterId/topics" params={{ clusterId: card.id }}>
            topics
            <ArrowRight aria-hidden />
          </Link>
        </Button>
      </CardFooter>
    </Card>
  );
}
