import { bytes, count } from "@/lib/format"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import type { LogDir } from "@/api/types"

/** One broker's log directories: capacity, and what of it Kafka occupies. */
export function LogDirsTable({
  broker,
  dirs,
}: {
  broker: number
  dirs: LogDir[]
}) {
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>path (broker {broker})</TableHead>
            <TableHead className="text-right">total</TableHead>
            <TableHead className="text-right">usable</TableHead>
            <TableHead className="text-right">replicas</TableHead>
            <TableHead className="text-right">on disk</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {dirs.map((dir) => (
            <TableRow key={dir.path}>
              <TableCell className="font-mono">{dir.path}</TableCell>
              <TableCell className="text-right">
                {bytes(dir.totalBytes)}
              </TableCell>
              <TableCell className="text-right">
                {bytes(dir.usableBytes)}
              </TableCell>
              <TableCell className="text-right">
                {count(dir.replicas.length)}
              </TableCell>
              <TableCell className="text-right">
                {bytes(
                  dir.replicas.reduce(
                    (total, replica) => total + replica.sizeBytes,
                    0
                  )
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
