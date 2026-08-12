import { useTopicSize } from "@/api/client"
import type { Partition } from "@/api/types"
import { HintHead, PlacementLegend, placementCell } from "@/components/domain"
import { bytes, count } from "@/lib/format"
import { cn } from "@/lib/utils"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

import { pending } from "@/features/topics/pending"

/**
 * Partitions, with the replica placement in the same rows.
 *
 * These were two tabs. They are one table because they were always one
 * question asked twice: the grid said *where* a partition lives and the table
 * said *what state* it is in, and answering "which broker is the out-of-sync
 * replica on, and how far behind is that partition" meant holding one view in
 * your head while looking at the other.
 *
 * The broker columns come first, right after the partition number, because
 * that block is the shape you scan — a column of `L` glyphs drifting to one
 * broker is a leader imbalance, and a gap in it is visible before any number
 * is read.
 */
export function PartitionTable({
  partitions,
  brokerIds,
  envId,
  clusterId,
  topic,
}: {
  partitions: Partition[]
  brokerIds: number[]
  envId: string
  clusterId: string
  topic: string
}) {
  // The same query the card reads, so this costs no second fan-out — and it
  // is joined by partition index rather than by position, because a partition
  // no broker reported a copy of is absent from the size answer and would
  // otherwise slide every row below it onto the wrong number.
  const size = useTopicSize(envId, clusterId, topic)
  const sizes = new Map(
    (size.data?.items[0]?.partitions ?? []).map((partition) => [
      partition.partition,
      partition.replicatedBytes,
    ])
  )
  const lags = new Map(
    (size.data?.items[0]?.partitions ?? []).map((partition) => [
      partition.partition,
      partition.maxFollowerLag,
    ])
  )
  // From the describe, not the size answer, so the column does not pop in
  // when the sizes arrive. A topic with no followers has nobody to lag, and
  // a column of dashes would only say so repeatedly.
  const hasFollowers = partitions.some(
    (partition) => partition.replicas.length > 1
  )

  return (
    <div className="space-y-3">
      <div className="overflow-x-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <HintHead
                label="partition"
                hint="its index within the topic"
                right
              />
              {/* One column per broker. Narrow and centred so the block reads
                  as a grid rather than as eight more columns of data. */}
              {brokerIds.map((broker, index) => (
                <HintHead
                  key={broker}
                  label={broker}
                  hint={`broker ${broker} — what it holds of each partition`}
                  className={cn(
                    "px-1 text-center font-mono font-normal",
                    index === 0 && "border-line border-l",
                    index === brokerIds.length - 1 && "border-line border-r"
                  )}
                />
              ))}
              <HintHead
                label="epoch"
                hint="leader epoch — it bumps on every leadership change"
                right
              />
              <HintHead
                label="earliest"
                hint="the oldest offset still retained"
                right
              />
              <HintHead
                label="latest"
                hint="the offset the next record will get"
                right
              />
              <HintHead
                label="records"
                hint="latest − earliest: what is retained, not what was ever written"
                right
              />
              <HintHead
                label="size"
                hint="bytes on disk for every non-future copy of this partition"
                right
              />
              {hasFollowers ? (
                <HintHead
                  label="lag"
                  hint="the worst follower's offset lag — 0 is every follower caught up"
                  right
                />
              ) : null}
            </TableRow>
          </TableHeader>
          <TableBody>
            {partitions.map((partition) => {
              const records =
                partition.earliestOffset !== null &&
                partition.latestOffset !== null
                  ? partition.latestOffset - partition.earliestOffset
                  : null
              return (
                <TableRow key={partition.partition}>
                  <TableCell className="text-right font-mono whitespace-nowrap">
                    {partition.partition}
                    {/* Every other state has a glyph. "No leader at all" has
                        the *absence* of one, which reads as nothing wrong
                        unless it is said. */}
                    {partition.leader === null ? (
                      <span className="text-danger ml-1.5" title="no leader">
                        ✕
                      </span>
                    ) : null}
                  </TableCell>
                  {brokerIds.map((broker, index) => {
                    const { label, style, title, preferred } = placementCell(
                      partition,
                      broker
                    )
                    return (
                      <TableCell
                        key={broker}
                        className={cn(
                          "px-1 py-0.5",
                          index === 0 && "border-line border-l",
                          index === brokerIds.length - 1 &&
                            "border-line border-r"
                        )}
                      >
                        <div
                          title={`p${partition.partition} on broker ${broker}: ${title}`}
                          style={style}
                          className={cn(
                            "mx-auto grid h-5 w-6 place-items-center rounded-[2px] font-mono text-[12px]",
                            preferred &&
                              "outline-ink-muted outline-2 -outline-offset-1"
                          )}
                        >
                          {label}
                        </div>
                      </TableCell>
                    )
                  })}
                  <TableCell className="text-ink-faint text-right font-mono">
                    {partition.leaderEpoch}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {count(partition.earliestOffset)}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {count(partition.latestOffset)}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {count(records)}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {pending(
                      sizes.get(partition.partition) ?? null,
                      bytes,
                      size.isFetching
                    )}
                  </TableCell>
                  {hasFollowers ? (
                    <TableCell
                      className={cn(
                        "text-right font-mono",
                        (lags.get(partition.partition) ?? 0) > 0 &&
                          "text-warn-ink"
                      )}
                    >
                      {pending(
                        lags.get(partition.partition) ?? null,
                        count,
                        size.isFetching
                      )}
                    </TableCell>
                  ) : null}
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      </div>
      {brokerIds.length > 0 ? <PlacementLegend /> : null}
    </div>
  )
}
