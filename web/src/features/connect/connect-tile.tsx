import { Link } from "@tanstack/react-router"
import { ArrowRight, RefreshCw } from "lucide-react"

import { useConnectCluster } from "@/api/client"
import type { ConnectClusterCard } from "@/api/types"
import { RESOURCE_KINDS, RegistryStatusBadge, Stat } from "@/components/domain"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardFooter, CardHeader } from "@/components/ui/card"

/**
 * A Connect cluster on the fleet: dialled, so built like the registry tile —
 * status, numbers, a page to open.
 *
 * The counts are fetched here rather than ridden along on `/api/environments`
 * for the registry's reason: the fleet response must not wait on workers that
 * are not answering, and one Connect cluster's outage must degrade one card.
 * Asking is what probes, so a Connect cluster stops reading `unprobed` the
 * moment somebody looks at the fleet.
 */
export function ConnectTile({
  envId,
  card,
}: {
  envId: string
  card: ConnectClusterCard
}) {
  const Icon = RESOURCE_KINDS.kafka_connect.icon
  const detail = useConnectCluster(envId, card.id)
  const live = detail.data?.card ?? card
  const broken =
    live.status === "unreachable" || live.status === "misconfigured"
  const counts = detail.data?.counts ?? null
  const error = detail.error
    ? (detail.error as Error).message
    : broken
      ? (live.error ?? live.status)
      : null
  const params = { envId, connectId: card.id }

  return (
    <Card className="gap-3 py-4">
      <CardHeader className="gap-2 px-4">
        <div className="flex items-start justify-between gap-3">
          <Link
            to="/environments/$envId/connect-clusters/$connectId"
            params={params}
            className="font-semibold hover:underline"
            style={{ color: "var(--rust-ink)" }}
          >
            {card.name}
          </Link>
          <RegistryStatusBadge
            status={live.status}
            title={live.error ?? undefined}
          />
        </div>
        <div className="flex items-center gap-1.5 text-[12px] text-ink-muted">
          <Icon aria-hidden className="size-3.5" />
          Kafka Connect
          {live.info?.version ? (
            <span className="font-mono text-[11px] text-ink-faint">
              {live.info.version}
            </span>
          ) : null}
        </div>
      </CardHeader>

      <CardContent className="px-4">
        {error ? (
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
                  ? "check the url and credentials"
                  : "connector state is unknown until it answers"}
              </span>
              <Button
                variant="outline"
                size="sm"
                onClick={() => void detail.refetch()}
              >
                <RefreshCw aria-hidden />
                retry now
              </Button>
            </div>
          </div>
        ) : (
          <dl className="grid grid-cols-3 gap-3">
            <Stat
              label="connectors"
              value={counts ? String(counts.total) : "…"}
            />
            <Stat
              label="failing"
              value={counts ? String(counts.failed + counts.degraded) : "…"}
              tone={
                counts && counts.failed + counts.degraded > 0
                  ? "danger"
                  : undefined
              }
              hint="failed, plus running with some tasks failed"
            />
            <Stat
              label="paused"
              value={counts ? String(counts.paused + counts.stopped) : "…"}
              hint="paused or stopped on purpose — not a failure"
            />
          </dl>
        )}
        <p
          className="mt-3 truncate font-mono text-[12px] text-ink-faint"
          title={card.url}
        >
          {card.url}
        </p>
      </CardContent>

      <CardFooter className="mt-auto border-t px-4 pt-3 [.border-t]:pt-3">
        <span className="min-w-0 truncate text-[12px] text-ink-faint">
          {card.kafkaCluster ? (
            <>
              state in <span className="font-mono">{card.kafkaCluster}</span>
            </>
          ) : (
            "no Kafka cluster referenced"
          )}
        </span>
        <Button variant="link" size="sm" asChild className="ml-auto h-auto p-0">
          <Link
            to="/environments/$envId/connect-clusters/$connectId"
            params={params}
          >
            connectors
            <ArrowRight aria-hidden />
          </Link>
        </Button>
      </CardFooter>
    </Card>
  )
}
