import { Link } from "@tanstack/react-router"

import type { TopicSchemas, TopicSummary } from "@/api/types"
import { HintHead, RESOURCE_KINDS, SortableHead } from "@/components/domain"
import { bytes, count } from "@/lib/format"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

/** From the one table, so this column cannot disagree with the sidebar. */
const SchemaIcon = RESOURCE_KINDS.schema_registry.icon

/**
 * A metric cell in one of its three states.
 *
 * `—` and blank are different answers and must not look alike: blank means the
 * fan-out is still out, `—` means it came back and this topic has no number —
 * a partition that would not answer, or a broker with no `DescribeLogDirs`.
 * A dash that silently means "still loading" is how a cluster looks broken for
 * as long as it is slow.
 */
function Metric({
  value,
  render,
  pending,
}: {
  value: number | null
  render: (value: number) => string
  pending: boolean
}) {
  if (value !== null) return <>{render(value)}</>
  return (
    <span
      className="text-ink-faint"
      title={pending ? "still asking" : undefined}
    >
      {pending ? "·" : "—"}
    </span>
  )
}

/**
 * The subjects naming one topic, in the three states this cell also has.
 *
 * A link per side, because "there is a schema" and "here it is" are one click
 * apart and the second is what anyone reading the column wants next. Both sides
 * appear when both exist: a key schema and a value schema are two subjects.
 *
 * The registry's own glyph rather than the words `value` and `key`. Fifty rows
 * of two-letter badges is a column of text to read where the question is one a
 * mark answers at a glance, and the mark is the one the sidebar and the fleet
 * already use for a registry. Nothing pops up on hover to say which side each
 * is: it is in the subject the link goes to, which is where a reader who wants
 * to know is headed anyway.
 *
 * `—` is an answer — the registry holds nothing for this topic — and `·` is the
 * absence of one, which is the same distinction `Metric` draws and for the same
 * reason: a dash that quietly means "still asking" is how a registry looks
 * empty for as long as it is slow.
 */
function SchemaCell({
  envId,
  schemas,
  pending,
}: {
  envId: string
  schemas: TopicSchemas | undefined
  pending: boolean
}) {
  if (!schemas) {
    return (
      <span
        className="text-ink-faint"
        title={pending ? "still asking" : undefined}
      >
        {pending ? "·" : "—"}
      </span>
    )
  }

  const sides: [string, string][] = []
  if (schemas.value) sides.push(["value", schemas.value])
  if (schemas.key) sides.push(["key", schemas.key])
  if (!sides.length) return <span className="text-ink-faint">—</span>

  return (
    <span className="flex items-center gap-2">
      {sides.map(([side, subject]) => (
        <Link
          key={side}
          to="/environments/$envId/schema-registries/$registryId/subjects/$subject"
          params={{ envId, registryId: schemas.registry, subject }}
          // Named for a screen reader and not for a pointer: a link whose
          // whole content is an `aria-hidden` glyph has no accessible name
          // without this, and it raises nothing on hover.
          aria-label={`${side} schema of this topic: ${subject}`}
          // Muted, not link ink: fifty rows of an accent-coloured glyph is a
          // column that pulls the eye harder than the topic names beside it,
          // and the mark is a fact about the row rather than the thing the row
          // is for. It darkens on hover, which is where it says it is a link.
          className="text-ink-muted hover:text-ink"
        >
          <SchemaIcon aria-hidden className="size-4" />
        </Link>
      ))}
    </span>
  )
}

export function TopicTable({
  envId,
  clusterId,
  items,
  replication,
  enriched,
  registryId,
  subjects,
  schemasPending,
  metricsPending,
  sort,
  order,
  onSort,
}: {
  envId: string
  clusterId: string
  items: TopicSummary[]
  replication: boolean
  enriched: Map<string, TopicSummary>
  /** The registry this cluster reads, and whether the column exists at all. */
  registryId: string | null
  subjects: Map<string, TopicSchemas>
  schemasPending: boolean
  metricsPending: boolean
  sort: string
  order: "asc" | "desc"
  onSort: (column: string) => void
}) {
  // The arrow rides in the label so the sorted column says so inside the same
  // control it is set from, rather than beside it.
  const heading = (
    label: string,
    column: string,
    hint: string,
    right?: boolean
  ) => (
    <SortableHead
      label={`${label}${sort === column ? (order === "asc" ? " ↑" : " ↓") : ""}`}
      hint={hint}
      right={right}
      onClick={() => onSort(column)}
    />
  )

  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            {heading("name", "name", "sorted as bytes, so `A` before `a`")}
            {/* Not sortable: the column is a join the server does over the
                page, and ordering by it would mean reading the registry for
                every topic on the cluster before the first row could be
                placed. */}
            {registryId ? (
              <HintHead
                label="schema"
                hint={`subjects in ${registryId} naming this topic — one link per side`}
              />
            ) : null}
            {heading(
              "partitions",
              "partitions",
              "how many the topic has; a red ✕ counts those with no leader",
              true
            )}
            {replication
              ? heading(
                  "out of sync",
                  "underReplicated",
                  "partitions whose ISR is short of their replica count",
                  true
                )
              : null}
            {replication ? (
              <HintHead
                label="rf"
                hint="replication factor — the smallest replica count across partitions"
                right
              />
            ) : null}
            {heading(
              "messages",
              "messages",
              "latest − earliest summed: what is retained, not what was ever written",
              true
            )}
            {heading(
              "size",
              "size",
              "bytes on disk across every replica, not one copy",
              true
            )}
          </TableRow>
        </TableHeader>
        <TableBody>
          {items.map((topic) => {
            // The base row already carries the numbers when the sort is
            // a metric, because the server had to compute them to order
            // by them. Otherwise they arrive on the second request.
            const row = enriched.get(topic.name) ?? topic
            return (
              <TableRow key={topic.name}>
                <TableCell>
                  <Link
                    to="/environments/$envId/clusters/$clusterId/topics/$topic"
                    params={{ envId, clusterId, topic: topic.name }}
                    className="font-mono hover:underline"
                    style={{ color: "var(--rust-ink)" }}
                  >
                    {topic.name}
                  </Link>
                  {topic.internal ? (
                    <span className="ml-2 text-[11px] text-ink-faint">
                      internal
                    </span>
                  ) : null}
                </TableCell>
                {registryId ? (
                  <TableCell>
                    <SchemaCell
                      envId={envId}
                      schemas={subjects.get(topic.name)}
                      pending={schemasPending}
                    />
                  </TableCell>
                ) : null}
                {/* Offline partitions ride in this cell rather than in a
                    column of their own: on a healthy cluster that column
                    is a stripe of zeroes, and the one row that matters is
                    easier to see against plain numbers than against them. */}
                <TableCell className="text-right font-mono whitespace-nowrap">
                  {topic.partitionCount}
                  {topic.offlinePartitionCount > 0 ? (
                    <span
                      className="text-danger ml-1.5 font-medium"
                      title={`${topic.offlinePartitionCount} partition(s) with no leader or an offline replica`}
                    >
                      ✕{topic.offlinePartitionCount}
                    </span>
                  ) : null}
                </TableCell>
                {replication ? (
                  <TableCell className="text-right">
                    {topic.underReplicatedPartitionCount > 0 ? (
                      <span className="font-mono font-medium text-warn-ink">
                        △ {topic.underReplicatedPartitionCount}
                      </span>
                    ) : (
                      <span className="text-ink-faint">0</span>
                    )}
                  </TableCell>
                ) : null}
                {replication ? (
                  <TableCell className="text-right font-mono">
                    {topic.replicationFactor}
                  </TableCell>
                ) : null}
                <TableCell className="text-right font-mono">
                  <Metric
                    value={row.messageCount}
                    render={count}
                    pending={metricsPending}
                  />
                </TableCell>
                <TableCell className="text-right font-mono">
                  <Metric
                    value={row.replicatedBytes}
                    render={bytes}
                    pending={metricsPending}
                  />
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}
