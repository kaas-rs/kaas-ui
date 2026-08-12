import { useState } from "react"

import { useLogDirs } from "@/api/client"
import { Spinner } from "@/components/domain"
import { count } from "@/lib/format"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import type { Broker } from "@/api/types"
import { LogDirsTable } from "./log-dirs-table"

/**
 * The broker list, with an expandable per-broker log-dirs view.
 *
 * Which broker's log dirs are open is this table's own state: the fan-out is
 * per broker and lazy, so nothing is fetched until someone asks.
 */
export function BrokerTable({
  envId,
  clusterId,
  brokers,
}: {
  envId: string
  clusterId: string
  brokers: Broker[]
}) {
  const [logDirBroker, setLogDirBroker] = useState<number | null>(null)
  const logDirs = useLogDirs(envId, clusterId, logDirBroker)

  return (
    <>
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>node</TableHead>
              <TableHead>host</TableHead>
              <TableHead className="text-right">port</TableHead>
              <TableHead>rack</TableHead>
              <TableHead className="text-right">leads</TableHead>
              <TableHead className="text-right">replicas</TableHead>
              <TableHead>role</TableHead>
              <TableHead>log dirs</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {brokers.map((broker) => (
              <TableRow key={broker.nodeId}>
                <TableCell className="font-mono">{broker.nodeId}</TableCell>
                <TableCell className="font-mono text-ink-muted">
                  {broker.host}
                </TableCell>
                <TableCell className="text-right font-mono">
                  {broker.port}
                </TableCell>
                <TableCell>
                  {broker.rack ?? <span className="text-ink-faint">—</span>}
                </TableCell>
                <TableCell className="text-right font-mono">
                  {count(broker.leaderPartitionCount)}
                </TableCell>
                <TableCell className="text-right font-mono">
                  {count(broker.replicaPartitionCount)}
                </TableCell>
                <TableCell>
                  <BrokerRoleBadges broker={broker} />
                </TableCell>
                <TableCell>
                  <Button
                    variant="link"
                    size="sm"
                    className="h-auto p-0 text-[12px]"
                    onClick={() =>
                      setLogDirBroker(
                        logDirBroker === broker.nodeId ? null : broker.nodeId
                      )
                    }
                  >
                    {logDirBroker === broker.nodeId ? "hide" : "show"}
                  </Button>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      {logDirBroker !== null ? (
        <div className="mt-4">
          {logDirs.isLoading ? (
            <Spinner label={`reading log dirs on broker ${logDirBroker}`} />
          ) : logDirs.error ? (
            <Card className="p-4 text-[13px] text-danger">
              broker {logDirBroker}: {String(logDirs.error)}
            </Card>
          ) : (
            <LogDirsTable
              broker={logDirBroker}
              dirs={logDirs.data?.items ?? []}
            />
          )}
        </div>
      ) : null}
    </>
  )
}

function BrokerRoleBadges({ broker }: { broker: Broker }) {
  return (
    <div className="flex gap-2">
      {broker.isController ? (
        <Badge
          style={{
            background: "var(--rust)",
            color: "#3B2E2A",
          }}
          className="border-transparent"
        >
          controller
        </Badge>
      ) : null}
      {broker.isFenced === true ? (
        <Badge
          style={{
            background: "var(--danger-soft)",
            color: "var(--danger)",
          }}
          className="border-transparent"
        >
          fenced
        </Badge>
      ) : broker.isFenced === null ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <span className="text-[11px] text-ink-faint">fencing unknown</span>
          </TooltipTrigger>
          <TooltipContent>
            this cluster does not report fencing — unknown, not false
          </TooltipContent>
        </Tooltip>
      ) : null}
    </div>
  )
}
