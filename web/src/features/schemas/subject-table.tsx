import { Link } from "@tanstack/react-router"

import type { SubjectRow } from "@/api/types"
import { HintHead } from "@/components/domain"
import { Badge } from "@/components/ui/badge"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

import { Compatibility } from "./compatibility"
import { Pending } from "./pending"
import { SortableHead } from "./sortable-head"

export function SubjectTable({
  envId,
  registryId,
  rows,
  described,
  fetching,
  order,
  onSort,
}: {
  envId: string
  registryId: string
  rows: SubjectRow[]
  described: Map<string, SubjectRow>
  fetching: boolean
  order: "asc" | "desc"
  onSort: () => void
}) {
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <SortableHead
              label={`subject${order === "asc" ? " ↑" : " ↓"}`}
              hint="What the schema is registered against — usually a topic plus -value."
              onClick={onSort}
            />
            <HintHead
              label="id"
              hint="The number the wire format carries. Registry-wide, not per subject."
              right
            />
            <HintHead label="type" hint="Avro, Protobuf or JSON Schema." />
            <HintHead
              label="version"
              hint="The newest version of this subject."
              right
            />
            <HintHead
              label="compatibility"
              hint="What the registry will accept as the next version."
            />
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => {
            const full = described.get(row.subject) ?? row
            return (
              <TableRow key={row.subject}>
                <TableCell>
                  <Link
                    to="/environments/$envId/schema-registries/$registryId/subjects/$subject"
                    params={{ envId, registryId, subject: row.subject }}
                    className="font-mono hover:underline"
                    style={{ color: "var(--rust-ink)" }}
                  >
                    {row.subject}
                  </Link>
                </TableCell>
                <TableCell className="text-right font-mono">
                  <Pending value={full.id} fetching={fetching}>
                    {(id) => `#${id}`}
                  </Pending>
                </TableCell>
                <TableCell>
                  <Pending value={full.format} fetching={fetching}>
                    {(format) => <Badge variant="outline">{format}</Badge>}
                  </Pending>
                </TableCell>
                <TableCell className="text-right font-mono">
                  <Pending value={full.version} fetching={fetching}>
                    {(version) => String(version)}
                  </Pending>
                </TableCell>
                <TableCell>
                  <Pending value={full.compatibility} fetching={fetching}>
                    {(mode) => (
                      <Compatibility
                        mode={mode}
                        inherited={full.compatibilityInherited}
                      />
                    )}
                  </Pending>
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}
