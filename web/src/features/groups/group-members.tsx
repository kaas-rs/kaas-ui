import type { GroupMember } from "@/api/types"
import { Empty } from "@/components/domain"
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

export function GroupMembers({ members }: { members: GroupMember[] }) {
  if (members.length === 0) {
    return <Empty>no members — the group exists but nothing is consuming</Empty>
  }
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>member</TableHead>
            <TableHead>client</TableHead>
            <TableHead>host</TableHead>
            <TableHead className="text-right">epoch</TableHead>
            <TableHead>assignment</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {members.map((member) => (
            <TableRow key={member.memberId}>
              <TableCell>
                <span className="font-mono text-[12px] break-all">
                  {member.memberId}
                </span>
                {member.instanceId ? (
                  <span className="block text-[11px] text-ink-faint">
                    static: {member.instanceId}
                  </span>
                ) : null}
              </TableCell>
              <TableCell className="font-mono">{member.clientId}</TableCell>
              <TableCell className="font-mono text-ink-muted">
                {member.clientHost}
              </TableCell>
              <TableCell className="text-right font-mono">
                {member.memberEpoch ?? "—"}
              </TableCell>
              <TableCell>
                {member.assignment.length === 0 ? (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span className="text-[12px] text-ink-faint">
                        not reported
                      </span>
                    </TooltipTrigger>
                    <TooltipContent>
                      the classic protocol carries an assignor-defined blob that
                      kaas-ui does not guess at
                    </TooltipContent>
                  </Tooltip>
                ) : (
                  <div className="flex flex-col gap-0.5">
                    {member.assignment.map((assignment) => (
                      <span
                        key={assignment.topic}
                        className="font-mono text-[12px]"
                      >
                        {assignment.topic}{" "}
                        <span className="text-ink-faint">
                          [{assignment.partitions.join(", ")}]
                        </span>
                      </span>
                    ))}
                  </div>
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
