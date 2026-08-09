// The topic table.
//
// A paged, sorted, filtered list over one cluster's topics, and nothing else:
// the topic *itself* — partitions, configs, messages — is `topic-detail.tsx`,
// and the two share a route prefix and no code. Two requests fill this one
// table, which is the only thing here that is not a plain list.

import { Link } from "@tanstack/react-router"
import { useMemo, useState } from "react"

import { useTopicMetrics, useTopics } from "@/api/client"
import type { TopicSummary } from "@/api/types"
import {
  Empty,
  ErrorChips,
  SnapshotAge,
  Spinner,
  bytes,
  count,
} from "@/components/domain"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { PageTitle } from "@/components/page-title"

const PAGE = 50

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

export function Topics({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  const [search, setSearch] = useState("")
  const [internal, setInternal] = useState(false)
  const [replication, setReplication] = useState(false)
  const [sort, setSort] = useState("name")
  const [order, setOrder] = useState<"asc" | "desc">("asc")
  const [offset, setOffset] = useState(0)

  const query = { search, internal, sort, order, limit: PAGE, offset }

  // Two requests for one table. The first is snapshot-only and lands at once;
  // the second costs a `DescribeLogDirs` per broker and a `ListOffsets` per
  // leader, and fills the last two columns when it arrives.
  const topics = useTopics(envId, clusterId, query)
  const metrics = useTopicMetrics(envId, clusterId, query)

  const total = topics.data?.total ?? 0
  const items = topics.data?.items ?? []

  // Keyed by name rather than by index: the two responses are separate reads
  // of a moving cluster, and a topic created between them would shift every
  // row below it onto the wrong numbers.
  const enriched = useMemo(() => {
    const map = new Map<string, TopicSummary>()
    for (const topic of metrics.data?.items ?? []) map.set(topic.name, topic)
    return map
  }, [metrics.data])

  const sortBy = (column: string) => {
    if (sort === column) {
      setOrder(order === "asc" ? "desc" : "asc")
    } else {
      setSort(column)
      setOrder("asc")
    }
    setOffset(0)
  }

  const heading = (label: string, column: string, right?: boolean) => (
    <TableHead className={right ? "text-right" : undefined}>
      <button
        type="button"
        onClick={() => sortBy(column)}
        className="hover:underline"
      >
        {label}
        {sort === column ? (order === "asc" ? " ↑" : " ↓") : ""}
      </button>
    </TableHead>
  )

  return (
    <>
      <PageTitle
        title="Topics"
        subtitle={`${count(total)} matching`}
        actions={
          <SnapshotAge
            ageMs={topics.data?.snapshotAgeMs ?? null}
            asOfMs={topics.dataUpdatedAt}
          />
        }
      />

      <div className="mb-4 flex flex-wrap items-center gap-4">
        <Input
          value={search}
          onChange={(event) => {
            setSearch(event.target.value)
            setOffset(0)
          }}
          placeholder="filter by name"
          className="h-8 max-w-xs"
        />
        <Label className="text-[12px] font-normal text-ink-muted">
          <input
            type="checkbox"
            checked={internal}
            onChange={(event) => {
              setInternal(event.target.checked)
              setOffset(0)
            }}
          />
          internal topics
        </Label>
        <Label className="text-[12px] font-normal text-ink-muted">
          <input
            type="checkbox"
            checked={replication}
            onChange={(event) => {
              setReplication(event.target.checked)
              // Leaving a sort pointed at a column that is no longer on screen
              // reorders the table for a reason the reader cannot see.
              if (!event.target.checked && sort === "underReplicated") {
                setSort("name")
                setOrder("asc")
                setOffset(0)
              }
            }}
          />
          replication
        </Label>
      </div>

      <ErrorChips
        errors={[
          ...(topics.data?.errors ?? []),
          ...(metrics.data?.errors ?? []),
        ]}
      />

      {topics.isLoading ? (
        <Spinner />
      ) : items.length === 0 ? (
        <Empty>no topics match</Empty>
      ) : (
        <>
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  {heading("name", "name")}
                  {heading("partitions", "partitions", true)}
                  {replication
                    ? heading("out of sync", "underReplicated", true)
                    : null}
                  {replication ? (
                    <TableHead className="text-right">rf</TableHead>
                  ) : null}
                  {heading("messages", "messages", true)}
                  {heading("size", "size", true)}
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
                          pending={metrics.isFetching}
                        />
                      </TableCell>
                      <TableCell className="text-right font-mono">
                        <Metric
                          value={row.replicatedBytes}
                          render={bytes}
                          pending={metrics.isFetching}
                        />
                      </TableCell>
                    </TableRow>
                  )
                })}
              </TableBody>
            </Table>
          </div>

          {total > PAGE ? (
            <div className="mt-3 flex items-center gap-3 text-[12px]">
              <Button
                variant="outline"
                size="sm"
                disabled={offset === 0}
                onClick={() => setOffset(Math.max(0, offset - PAGE))}
              >
                previous
              </Button>
              <span className="text-ink-muted">
                {offset + 1}–{Math.min(offset + PAGE, total)} of {count(total)}
              </span>
              <Button
                variant="outline"
                size="sm"
                disabled={offset + PAGE >= total}
                onClick={() => setOffset(offset + PAGE)}
              >
                next
              </Button>
            </div>
          ) : null}
        </>
      )}
    </>
  )
}
