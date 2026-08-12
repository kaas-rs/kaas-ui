import { Link } from "@tanstack/react-router"

import type { GroupSummary } from "@/api/types"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"

export function GroupTable({
  envId,
  clusterId,
  items,
}: {
  envId: string
  clusterId: string
  items: GroupSummary[]
}) {
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>group</TableHead>
            <TableHead>state</TableHead>
            <TableHead>type</TableHead>
            <TableHead>protocol</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {items.map((group) => (
            <TableRow key={group.groupId}>
              <TableCell>
                {group.describable ? (
                  <Link
                    to="/environments/$envId/clusters/$clusterId/groups/$groupId"
                    params={{ envId, clusterId, groupId: group.groupId }}
                    className="font-mono hover:underline"
                    style={{ color: "var(--rust-ink)" }}
                  >
                    {group.groupId}
                  </Link>
                ) : (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span className="font-mono text-ink-muted">
                        {group.groupId}
                      </span>
                    </TooltipTrigger>
                    <TooltipContent>
                      this build has no schema for this group kind
                    </TooltipContent>
                  </Tooltip>
                )}
              </TableCell>
              <TableCell>
                <GroupState state={group.state} />
              </TableCell>
              <TableCell className="font-mono text-ink-muted">
                {group.groupType || (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span className="text-ink-faint">unreported</span>
                    </TooltipTrigger>
                    <TooltipContent>
                      this broker is too old to report a group type; it takes
                      the classic path
                    </TooltipContent>
                  </Tooltip>
                )}
              </TableCell>
              <TableCell className="font-mono text-ink-muted">
                {group.protocolType}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

function GroupState({ state }: { state: string }) {
  const tone =
    state === "Stable"
      ? "text-ok"
      : state === "Empty" || state === "Dead"
        ? "text-ink-faint"
        : "text-warn-ink"
  return <span className={`text-[12px] font-medium ${tone}`}>{state}</span>
}
