import type { ClientQuota } from "@/api/types"
import { HintHead } from "@/components/domain"
import { bytes } from "@/lib/format"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

/**
 * The configured client quotas.
 *
 * A quota is addressed by a *set* of components rather than by a name, so the
 * entity column is a list: `user=alice` and `user=alice, client-id=app` are two
 * quotas and the second is the more specific match.
 *
 * `<default>` is a value and not a blank. A null user beside a set client-id
 * means "that client, for every user who has no quota of their own" — the
 * default-quota semantics — and rendering it as an empty cell loses the entire
 * meaning of the row.
 */
export function QuotaTable({ quotas }: { quotas: ClientQuota[] }) {
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <HintHead
              label="entity"
              hint="who the limit applies to. More components is a more specific quota, and the most specific match wins"
            />
            <HintHead
              label="limits"
              hint="what the broker throttles on. Byte rates are per second and per broker, not per cluster"
            />
          </TableRow>
        </TableHeader>
        <TableBody>
          {quotas.map((quota) => (
            <TableRow key={entityKey(quota)}>
              <TableCell>
                <span className="flex flex-wrap gap-1.5">
                  {quota.entity.map((component) => (
                    <span
                      key={component.entityType}
                      className="font-mono text-[13px]"
                    >
                      <span className="text-ink-muted">
                        {component.entityType}=
                      </span>
                      {component.name ?? (
                        <span className="text-ink-faint">&lt;default&gt;</span>
                      )}
                    </span>
                  ))}
                </span>
              </TableCell>
              <TableCell>
                <dl className="flex flex-wrap gap-x-6 gap-y-1 text-[13px]">
                  {quota.values.map((value) => (
                    <span key={value.key} className="flex gap-1.5">
                      <dt className="text-ink-muted">{value.key}</dt>
                      <dd className="font-mono">
                        {limit(value.key, value.value)}
                      </dd>
                    </span>
                  ))}
                </dl>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

/** `user=alice, client-id=app` — the identity the reader sees, as one string. */
function entityKey(quota: ClientQuota): string {
  return quota.entity
    .map(
      (component) => `${component.entityType}=${component.name ?? "<default>"}`
    )
    .join(", ")
}

/**
 * A quota value in the unit its key implies.
 *
 * The unit is not on the wire and cannot be: `producer_byte_rate` is bytes per
 * second and `request_percentage` is a percentage, and the broker sends both as
 * a bare double. Reading the key is the only way to say which, and rendering
 * 1048576 without saying "per second" is a number nobody can act on.
 */
function limit(key: string, value: number): string {
  if (key.endsWith("_byte_rate")) return `${bytes(value)}/s`
  if (key.endsWith("_percentage")) return `${value}%`
  return String(value)
}
