import { Link } from "@tanstack/react-router"

import type { Reassignment } from "@/api/types"
import { HintHead } from "@/components/domain"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

/**
 * What is moving right now.
 *
 * Adding and removing are separate columns rather than a diff against the
 * replica set, because the replica set holds *both* until the move completes:
 * a reader taking the difference themselves would be doing arithmetic the
 * broker already did, and getting it wrong on the partition that matters.
 *
 * The placement grid on the topic page shows where replicas are; this shows
 * where they are going. The topic link is what joins the two.
 */
export function ReassignmentTable({
  envId,
  clusterId,
  moves,
}: {
  envId: string
  clusterId: string
  moves: Reassignment[]
}) {
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <HintHead
              label="topic"
              hint="the topic a partition of which is moving"
            />
            <HintHead
              label="partition"
              hint="its index within the topic"
              right
            />
            <HintHead
              label="replicas"
              hint="the current set — it holds the arriving and the departing at once until the move finishes"
            />
            <HintHead
              label="adding"
              hint="brokers catching up a copy. They are not in the in-sync set until they have caught up"
            />
            <HintHead
              label="removing"
              hint="brokers whose copy is dropped when the move completes"
            />
          </TableRow>
        </TableHeader>
        <TableBody>
          {moves.map((move) => (
            <TableRow key={`${move.topic}-${move.partition}`}>
              <TableCell>
                <Link
                  to="/environments/$envId/clusters/$clusterId/topics/$topic"
                  params={{ envId, clusterId, topic: move.topic }}
                  className="font-mono hover:underline"
                  style={{ color: "var(--rust-ink)" }}
                >
                  {move.topic}
                </Link>
              </TableCell>
              <TableCell className="text-right font-mono">
                {move.partition}
              </TableCell>
              <TableCell className="font-mono text-ink-muted">
                {move.replicas.join(", ")}
              </TableCell>
              <TableCell className="font-mono text-ok">
                {move.adding.length ? `+ ${move.adding.join(", ")}` : "—"}
              </TableCell>
              <TableCell className="font-mono text-warn-ink">
                {move.removing.length ? `− ${move.removing.join(", ")}` : "—"}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
