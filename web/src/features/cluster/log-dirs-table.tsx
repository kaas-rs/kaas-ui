import { bytes, count } from "@/lib/format"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { HintHead } from "@/components/domain"
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
            <HintHead
              label={`path (broker ${broker})`}
              hint="a directory this broker stores log segments in"
            />
            <HintHead
              label="total"
              hint="the size of the filesystem holding it, not a Kafka quota"
              right
            />
            <HintHead
              label="usable"
              hint="what the filesystem says is free — everything on it counts, Kafka or not"
              right
            />
            <HintHead
              label="replicas"
              hint="partition copies stored here, leaders and followers alike"
              right
            />
            <HintHead
              label="on disk"
              hint="those replicas summed: what Kafka occupies of the total"
              right
            />
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
