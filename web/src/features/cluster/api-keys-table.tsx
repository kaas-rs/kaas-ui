import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { HintHead } from "@/components/domain"
import type { ApiKeyEntry } from "@/api/types"

/** The version table, one row per advertised api key. */
export function ApiKeysTable({ keys }: { keys: ApiKeyEntry[] }) {
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <HintHead
              label="key"
              hint="the protocol's number for this request — stable across releases"
              right
            />
            <HintHead
              label="name"
              hint="what this build calls the key; blank where it has no name for it"
            />
            <HintHead
              label="broker"
              hint="the version range this broker advertises for the key"
            />
            <HintHead
              label="kaas-ui"
              hint="the range kaas-lib can speak, from the schema compiled in"
            />
            <HintHead
              label="negotiated"
              hint="the highest version both ends have — what a request actually uses"
              right
            />
            <HintHead
              label="note"
              hint="where the two ranges do not meet, and which side is ahead"
            />
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
