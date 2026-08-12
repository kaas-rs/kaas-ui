// The topic table.
//
// A paged, sorted, filtered list over one cluster's topics, and nothing else:
// the topic *itself* — partitions, configs, messages — is `topic-detail.tsx`,
// and the two share a route prefix and no code. Two requests fill this one
// table, which is the only thing here that is not a plain list.

import { useMemo, useState } from "react"

import { useTopicMetrics, useTopics } from "@/api/client"
import type { TopicSummary } from "@/api/types"
import { Empty, ErrorChips, SnapshotAge, Spinner } from "@/components/domain"
import { count } from "@/lib/format"
import { Button } from "@/components/ui/button"
import { PageTitle } from "@/components/page-title"
import { TopicListControls } from "@/features/topics/topic-list-controls"
import { TopicTable } from "@/features/topics/topic-table"

const PAGE = 50

export function TopicsPage({
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

      <TopicListControls
        search={search}
        internal={internal}
        replication={replication}
        onSearch={(value) => {
          setSearch(value)
          setOffset(0)
        }}
        onInternal={(checked) => {
          setInternal(checked)
          setOffset(0)
        }}
        onReplication={(checked) => {
          setReplication(checked)
          // Leaving a sort pointed at a column that is no longer on screen
          // reorders the table for a reason the reader cannot see.
          if (!checked && sort === "underReplicated") {
            setSort("name")
            setOrder("asc")
            setOffset(0)
          }
        }}
      />

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
          <TopicTable
            envId={envId}
            clusterId={clusterId}
            items={items}
            replication={replication}
            enriched={enriched}
            metricsPending={metrics.isFetching}
            sort={sort}
            order={order}
            onSort={sortBy}
          />

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
