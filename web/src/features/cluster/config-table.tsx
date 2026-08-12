import { Badge } from "@/components/ui/badge"
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
import type { ConfigEntry } from "@/api/types"

/**
 * Config entries as the broker reported them — shared by the cluster's
 * configuration page and the topic's configs tab, so "explicit vs inherited"
 * renders the same way wherever a config is read.
 */
export function ConfigTable({
  entries,
  total,
}: {
  entries: ConfigEntry[]
  total?: number
}) {
  return (
    <>
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>key</TableHead>
              <TableHead>value</TableHead>
              <TableHead>source</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {entries.map((entry) => (
              <TableRow key={entry.name}>
                <TableCell>
                  {entry.documentation ? (
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span className="font-mono">
                          {entry.name}
                          <span className="ml-1.5 text-[11px] text-ink-faint">
                            ⓘ
                          </span>
                        </span>
                      </TooltipTrigger>
                      <TooltipContent className="max-w-lg">
                        <span
                          // The broker's own documentation, which is HTML.
                          dangerouslySetInnerHTML={{
                            __html: entry.documentation.replace(/<[^>]*>/g, ""),
                          }}
                        />
                      </TooltipContent>
                    </Tooltip>
                  ) : (
                    <span className="font-mono">{entry.name}</span>
                  )}
                </TableCell>
                <TableCell className="max-w-[28rem] break-all whitespace-normal">
                  {entry.isSensitive ? (
                    <Badge variant="secondary" className="text-ink-muted">
                      redacted by the broker
                    </Badge>
                  ) : entry.value === null ? (
                    <span className="text-ink-faint">—</span>
                  ) : (
                    <span className="font-mono">{entry.value}</span>
                  )}
                </TableCell>
                <TableCell>
                  <span
                    className={
                      entry.isExplicit
                        ? "font-mono text-[12px] font-medium text-rust-ink"
                        : "font-mono text-[12px] text-ink-faint"
                    }
                    title={
                      entry.isExplicit ? "set explicitly" : "inherited default"
                    }
                  >
                    {entry.source}
                  </span>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
      {total !== undefined && total !== entries.length ? (
        <p className="mt-2 text-[12px] text-ink-faint">
          {entries.length} of {total} entries
        </p>
      ) : null}
    </>
  )
}
