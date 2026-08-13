import type { ScramUser } from "@/api/types"
import { HintHead } from "@/components/domain"
import { Badge } from "@/components/ui/badge"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

/**
 * Who can authenticate with SCRAM.
 *
 * **Not how.** The broker stores a salt and a salted hash and has no api that
 * returns either, so there is no field this table declines to show — the
 * credential is not on the wire at all. Said out loud on the screen because
 * "SCRAM users" reads like a place credentials might be, and a reader should
 * not have to take that on trust.
 */
export function ScramTable({ users }: { users: ScramUser[] }) {
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <HintHead
              label="user"
              hint="the SCRAM user name — the principal an ACL names as `User:<this>`"
            />
            <HintHead
              label="mechanisms"
              hint="what this user can authenticate with. Two entries is two separately stored credentials, not one used both ways"
            />
            <HintHead
              label="iterations"
              hint="the hashing rounds the credential was stored with. Kafka's floor is 4096; higher is slower to verify and slower to crack"
              right
            />
          </TableRow>
        </TableHeader>
        <TableBody>
          {users.map((user) => (
            <TableRow key={user.user}>
              <TableCell className="font-mono">{user.user}</TableCell>
              <TableCell>
                <span className="flex flex-wrap gap-1.5">
                  {user.credentials.map((credential) => (
                    <Badge
                      key={credential.mechanism}
                      variant="outline"
                      className="font-mono text-[11px]"
                    >
                      {credential.mechanism}
                    </Badge>
                  ))}
                </span>
              </TableCell>
              <TableCell className="text-right font-mono">
                {user.credentials
                  .map((credential) => credential.iterations)
                  .join(" · ")}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
