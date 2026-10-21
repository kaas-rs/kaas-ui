import { useMemo } from "react"
import { KeyRound, Link2 } from "lucide-react"

import type { RedactedConfig } from "@/api/types"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

/**
 * A connector config, as the server redacted it.
 *
 * Grouped by the plugin definition's own `group` where it declares one, which
 * is how the plugin's author thought about the keys — "Connection" before
 * "Error handling" — and ungrouped keys last. A withheld value says it was
 * withheld rather than rendering as an empty cell, and a config-provider
 * reference is marked as the pointer it is.
 */
export function ConfigTable({ config }: { config: RedactedConfig }) {
  const groups = useMemo(() => {
    const byGroup = new Map<string, RedactedConfig["entries"]>()
    for (const entry of config.entries) {
      const group = entry.group ?? ""
      const list = byGroup.get(group) ?? []
      list.push(entry)
      byGroup.set(group, list)
    }
    return [...byGroup.entries()].sort(([a], [b]) =>
      a === "" ? 1 : b === "" ? -1 : a.localeCompare(b)
    )
  }, [config])

  return (
    <div className="space-y-3">
      <p className="text-[12px] text-ink-faint">
        {config.redaction === "declared" ? (
          <>
            Secrets are withheld by the server: every key the plugin declares a
            password, prefixed or not, and every key whose name says it is one.
          </>
        ) : (
          <span className="text-warn-ink">
            The plugin's definition could not be read, so secrets are withheld
            by name alone — a secret with an ordinary name would be shown.
          </span>
        )}
      </p>
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[40%]">key</TableHead>
              <TableHead>value</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {groups.map(([group, entries]) => (
              <GroupRows key={group || "-"} group={group} entries={entries} />
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}

function GroupRows({
  group,
  entries,
}: {
  group: string
  entries: RedactedConfig["entries"]
}) {
  return (
    <>
      {group ? (
        <TableRow className="hover:bg-transparent">
          <TableCell
            colSpan={2}
            className="bg-muted/40 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-ink-muted"
          >
            {group}
          </TableCell>
        </TableRow>
      ) : null}
      {entries.map((entry) => (
        <TableRow key={entry.key}>
          <TableCell
            className="align-top font-mono text-[12px]"
            title={entry.documentation ?? undefined}
          >
            {entry.key}
          </TableCell>
          <TableCell className="whitespace-pre-wrap break-all font-mono text-[12px] text-ink-muted">
            {entry.redacted ? (
              <span className="inline-flex items-center gap-1.5 text-ink-faint">
                <KeyRound aria-hidden className="size-3.5" />
                withheld
              </span>
            ) : entry.provider ? (
              <span
                className="inline-flex items-center gap-1.5"
                title="a config-provider reference: the worker resolves it, kaas-ui never sees the value"
              >
                <Link2 aria-hidden className="size-3.5 text-ink-faint" />
                {entry.value}
              </span>
            ) : (
              entry.value
            )}
          </TableCell>
        </TableRow>
      ))}
    </>
  )
}
