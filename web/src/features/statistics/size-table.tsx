import type { SizeStats } from "@/api/types"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { HintHead } from "@/components/domain"
import { bytes } from "@/lib/format"

const PERCENTILES: Array<{
  label: string
  hint: string
  pick(stats: SizeStats): number | undefined
  exact: boolean
}> = [
  {
    label: "min",
    hint: "the smallest — exact",
    pick: (s) => s.min,
    exact: true,
  },
  {
    label: "avg",
    hint: "the mean — exact",
    pick: (s) => Math.round(s.avg),
    exact: true,
  },
  {
    label: "p50",
    hint: "the median: half the records are smaller — a sketch estimate (±4%)",
    pick: (s) => s.p50,
    exact: false,
  },
  {
    label: "p75",
    hint: "three quarters are smaller — estimate",
    pick: (s) => s.p75,
    exact: false,
  },
  {
    label: "p95",
    hint: "19 of 20 are smaller — estimate",
    pick: (s) => s.p95,
    exact: false,
  },
  {
    label: "p99",
    hint: "99% are smaller — estimate; the usual sizing figure",
    pick: (s) => s.p99,
    exact: false,
  },
  {
    label: "p99.9",
    hint: "999 of 1000 are smaller — estimate; the outliers",
    pick: (s) => s.p999,
    exact: false,
  },
  {
    label: "max",
    hint: "the largest single record — exact",
    pick: (s) => s.max,
    exact: true,
  },
  {
    label: "sum",
    hint: "every record summed — exact",
    pick: (s) => s.sum,
    exact: true,
  },
]

export function SizeTable({
  keySize,
  valueSize,
}: {
  keySize?: SizeStats
  valueSize?: SizeStats
}) {
  return (
    <div className="overflow-x-auto">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead />
            {PERCENTILES.map((column) => (
              <HintHead
                key={column.label}
                label={column.exact ? column.label : `≈ ${column.label}`}
                hint={column.hint}
                right
              />
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {(
            [
              ["key", keySize],
              ["value", valueSize],
            ] as const
          ).map(([name, stats]) => (
            <TableRow key={name}>
              <TableCell className="text-ink-muted">{name}</TableCell>
              {PERCENTILES.map((column) => (
                <TableCell
                  key={column.label}
                  className="text-right font-mono whitespace-nowrap"
                >
                  {stats ? bytes(column.pick(stats) ?? null) : "—"}
                </TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
