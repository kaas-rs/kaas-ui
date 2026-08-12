// One topic: its partitions, its configs, and the message browser in a tab.
//
// Split from `topics.tsx`, which is the paged table of topic *names*. They
// share a route prefix and nothing else — that page is a sorted, filtered list
// over the whole cluster, this one is a single describe fanned out across
// three tabs, and keeping both in one file meant the partition grid and the
// placement legend sat below a paging control they have no relationship to.
//
// Read-only: no creating a partition, no editing a config, no producing a
// record. That is most of what kafbat's equivalent page spends its buttons on.

import { Link, useNavigate, useSearch } from "@tanstack/react-router"
import { useCallback } from "react"
import { ArrowLeft } from "lucide-react"

import { useClusters, useTopic } from "@/api/client"
import { MessageBrowser } from "@/features/messages/browser"
import type { TopicSearch, TopicTab } from "@/features/messages/search"
import { TopicStatistics } from "@/features/statistics"
import { ErrorChips, Mono, Spinner } from "@/components/domain"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { PageTitle } from "@/components/page-title"
import { TopicFacts } from "@/features/topics/topic-facts"
import { TopicSchemas } from "@/features/topics/topic-schemas"
import { PartitionTable } from "@/features/topics/partition-table"
import { TopicConfigs } from "@/features/topics/topic-configs"

export function TopicDetailPage({
  envId,
  clusterId,
  topic,
}: {
  envId: string
  clusterId: string
  topic: string
}) {
  const detail = useTopic(envId, clusterId, topic)
  const search = useSearch({
    from: "/environments/$envId/clusters/$clusterId/topics/$topic",
  })
  // What this caller may do here, from the cluster's own card. A messages tab
  // that 403s on click is worse than no messages tab — the same reasoning the
  // sidebar applies to a capability the *broker* does not have. Until the
  // answer arrives, show it: a tab that appears under the cursor is worse than
  // one that errors once, and an open deployment always grants both.
  const clusters = useClusters(envId)
  const grants = clusters.data?.items.find(
    (card) => card.id === clusterId
  )?.grants
  const mayReadMessages =
    grants === undefined || !!grants.topic?.includes("messages_read")
  const navigate = useNavigate()

  /**
   * Every write to this page's URL, including the message browser's.
   *
   * Replaces by default: seeking, filtering and selecting are one continuous
   * act of looking at a topic, and a back button that walks a reader out
   * through forty row selections is not a back button. Changing tab is the
   * exception — that is a place someone can want to come back to.
   */
  const setSearch = useCallback(
    (next: Partial<TopicSearch>, replace = true) => {
      void navigate({
        to: "/environments/$envId/clusters/$clusterId/topics/$topic",
        params: { envId, clusterId, topic },
        search: (previous) => ({ ...previous, ...next }),
        replace,
      })
    },
    [navigate, clusterId, topic]
  )

  if (detail.isLoading) return <Spinner label={`describing ${topic}`} />

  const info = detail.data?.items[0]
  const errors = detail.data?.errors ?? []

  if (!info) {
    return (
      <>
        <PageTitle title={topic} />
        <ErrorChips errors={errors} />
        <Card className="p-5 text-[13px] text-ink-muted">
          {errors[0]?.message ?? "the cluster did not describe this topic"}
        </Card>
      </>
    )
  }

  return (
    <>
      <PageTitle
        title={<span className="font-mono">{info.name}</span>}
        subtitle={
          <span className="flex flex-wrap items-center gap-3">
            <span>{info.partitions.length} partitions</span>
            {info.internal ? (
              <span className="text-warn-ink">internal</span>
            ) : null}
            {info.topicId ? <Mono>{info.topicId}</Mono> : null}
          </span>
        }
        actions={
          <Button variant="ghost" size="sm" asChild>
            <Link
              to="/environments/$envId/clusters/$clusterId/topics"
              params={{ envId, clusterId }}
            >
              <ArrowLeft aria-hidden />
              all topics
            </Link>
          </Button>
        }
      />

      <ErrorChips errors={errors} />

      {/* Controlled by the URL, not by local state: `?tab=messages` alongside
          the seek parameters is what makes a link to a filtered, seeked view
          open on that view rather than on the partition table. */}
      <Tabs
        value={search.tab}
        onValueChange={(tab) => setSearch({ tab: tab as TopicTab }, false)}
      >
        <TabsList>
          <TabsTrigger value="overview">overview</TabsTrigger>
          <TabsTrigger value="configs">configs</TabsTrigger>
          {mayReadMessages ? (
            <TabsTrigger value="messages">messages</TabsTrigger>
          ) : null}
          {/* Gated like messages: an analysis reads every payload, so it
              spends the same grant. */}
          {mayReadMessages ? (
            <TabsTrigger value="statistics">statistics</TabsTrigger>
          ) : null}
        </TabsList>

        <TabsContent value="overview" className="mt-4 space-y-6">
          <TopicFacts
            envId={envId}
            clusterId={clusterId}
            topic={topic}
            info={info}
          />
          <TopicSchemas
            envId={envId}
            registryId={
              clusters.data?.items.find((card) => card.id === clusterId)
                ?.schemaRegistry ?? null
            }
            topic={topic}
          />
          <PartitionTable
            partitions={info.partitions}
            brokerIds={info.brokerIds}
            envId={envId}
            clusterId={clusterId}
            topic={topic}
          />
        </TabsContent>
        <TabsContent value="configs" className="mt-4">
          <TopicConfigs envId={envId} clusterId={clusterId} topic={topic} />
        </TabsContent>
        {/* Radix unmounts the hidden panel, and the statistics component
            closes its stream on unmount — so leaving this tab cancels a
            running analysis, which is the whole cancellation story. */}
        <TabsContent value="statistics" className="mt-4">
          <TopicStatistics
            envId={envId}
            clusterId={clusterId}
            topic={topic}
            info={info}
          />
        </TabsContent>
        {/* The panel is given a height rather than left to grow: the list is
            virtualized and the split pane is a flex box, and neither can work
            inside a page that scrolls. The subtraction is this page's chrome —
            app header, padding, title, tab row, footer. Leaving the tab stops
            the stream, because Radix unmounts the panel that is not shown and
            a live scan nobody is looking at is a scan that should not be open. */}
        <TabsContent value="messages" className="mt-4">
          <MessageBrowser
            envId={envId}
            clusterId={clusterId}
            topic={topic}
            search={search}
            onSearch={setSearch}
            className="h-[calc(100vh-17rem)] min-h-[26rem]"
          />
        </TabsContent>
      </Tabs>
    </>
  )
}
