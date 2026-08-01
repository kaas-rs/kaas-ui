import { Link } from "@tanstack/react-router";
import { useClusters } from "../api/client";
import type { ClusterCard } from "../api/types";
import {
  Card,
  ClusterChip,
  ClusterCounts,
  SnapshotAge,
  Spinner,
  StatusBadge,
} from "../components";
import { PageTitle } from "../shell";

/** Group by `env`, then `kind` — the two labels the fleet is organised by. */
function groupKey(card: ClusterCard): string {
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
  const groups = new Map<string, ClusterCard[]>();
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
          <h2 className="text-[12px] uppercase tracking-wide text-ink-faint mb-3">
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

function FleetCard({ card }: { card: ClusterCard }) {
  const unreachable = card.status === "unreachable";
  const clusters = useClusters();

  return (
    <Card
      className="p-4 flex flex-col gap-3"
      // The dead cluster must be visually distinct without being the loudest
      // thing on the page: a fleet where one cluster is always down should not
      // train anyone to ignore red.
    >
      <div className="flex items-start justify-between gap-3">
        <Link
          to="/clusters/$clusterId"
          params={{ clusterId: card.id }}
          className="font-semibold hover:underline"
          style={{ color: "var(--color-accent-ink)" }}
        >
          {card.name}
        </Link>
        <StatusBadge status={card.status} />
      </div>

      <div className="flex items-center gap-2">
        <ClusterChip id={card.id} labels={card.labels} size="small" />
        {card.clusterId ? (
          <span className="font-mono text-[11px] text-ink-faint truncate">
            {card.clusterId}
          </span>
        ) : null}
      </div>

      {unreachable ? (
        <div
          className="text-[12px] p-3 rounded-sm border"
          style={{
            background: "var(--color-danger-soft)",
            borderColor: "var(--color-danger)",
          }}
        >
          <p className="font-mono break-words">{card.error}</p>
          <div className="flex items-center justify-between mt-2 gap-3">
            <span className="text-ink-muted">
              {card.attempts} failed attempt{card.attempts === 1 ? "" : "s"}
            </span>
            <button
              type="button"
              // Asking again is a GET. The server nudges its connector to
              // retry now rather than at the end of the backoff, which is how
              // a retry button exists in an application with no non-GET route.
              onClick={() => void clusters.refetch()}
              className="px-2 py-1 rounded-sm border border-line-strong hover:bg-surface-sunken"
            >
              retry now
            </button>
          </div>
        </div>
      ) : (
        <ClusterCounts card={card} />
      )}

      <div className="mt-auto pt-2 border-t border-line flex items-center justify-between">
        <SnapshotAge ageMs={card.snapshotAgeMs} maxStalenessMs={card.maxStalenessMs} />
        <Link
          to="/clusters/$clusterId/topics"
          params={{ clusterId: card.id }}
          className="text-[12px] hover:underline"
          style={{ color: "var(--color-link)" }}
        >
          topics →
        </Link>
      </div>
    </Card>
  );
}
