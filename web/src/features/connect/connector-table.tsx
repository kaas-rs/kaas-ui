import { Link } from "@tanstack/react-router"

import type { ConnectorSummary } from "@/api/types"
import { HintHead } from "@/components/domain"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  ConnectorStatusBadge,
  connectorTypeLabel,
  shortClass,
} from "@/features/connect/connector-status"
import { cn } from "@/lib/utils"

export function ConnectorTable({
  envId,
  connectId,
  items,
}: {
  envId: string
  connectId: string
  items: ConnectorSummary[]
}) {
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <HintHead label="connector" hint="the name it was created under" />
            <HintHead
              label="state"
              hint="the connector's state folded with its tasks' — paused is chosen, not failed"
            />
            <HintHead
              label="type"
              hint="source writes to Kafka, sink reads from it"
            />
            <HintHead
              label="class"
              hint="connector.class, by its last segment; hover for the whole name"
            />
            <HintHead
              label="tasks"
              hint="running of total; failed ones counted beside"
            />
            <HintHead
              label="worker"
              hint="where the connector itself runs; its tasks may be elsewhere"
            />
          </TableRow>
        </TableHeader>
        <TableBody>
          {items.map((connector) => (
            <TableRow key={connector.name}>
              <TableCell>
                <Link
                  to="/environments/$envId/connect-clusters/$connectId/connectors/$connector"
                  params={{ envId, connectId, connector: connector.name }}
                  className="font-mono hover:underline"
                  style={{ color: "var(--rust-ink)" }}
                >
                  {connector.name}
                </Link>
              </TableCell>
              <TableCell>
                <ConnectorStatusBadge status={connector.status} />
              </TableCell>
              <TableCell className="text-[12px] text-ink-muted">
                {connectorTypeLabel(connector.type)}
              </TableCell>
              <TableCell
                className="font-mono text-[12px] text-ink-muted"
                title={connector.class ?? undefined}
              >
                {shortClass(connector.class)}
              </TableCell>
              <TableCell className="font-mono text-[12px]">
                <span>
                  {connector.tasksRunning}/{connector.tasks.length}
                </span>
                {connector.tasksFailed > 0 ? (
                  <span className={cn("ml-2 text-danger")}>
                    {connector.tasksFailed} failed
                  </span>
                ) : null}
              </TableCell>
              <TableCell
                className="max-w-[16rem] truncate font-mono text-[12px] text-ink-faint"
                title={connector.workerId ?? undefined}
              >
                {connector.workerId ?? "—"}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
