import type { AnalysisStats } from "@/api/types"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { HintHead } from "@/components/domain"
import { bytes, count } from "@/lib/format"

export function PartitionStatsTable({
  partitions,
}: {
  partitions: AnalysisStats[]
}) {
  return (
    <div className="overflow-x-auto rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <HintHead
              label="partition"
              hint="its index within the topic"
              right
            />
            <HintHead
              label="messages"
              hint="records scanned in this partition — one carrying most of them is a skewed partitioning key"
              right
            />
            <HintHead
              label="min offset"
              hint="the lowest offset the scan read here"
              right
            />
            <HintHead
              label="max offset"
              hint="the highest offset the scan read here"
              right
            />
            <HintHead
              label="null keys"
              hint="records written without a key"
              right
            />
            <HintHead
              label="tombstones"
              hint="null-value records — deletion markers on a compacted topic"
              right
            />
            <HintHead
              label="≈ unique keys"
              hint="estimated distinct keys in this partition (sketch, ±1.6%)"
              right
            />
            <HintHead
              label="avg value"
              hint="the mean value size in this partition — exact"
              right
            />
            <HintHead
              label="bytes"
              hint="key plus value bytes scanned in this partition"
              right
            />
            <HintHead
              label="malformed"
              hint="batches that would not decode; skipped and counted"
              right
            />
          </TableRow>
        </TableHeader>
        <TableBody>
          {partitions.map((partition) => (
            <TableRow key={partition.partition ?? -1}>
              <TableCell className="text-right font-mono">
                {partition.partition}
              </TableCell>
              <TableCell className="text-right font-mono">
                {count(partition.totalMsgs)}
              </TableCell>
              <TableCell className="text-right font-mono">
                {count(partition.minOffset ?? null)}
              </TableCell>
              <TableCell className="text-right font-mono">
                {count(partition.maxOffset ?? null)}
              </TableCell>
              <TableCell className="text-right font-mono">
                {count(partition.nullKeys)}
              </TableCell>
              <TableCell className="text-right font-mono">
                {count(partition.nullValues)}
              </TableCell>
              <TableCell className="text-right font-mono">
                {count(partition.approxUniqKeys)}
              </TableCell>
              <TableCell className="text-right font-mono">
                {bytes(
                  partition.valueSize
                    ? Math.round(partition.valueSize.avg)
                    : null
                )}
              </TableCell>
              <TableCell className="text-right font-mono">
                {bytes(
                  (partition.keySize?.sum ?? 0) +
                    (partition.valueSize?.sum ?? 0)
                )}
              </TableCell>
              <TableCell
                className={
                  partition.malformedBatches > 0
                    ? "text-warn-ink text-right font-mono"
                    : "text-right font-mono"
                }
              >
                {count(partition.malformedBatches)}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
