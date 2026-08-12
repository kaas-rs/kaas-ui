import { Link } from "@tanstack/react-router"

import { useGroupOffsets } from "@/api/client"
import {
  Empty,
  ErrorChips,
  LagCell,
  Section,
  Spinner,
} from "@/components/domain"
import { count } from "@/lib/format"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

export function GroupOffsets({
  envId,
  clusterId,
  groupId,
}: {
  envId: string
  clusterId: string
  groupId: string
}) {
  const offsets = useGroupOffsets(envId, clusterId, groupId)

  return (
    <Section title="Committed offsets">
      <ErrorChips errors={offsets.data?.errors ?? []} />
      {offsets.isLoading ? (
        <Spinner />
      ) : (offsets.data?.items.length ?? 0) === 0 ? (
        <Empty>this group has committed no offsets</Empty>
      ) : (
        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>topic</TableHead>
                <TableHead className="text-right">partition</TableHead>
                <TableHead className="text-right">committed</TableHead>
                <TableHead className="text-right">log end</TableHead>
                <TableHead className="text-right">lag</TableHead>
                <TableHead>metadata</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {(offsets.data?.items ?? []).map((row) => (
                <TableRow key={`${row.topic}-${row.partition}`}>
                  <TableCell>
                    <Link
                      to="/environments/$envId/clusters/$clusterId/topics/$topic"
                      params={{ envId, clusterId, topic: row.topic }}
                      className="font-mono hover:underline"
                      style={{ color: "var(--rust-ink)" }}
                    >
                      {row.topic}
                    </Link>
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {row.partition}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {count(row.committedOffset)}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {count(row.latestOffset)}
                  </TableCell>
                  <TableCell className="text-right">
                    <LagCell lag={row.lag} />
                  </TableCell>
                  <TableCell className="font-mono text-[12px] text-ink-faint">
                    {row.metadata ?? ""}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </Section>
  )
}
