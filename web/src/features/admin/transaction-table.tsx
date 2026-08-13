import { useState } from "react"
import { Link } from "@tanstack/react-router"

import type { Transaction } from "@/api/types"
import { HintHead, SortableHead } from "@/components/domain"
import { Badge } from "@/components/ui/badge"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { OpenFor } from "./open-for"
import { ProducerPanel } from "./producer-panel"

/**
 * The transaction inspector.
 *
 * Sorted by how long each has been open, descending, because that is the
 * question the screen exists to answer: a transaction open past its timeout is
 * holding the last stable offset back, and every `read_committed` consumer on
 * its partitions is stalled behind it.
 *
 * A row opens to the producers on its partitions — `DescribeProducers` routed
 * to each partition's leader — which is how the stuck writer is identified
 * rather than inferred.
 */
export function TransactionTable({
  envId,
  clusterId,
  transactions,
}: {
  envId: string
  clusterId: string
  transactions: Transaction[]
}) {
  const [ascending, setAscending] = useState(false)
  const [open, setOpen] = useState<string | null>(null)

  const rows = [...transactions].sort((a, b) => {
    // A transaction with no open one sorts last either way: it is not a
    // candidate for the thing this ordering is looking for.
    const left = a.startTimeMs ?? Infinity
    const right = b.startTimeMs ?? Infinity
    return ascending ? right - left : left - right
  })

  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <HintHead
              label="transactional id"
              hint="the id a producer claims to write transactionally under. Two producers cannot hold one at a time — the second fences the first"
            />
            <HintHead
              label="state"
              hint="the coordinator's own word for it, passed through unmapped: the set grows with Kafka releases"
            />
            <SortableHead
              label={`open for${ascending ? " ↑" : " ↓"}`}
              hint="since the current transaction began, ticking in your browser rather than frozen when the response was written"
              right
              onClick={() => setAscending(!ascending)}
            />
            <HintHead
              label="timeout"
              hint="what the producer configured. Open for longer than this is the state worth acting on"
              right
            />
            <HintHead
              label="producer"
              hint="the producer id holding the id, and its epoch — the epoch bumps when a producer is fenced by a newer one"
              right
            />
            <HintHead
              label="partitions"
              hint="the topic-partitions enrolled in the current transaction. These are the ones whose last stable offset is held back"
            />
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((txn) => {
            const expanded = open === txn.transactionalId
            const topics = txn.partitions.map((entry) => entry.topic)
            return (
              <TableRow key={txn.transactionalId}>
                <TableCell className="align-top">
                  <button
                    type="button"
                    className="font-mono text-[13px] hover:underline"
                    style={{ color: "var(--rust-ink)" }}
                    onClick={() =>
                      setOpen(expanded ? null : txn.transactionalId)
                    }
                  >
                    {txn.transactionalId}
                  </button>
                  {expanded ? (
                    <div className="mt-3 space-y-3">
                      {topics.length === 0 ? (
                        <p className="text-[12px] text-ink-muted">
                          No partitions enrolled — nothing is in flight under
                          this id, so there are no producers to look at.
                        </p>
                      ) : (
                        topics.map((topic) => (
                          <ProducerPanel
                            key={topic}
                            envId={envId}
                            clusterId={clusterId}
                            topic={topic}
                          />
                        ))
                      )}
                    </div>
                  ) : null}
                </TableCell>
                <TableCell className="align-top">
                  <Badge variant="outline" className="font-mono text-[11px]">
                    {txn.state}
                  </Badge>
                </TableCell>
                <TableCell className="text-right align-top font-mono">
                  <OpenFor
                    startTimeMs={txn.startTimeMs}
                    timeoutMs={txn.timeoutMs}
                  />
                </TableCell>
                <TableCell className="text-right align-top font-mono text-ink-muted">
                  {txn.timeoutMs === null ? "—" : `${txn.timeoutMs / 1000}s`}
                </TableCell>
                <TableCell className="text-right align-top font-mono">
                  {txn.producerId}
                  {txn.producerEpoch === null ? null : (
                    <span className="text-ink-faint">
                      {" "}
                      · e{txn.producerEpoch}
                    </span>
                  )}
                </TableCell>
                <TableCell className="align-top">
                  {txn.partitions.length === 0 ? (
                    <span className="text-ink-faint">—</span>
                  ) : (
                    <span className="flex flex-wrap gap-x-3 gap-y-1 text-[12px]">
                      {txn.partitions.map((entry) => (
                        <span key={entry.topic}>
                          <Link
                            to="/environments/$envId/clusters/$clusterId/topics/$topic"
                            params={{
                              envId,
                              clusterId,
                              topic: entry.topic,
                            }}
                            className="font-mono hover:underline"
                            style={{ color: "var(--rust-ink)" }}
                          >
                            {entry.topic}
                          </Link>
                          <span className="text-ink-faint">
                            {" "}
                            [{entry.partitions.join(", ")}]
                          </span>
                        </span>
                      ))}
                    </span>
                  )}
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}
