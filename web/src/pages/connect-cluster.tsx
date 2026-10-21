// One Kafka Connect cluster: its workers, its connectors, its plugins.
//
// A peer of a Kafka cluster inside its environment, dialled over the Connect
// REST API — read-only, and not by convention: the server's client has no
// method that sends anything but GET. There is no action button on this page,
// disabled or otherwise; a greyed "Restart" would be a promise this build
// does not make. Phase 10's write gate is where that changes.

import { useState } from "react"
import { Link, useNavigate, useSearch } from "@tanstack/react-router"
import { AlertTriangle } from "lucide-react"

import {
  useConnectCluster,
  useConnectPlugins,
  useConnectors,
} from "@/api/client"
import type {
  ConnectClusterDetail,
  ConnectorStatus,
  KafkaClusterCheck,
} from "@/api/types"
import {
  Empty,
  ErrorChips,
  Mono,
  RegistryStatusBadge,
  Spinner,
  Stat,
} from "@/components/domain"
import { PageTitle } from "@/components/page-title"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  ConnectorStatusBadge,
  connectorTypeLabel,
} from "@/features/connect/connector-status"
import { ConnectorTable } from "@/features/connect/connector-table"
import { cn } from "@/lib/utils"

export const CONNECT_VIEWS = ["connectors", "plugins"] as const
export type ConnectView = (typeof CONNECT_VIEWS)[number]

/** The states the filter offers, in the order a reader triages them. */
const FILTERS: ConnectorStatus[] = [
  "failed",
  "degraded",
  "running",
  "paused",
  "stopped",
]

export function ConnectClusterPage({
  envId,
  connectId,
}: {
  envId: string
  connectId: string
}) {
  const { listing } = useSearch({
    from: "/environments/$envId/connect-clusters/$connectId/",
  })
  const navigate = useNavigate()
  const detail = useConnectCluster(envId, connectId)

  if (detail.isLoading) return <Spinner label="asking the workers" />
  if (detail.error) {
    return (
      <>
        <PageTitle
          title="Kafka Connect"
          subtitle="This Connect cluster could not be read."
        />
        <p className="flex items-start gap-2 text-xs text-danger">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          {(detail.error as Error).message}
        </p>
      </>
    )
  }
  const data = detail.data
  if (!data) return null
  const { card } = data

  return (
    <>
      <PageTitle
        title={card.name}
        subtitle={
          <span className="flex flex-wrap items-center gap-x-2">
            <span>Kafka Connect</span>
            {card.info?.version ? (
              <span className="font-mono text-ink-faint">
                {card.info.version}
              </span>
            ) : null}
            <span className="truncate font-mono text-ink-faint">
              {card.url}
            </span>
          </span>
        }
        actions={
          <RegistryStatusBadge
            status={card.status}
            title={card.error ?? undefined}
          />
        }
      />

      <KafkaCheckBanner
        envId={envId}
        check={data.kafkaClusterCheck}
        reference={card.kafkaCluster}
      />
      <ErrorChips errors={data.errors} />

      <HeaderStats data={data} />

      <Tabs
        value={listing}
        onValueChange={(next) =>
          void navigate({
            to: "/environments/$envId/connect-clusters/$connectId",
            params: { envId, connectId },
            search: { listing: next as ConnectView },
            replace: true,
          })
        }
      >
        <TabsList>
          <TabsTrigger value="connectors">connectors</TabsTrigger>
          <TabsTrigger value="plugins">plugins</TabsTrigger>
        </TabsList>
        <TabsContent value="connectors" className="mt-4">
          <Connectors envId={envId} connectId={connectId} />
        </TabsContent>
        <TabsContent value="plugins" className="mt-4">
          <Plugins envId={envId} connectId={connectId} />
        </TabsContent>
      </Tabs>
    </>
  )
}

function HeaderStats({ data }: { data: ConnectClusterDetail }) {
  const counts = data.counts
  const failing = counts ? counts.failed + counts.degraded : 0
  return (
    <Card className="mb-6 py-4">
      <CardContent className="px-5">
        <dl className="grid grid-cols-2 gap-4 sm:grid-cols-5">
          <Stat
            label="connectors"
            value={counts ? String(counts.total) : "—"}
          />
          <Stat label="running" value={counts ? String(counts.running) : "—"} />
          <Stat
            label="failing"
            value={counts ? String(failing) : "—"}
            tone={failing > 0 ? "danger" : undefined}
            hint="failed, plus running with some tasks failed"
          />
          <Stat
            label="paused"
            value={counts ? String(counts.paused + counts.stopped) : "—"}
            hint="paused or stopped on purpose — not a failure"
          />
          <Stat
            label="plugins"
            value={data.pluginCount === null ? "—" : String(data.pluginCount)}
            hint="installed connector plugins; converters and transforms are not counted"
          />
        </dl>
      </CardContent>
    </Card>
  )
}

/**
 * Whether the workers keep their state in the Kafka cluster the config says.
 *
 * Silent when it checks out — the reference is on the fleet card — and loud
 * when it does not, because a Connect cluster attached to the wrong Kafka
 * cluster is exactly the mistake that looks fine everywhere else.
 */
function KafkaCheckBanner({
  envId,
  check,
  reference,
}: {
  envId: string
  check: KafkaClusterCheck
  reference: string | null
}) {
  if (check.state === "verified") {
    return reference ? (
      <p className="mb-4 text-[12px] text-ink-faint">
        state kept in{" "}
        <Link
          to="/environments/$envId/clusters/$clusterId"
          params={{ envId, clusterId: reference }}
          className="font-mono hover:underline"
        >
          {reference}
        </Link>{" "}
        — checked against the cluster id the workers report
      </p>
    ) : null
  }
  if (check.state === "mismatch") {
    return (
      <div
        className="mb-4 rounded-sm border p-3 text-[12px]"
        style={{
          background: "var(--danger-soft)",
          borderColor: "var(--danger)",
        }}
      >
        The workers keep their state in Kafka cluster{" "}
        <Mono>{check.actual}</Mono>, but <Mono>kafka_cluster: {reference}</Mono>{" "}
        is <Mono>{check.expected}</Mono>. The reference is wrong, or these
        workers are pointed somewhere unexpected.
      </div>
    )
  }
  if (check.state === "unverified") {
    return (
      <p className="mb-4 text-[12px] text-ink-faint">
        state said to be in <span className="font-mono">{reference}</span> — not
        yet checked:{" "}
        {check.expected
          ? "the workers have not answered"
          : "that cluster is not connected"}
      </p>
    )
  }
  return null
}

function Connectors({
  envId,
  connectId,
}: {
  envId: string
  connectId: string
}) {
  const [search, setSearch] = useState("")
  const [statuses, setStatuses] = useState<ConnectorStatus[]>([])
  const connectors = useConnectors(envId, connectId, {
    search,
    status: statuses,
  })

  if (connectors.isLoading) return <Spinner label="listing connectors" />
  if (connectors.error) {
    return (
      <p className="flex items-start gap-2 text-xs text-danger">
        <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
        {(connectors.error as Error).message}
      </p>
    )
  }
  const items = connectors.data?.connectors ?? []
  const total = connectors.data?.total ?? 0

  return (
    <>
      <div className="mb-4 flex flex-wrap items-center gap-3">
        <Input
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          placeholder="filter by name or class"
          aria-label="Filter connectors"
          className="h-8 max-w-xs"
        />
        <div
          className="flex flex-wrap gap-1.5"
          role="group"
          aria-label="Filter by state"
        >
          {FILTERS.map((status) => {
            const on = statuses.includes(status)
            return (
              <button
                key={status}
                type="button"
                aria-pressed={on}
                onClick={() =>
                  setStatuses(
                    on
                      ? statuses.filter((s) => s !== status)
                      : [...statuses, status]
                  )
                }
                className={cn(
                  "rounded-sm border px-2 py-0.5",
                  on
                    ? "border-[var(--accent)] bg-[var(--accent-soft)]"
                    : "opacity-70 hover:opacity-100"
                )}
              >
                <ConnectorStatusBadge status={status} />
              </button>
            )
          })}
        </div>
        <span className="ml-auto text-[12px] text-ink-faint">
          {items.length === total
            ? `${total} connectors`
            : `${items.length} of ${total}`}
        </span>
      </div>
      {items.length === 0 ? (
        <Empty>
          {total === 0
            ? "This Connect cluster runs no connectors."
            : "No connector matches that."}
        </Empty>
      ) : (
        <ConnectorTable envId={envId} connectId={connectId} items={items} />
      )}
    </>
  )
}

function Plugins({ envId, connectId }: { envId: string; connectId: string }) {
  const plugins = useConnectPlugins(envId, connectId)
  if (plugins.isLoading) return <Spinner label="listing plugins" />
  if (plugins.error) {
    return (
      <p className="flex items-start gap-2 text-xs text-danger">
        <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
        {(plugins.error as Error).message}
      </p>
    )
  }
  const items = plugins.data?.plugins ?? []
  if (items.length === 0)
    return <Empty>No connector plugins are installed.</Empty>
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>class</TableHead>
            <TableHead>type</TableHead>
            <TableHead>version</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {items.map((plugin) => (
            <TableRow key={plugin.class}>
              <TableCell className="font-mono text-[12px]">
                {plugin.class}
              </TableCell>
              <TableCell className="text-[12px] text-ink-muted">
                {connectorTypeLabel(plugin.type)}
              </TableCell>
              <TableCell className="font-mono text-[12px] text-ink-faint">
                {plugin.version ?? "—"}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
