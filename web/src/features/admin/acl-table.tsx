import type { Acl } from "@/api/types"
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
 * The bindings an authorizer holds.
 *
 * `deny` is a badge and not a red row: denies are rare, they win over allows in
 * Kafka's evaluation order, and a reader scanning for the one that is blocking
 * something needs to find it rather than to be alarmed by it.
 *
 * `prefixed` is marked for the same reason in reverse — it looks like a literal
 * name until you notice, and the difference is one topic against a namespace.
 */
export function AclTable({ acls }: { acls: Acl[] }) {
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <HintHead
              label="principal"
              hint="who the binding is about, as the authorizer spells it — the `User:` prefix is part of the name"
            />
            <HintHead
              label="permission"
              hint="allow or deny. A deny beats every allow on the same resource, whichever order they were written in"
            />
            <HintHead
              label="operation"
              hint="what is permitted. `all` is every operation; a number in brackets is one this build has no name for"
            />
            <HintHead
              label="resource"
              hint="what it applies to — `literal` is that exact name, `prefixed` is every name starting with it, and `*` is all of them"
            />
            <HintHead
              label="host"
              hint="the client address it applies from; `*` is anywhere, and an empty cell is a binding stored without one"
            />
          </TableRow>
        </TableHeader>
        <TableBody>
          {acls.map((acl, index) => (
            <TableRow
              key={`${acl.principal}-${acl.resourceType}-${acl.resourceName}-${acl.operation}-${index}`}
            >
              <TableCell className="font-mono text-[13px]">
                {acl.principal}
              </TableCell>
              <TableCell>
                {acl.permission === "deny" ? (
                  <Badge className="bg-danger-soft text-danger">deny</Badge>
                ) : (
                  <span className="text-ink-muted">allow</span>
                )}
              </TableCell>
              <TableCell className="font-mono text-[13px]">
                {acl.operation}
              </TableCell>
              <TableCell>
                <span className="text-ink-muted">{acl.resourceType}:</span>
                <span className="font-mono">{acl.resourceName}</span>
                {acl.patternType === "prefixed" ? (
                  <Badge variant="outline" className="ml-2 text-[11px]">
                    prefixed
                  </Badge>
                ) : null}
              </TableCell>
              <TableCell className="font-mono text-ink-muted">
                {acl.host || <span className="text-ink-faint">—</span>}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
