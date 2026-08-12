import { Link } from "@tanstack/react-router"

import type { TopicSummary } from "@/api/types"
import { bytes, count } from "@/lib/format"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

/**
 * A metric cell in one of its three states.
 *
 * `—` and blank are different answers and must not look alike: blank means the
 * fan-out is still out, `—` means it came back and this topic has no number —
 * a partition that would not answer, or a broker with no `DescribeLogDirs`.
 * A dash that silently means "still loading" is how a cluster looks broken for
 * as long as it is slow.
 */
function Metric({
  value,
  render,
  pending,
}: {
  value: number | null
  render: (value: number) => string
  pending: boolean
}) {
  if (value !== null) return <>{render(value)}</>
  return (
    <span
      className="text-ink-faint"
      title={pending ? "still asking" : undefined}
    >
      {pending ? "·" : "—"}
    </span>
  )
}

export function TopicTable({
  envId,
  clusterId,
  items,
  replication,
  enriched,
  metricsPending,
  sort,
  order,
  onSort,
}: {
  envId: string
  clusterId: string
  items: TopicSummary[]
  replication: boolean
  enriched: Map<string, TopicSummary>
  metricsPending: boolean
  sort: string
  order: "asc" | "desc"
  onSort: (column: string) => void
}) {
  const heading = (label: string, column: string, right?: boolean) => (
    <TableHead className={right ? "text-right" : undefined}>
      <button
        type="button"
        onClick={() => onSort(column)}
        className="hover:underline"
      >
        {label}
        {sort === column ? (order === "asc" ? " ↑" : " ↓") : ""}
      </button>
    </TableHead>
  )

  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            {heading("name", "name")}
            {heading("partitions", "partitions", true)}
            {replication
              ? heading("out of sync", "underReplicated", true)
              : null}
            {replication ? (
              <TableHead className="text-right">rf</TableHead>
            ) : null}
            {heading("messages", "messages", true)}
            {heading("size", "size", true)}
          </TableRow>
        </TableHeader>
        <TableBody>
          {items.map((topic) => {
            // The base row already carries the numbers when the sort is
            // a metric, because the server had to compute them to order
            // by them. Otherwise they arrive on the second request.
            const row = enriched.get(topic.name) ?? topic
            return (
              <TableRow key={topic.name}>
                <TableCell>
                  <Link
                    to="/environments/$envId/clusters/$clusterId/topics/$topic"
                    params={{ envId, clusterId, topic: topic.name }}
                    className="font-mono hover:underline"
                    style={{ color: "var(--rust-ink)" }}
                  >
                    {topic.name}
                  </Link>
                  {topic.internal ? (
                    <span className="ml-2 text-[11px] text-ink-faint">
                      internal
                    </span>
                  ) : null}
                </TableCell>
                {/* Offline partitions ride in this cell rather than in a
                    column of their own: on a healthy cluster that column
                    is a stripe of zeroes, and the one row that matters is
                    easier to see against plain numbers than against them. */}
                <TableCell className="text-right font-mono whitespace-nowrap">
                  {topic.partitionCount}
                  {topic.offlinePartitionCount > 0 ? (
                    <span
                      className="text-danger ml-1.5 font-medium"
                      title={`${topic.offlinePartitionCount} partition(s) with no leader or an offline replica`}
                    >
                      ✕{topic.offlinePartitionCount}
                    </span>
                  ) : null}
                </TableCell>
                {replication ? (
                  <TableCell className="text-right">
                    {topic.underReplicatedPartitionCount > 0 ? (
                      <span className="font-mono font-medium text-warn-ink">
                        △ {topic.underReplicatedPartitionCount}
                      </span>
                    ) : (
                      <span className="text-ink-faint">0</span>
                    )}
                  </TableCell>
                ) : null}
                {replication ? (
                  <TableCell className="text-right font-mono">
                    {topic.replicationFactor}
                  </TableCell>
                ) : null}
                <TableCell className="text-right font-mono">
                  <Metric
                    value={row.messageCount}
                    render={count}
                    pending={metricsPending}
                  />
                </TableCell>
                <TableCell className="text-right font-mono">
                  <Metric
                    value={row.replicatedBytes}
                    render={bytes}
                    pending={metricsPending}
                  />
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}
