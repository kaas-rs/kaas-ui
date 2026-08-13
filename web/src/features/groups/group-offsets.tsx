import { Link } from "@tanstack/react-router"

import { useGroupOffsets } from "@/api/client"
import {
  Empty,
  ErrorChips,
  HintHead,
  LagCell,
  Section,
  Spinner,
} from "@/components/domain"
import { count } from "@/lib/format"
import {
  Table,
  TableBody,
  TableCell,
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
                <HintHead
                  label="topic"
                  hint="a topic this group has committed an offset for — it need not still be subscribed"
                />
                <HintHead
                  label="partition"
                  hint="its index within the topic"
                  right
                />
                <HintHead
                  label="committed"
                  hint="the offset the group will resume from, which is the next record it has not read"
                  right
                />
                <HintHead
                  label="log end"
                  hint="the offset the next record written will get"
                  right
                />
                <HintHead
                  label="lag"
                  hint="log end − committed: records written and not yet read"
                  right
                />
                <HintHead
                  label="metadata"
                  hint="whatever the member attached to the commit; usually empty"
                />
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
