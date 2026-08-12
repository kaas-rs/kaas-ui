import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import type { ApiKeyEntry } from "@/api/types"

/** The version table, one row per advertised api key. */
export function ApiKeysTable({ keys }: { keys: ApiKeyEntry[] }) {
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="text-right">key</TableHead>
            <TableHead>name</TableHead>
            <TableHead>broker</TableHead>
            <TableHead>kaas-ui</TableHead>
            <TableHead className="text-right">negotiated</TableHead>
            <TableHead>note</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {keys.map((key) => (
            <TableRow key={key.key}>
              <TableCell className="text-right font-mono">{key.key}</TableCell>
              <TableCell className="font-mono">{key.name}</TableCell>
              <TableCell className="font-mono text-ink-muted">
                {key.broker ? `v${key.broker[0]}–v${key.broker[1]}` : "—"}
              </TableCell>
              <TableCell className="font-mono text-ink-muted">
                {key.ours ? `v${key.ours[0]}–v${key.ours[1]}` : "—"}
              </TableCell>
              <TableCell className="text-right font-mono">
                {key.negotiated === null ? "—" : `v${key.negotiated}`}
              </TableCell>
              <TableCell>
                {key.ours === null ? (
                  <span className="text-[12px] text-warn-ink">
                    no schema in this build
                  </span>
                ) : key.brokerAhead ? (
                  <span className="text-[12px] text-ink-muted">
                    broker is ahead
                  </span>
                ) : null}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
