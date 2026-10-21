import { useState } from "react"

import { useBrokerTls, useLogDirs } from "@/api/client"
import { HintHead, Spinner } from "@/components/domain"
import { count } from "@/lib/format"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import type { Broker } from "@/api/types"
import { ExpandLink } from "./expand-link"
import { LogDirsTable } from "./log-dirs-table"
import { TlsCell, TlsDetail, brokerError } from "./tls-panel"

/**
 * The broker list, with one expandable panel below it: a broker's certificate
 * chain or its log dirs, never both — opening one closes the other, so the
 * panel under the table always answers the last thing clicked.
 *
 * Log dirs are fetched per broker and lazily, so nothing is read until someone
 * asks. Certificates
 * are fetched up front instead — one request, read off connections that
 * mostly exist already — because the expiry is worth seeing without asking.
 * The cluster card's summary reads the same query, so it is still one request.
 */
/** The one panel open below the table, and whose it is. */
interface Panel {
  kind: "tls" | "logDirs"
  nodeId: number
}

export function BrokerTable({
  envId,
  clusterId,
  brokers,
}: {
  envId: string
  clusterId: string
  brokers: Broker[]
}) {
  const [open, setOpen] = useState<Panel | null>(null)
  const isOpen = (kind: Panel["kind"], nodeId: number) =>
    open?.kind === kind && open.nodeId === nodeId
  const toggle = (kind: Panel["kind"], nodeId: number) =>
    setOpen(isOpen(kind, nodeId) ? null : { kind, nodeId })

  const logDirs = useLogDirs(
    envId,
    clusterId,
    open?.kind === "logDirs" ? open.nodeId : null
  )
  const tls = useBrokerTls(envId, clusterId)
  const tlsItems = tls.data?.items ?? []
  const tlsErrors = tls.data?.errors ?? []
  const tlsFor = (nodeId: number) =>
    tlsItems.find((item) => item.nodeId === nodeId)

  return (
    <>
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <HintHead
                label="node"
                hint="the broker id, which is configured rather than assigned"
              />
              <HintHead
                label="host"
                hint="the listener the broker advertises — what clients are told to dial"
              />
              <HintHead
                label="port"
                hint="the advertised port, not necessarily the one it binds"
                right
              />
              <HintHead
                label="rack"
                hint="the failure domain the broker declares; blank where it declares none"
              />
              <HintHead
                label="leads"
                hint="partitions this broker is leader of — every read and write goes here"
                right
              />
              <HintHead
                label="replicas"
                hint="partitions it holds a copy of, the ones it leads included"
                right
              />
              <HintHead
                label="role"
                hint="broker, controller, or both on a dual-role node"
              />
              <HintHead
                label="certificate"
                hint="the earliest expiry in the chain this broker presented to kaas-ui — click for the chain"
              />
              <HintHead
                label="log dirs"
                hint="its data directories and what they hold — fetched per broker, on ask"
              />
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
                  <TlsCell
                    tls={tlsFor(broker.nodeId)}
                    error={brokerError(tlsErrors, broker.nodeId)}
                    loading={tls.isLoading}
                    open={isOpen("tls", broker.nodeId)}
                    onToggle={() => toggle("tls", broker.nodeId)}
                  />
                </TableCell>
                <TableCell>
                  <ExpandLink
                    open={isOpen("logDirs", broker.nodeId)}
                    onToggle={() => toggle("logDirs", broker.nodeId)}
                  >
                    show
                  </ExpandLink>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      {open?.kind === "tls" ? (
        <div className="mt-4">
          <TlsDetail
            nodeId={open.nodeId}
            tls={tlsFor(open.nodeId)}
            error={brokerError(tlsErrors, open.nodeId)}
          />
        </div>
      ) : open?.kind === "logDirs" ? (
        <div className="mt-4">
          {logDirs.isLoading ? (
            <Spinner label={`reading log dirs on broker ${open.nodeId}`} />
          ) : logDirs.error ? (
            <Card className="p-4 text-[13px] text-danger">
              broker {open.nodeId}: {String(logDirs.error)}
            </Card>
          ) : (
            <LogDirsTable
              broker={open.nodeId}
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
          <TooltipTrigger
            render={<span className="text-[11px] text-ink-faint" />}
          >
            fencing unknown
          </TooltipTrigger>
          <TooltipContent>
            this cluster does not report fencing — unknown, not false
          </TooltipContent>
        </Tooltip>
      ) : null}
    </div>
  )
}
