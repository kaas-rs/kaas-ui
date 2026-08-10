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
import { useCallback, useState } from "react"
import { ArrowLeft, ArrowRight } from "lucide-react"

import {
  useClusters,
  useSubjectDetails,
  useTopic,
  useTopicConfigs,
  useTopicSize,
} from "@/api/client"
import type {
  NamingStrategy,
  Partition,
  TopicDetail as TopicDetailData,
} from "@/api/types"
import { MessageBrowser } from "@/features/messages/browser"
import type { TopicSearch, TopicTab } from "@/features/messages/search"
import { TopicStatistics } from "@/features/statistics"
import {
  Empty,
  ErrorChips,
  Mono,
  PlacementLegend,
  Section,
  Stat,
  placementCell,
  Spinner,
  bytes,
  count,
} from "@/components/domain"
import type { ReactNode } from "react"
import { cn } from "@/lib/utils"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Label } from "@/components/ui/label"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { PageTitle } from "@/components/page-title"
import { ConfigTable } from "./cluster"

export function TopicDetail({
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
          <Partitions
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

/**
 * The facts about a topic that are not one of its partitions.
 *
 * Assembled from three sources rather than one, because that is where they
 * live: the describe this page already has, the config fetch the next tab
 * makes, and a `DescribeLogDirs` fan-out that is its own request so the card
 * paints before it lands.
 *
 * Message count, replication factor and the under-replicated count come off
 * `TopicDetail` — the server derives them with the same functions the topic
 * list uses, so the rules that are decisions (minimum across partitions;
 * absent, not smaller) exist exactly once.
 *
 * Replication is three of the eight and it is all one question, so on a topic
 * with one replica per partition all three go. `replication factor: 1` next to
 * `under-replicated: 0` next to `in-sync: 6 of 6` is three ways of saying
 * there is no replication to report on, and it crowds out the five facts that
 * do vary. They come back the moment a topic has a second replica.
 */
function TopicFacts({
  envId,
  clusterId,
  topic,
  info,
}: {
  envId: string
  clusterId: string
  topic: string
  info: TopicDetailData
}) {
  const size = useTopicSize(envId, clusterId, topic)
  const configs = useTopicConfigs(envId, clusterId, topic)

  const partitions = info.partitions
  // The four counters come from the server now — `TopicDetail` carries them,
  // derived by the same functions the topic list uses, so the minimum-across-
  // partitions rule and the absent-not-smaller rule exist exactly once.
  const replicationFactor = info.replicationFactor
  const replicated = replicationFactor > 1
  const underReplicated = info.underReplicatedPartitionCount
  const messages = info.messageCount

  // Sums, not rules: how many in-sync of how many replicas is arithmetic the
  // table below already shows per row.
  const inSync = partitions.reduce(
    (total, partition) => total + partition.isr.length,
    0
  )
  const replicas = partitions.reduce(
    (total, partition) => total + partition.replicas.length,
    0
  )

  const onDisk = size.data?.items[0]?.replicatedBytes ?? null
  // The one-copy figure beside the all-replicas one: they differ by the
  // replication factor, and a topic whose two numbers do not differ by it is
  // itself worth seeing.
  const oneCopy = size.data?.items[0]?.logicalBytes ?? null
  // What kafbat-ui labels a "segment count". `DescribeLogDirs` reports no
  // segment files at all, so the number is one per replica copy per log
  // directory, and it is named for what it counts rather than for what the
  // other UI calls it.
  const entries = size.data?.items[0]?.logDirEntryCount ?? null
  const cleanup =
    configs.data?.items[0]?.entries.find(
      (entry) => entry.name === "cleanup.policy"
    )?.value ?? null

  return (
    <Card>
      <CardContent>
        <ErrorChips errors={size.data?.errors ?? []} />
        <dl className="grid grid-cols-2 gap-x-6 gap-y-3 text-[13px] sm:grid-cols-4">
          <Stat label="partitions" value={count(partitions.length)} />
          {replicated ? (
            <Stat label="replication factor" value={count(replicationFactor)} />
          ) : null}
          {replicated ? (
            <Stat
              label="under-replicated"
              value={count(underReplicated)}
              tone={underReplicated > 0 ? "warn" : undefined}
            />
          ) : null}
          {replicated ? (
            <Stat
              label="in-sync replicas"
              value={count(inSync)}
              note={`of ${count(replicas)}`}
              tone={inSync < replicas ? "warn" : undefined}
            />
          ) : null}
          <Stat label="type" value={info.internal ? "internal" : "external"} />
          <Stat
            label="segment size"
            value={pending(onDisk, bytes, size.isFetching)}
            note={
              onDisk === null
                ? undefined
                : oneCopy === null
                  ? "on disk, all replicas"
                  : `all replicas · ${bytes(oneCopy)} one copy`
            }
          />
          <Stat
            label="log-dir entries"
            value={pending(entries, count, size.isFetching)}
            note={entries === null ? undefined : "one per copy, per directory"}
          />
          <Stat
            label="cleanup policy"
            value={cleanup ?? (configs.isLoading ? "\u00b7" : "\u2014")}
          />
          <Stat
            label="messages"
            value={pending(messages, count, false)}
            note={messages === null ? undefined : "retained"}
          />
        </dl>
      </CardContent>
    </Card>
  )
}

/**
 * The schema this topic's records are written against, where there is one.
 *
 * Absent by default, and that is the common case: most topics carry no schema,
 * most environments reference no registry, and a card that says "no schema"
 * on every topic in the fleet is a card nobody reads. It appears only when a
 * subject genuinely names *this* topic.
 *
 * Which subjects those are comes from the server, per row, and it has to:
 * matching `orders-` here would claim `orders-eu-value` for the topic
 * `orders`, and under `TopicRecordNameStrategy` the seam between topic and
 * record is in the *schema* rather than in the name. The registry search is a
 * substring — it casts wide — and `naming.topic` is what narrows it to an
 * answer. See `SubjectNaming`.
 *
 * Both sides are listed when both exist. A key schema and a value schema are
 * two subjects and two schemas, and picking one to show would be picking the
 * one that happened to sort first.
 */
function TopicSchemas({
  envId,
  registryId,
  topic,
}: {
  envId: string
  registryId: string | null
  topic: string
}) {
  // `details` because the id, the format and the version are the card, and
  // because naming can only be read exactly once the newest schema is in
  // hand. The search is the topic name, so the page described is the handful
  // of subjects that mention it rather than the registry.
  const subjects = useSubjectDetails(envId, registryId ?? "", {
    search: topic,
    limit: SUBJECT_SEARCH,
  })

  if (!registryId) return null

  const rows = (subjects.data?.subjects ?? [])
    .filter((row) => row.naming.topic === topic)
    // `-value` first: it is the one people mean by "the schema of this topic",
    // and a key schema is the exception that should read as an addition.
    .sort((a, b) => rank(a.subject) - rank(b.subject))

  if (!rows.length) return null

  return (
    <Section title={rows.length === 1 ? "Schema" : "Schemas"}>
      <div className="space-y-3">
        {rows.map((row) => (
          <Card key={row.subject}>
            <CardContent>
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-mono text-[13px]">{row.subject}</span>
                    {row.format ? (
                      <Badge variant="outline">{row.format}</Badge>
                    ) : null}
                    <Badge variant="outline" className="text-ink-muted">
                      {side(row.subject, row.naming.strategy)}
                    </Badge>
                  </div>
                  <dl className="mt-3 grid grid-cols-2 gap-x-6 gap-y-3 text-[13px] sm:grid-cols-4">
                    <Stat
                      label="schema id"
                      value={row.id === null ? "—" : `#${row.id}`}
                      note="what the wire format carries"
                    />
                    <Stat
                      label="version"
                      value={row.version === null ? "—" : `v${row.version}`}
                    />
                    <Stat
                      label="compatibility"
                      value={row.compatibility ?? "—"}
                      note={
                        row.compatibilityInherited
                          ? "the registry's default"
                          : undefined
                      }
                    />
                    {row.naming.recordName ? (
                      <Stat label="record" value={row.naming.recordName} />
                    ) : null}
                  </dl>
                </div>

                {/* To the subject, not to the registry listing: the reader is
                    already on the thing the subject is about, and the page
                    they want is the schema text. */}
                <Button variant="outline" size="sm" asChild>
                  <Link
                    to="/environments/$envId/schema-registries/$registryId/subjects/$subject"
                    params={{ envId, registryId, subject: row.subject }}
                  >
                    open schema
                    <ArrowRight aria-hidden />
                  </Link>
                </Button>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>
    </Section>
  )
}

/** How many subjects mentioning the topic to describe. */
const SUBJECT_SEARCH = 50

/** `-value` before `-key` before anything a record strategy named. */
function rank(subject: string): number {
  if (subject.endsWith("-value")) return 0
  if (subject.endsWith("-key")) return 1
  return 2
}

/**
 * Which half of the record this subject decodes.
 *
 * Only `TopicNameStrategy` says: its whole suffix is the answer. Under
 * `{topic}-{record}` the subject names a record and nothing about the name
 * says which side carries it, so the badge says that instead of guessing.
 */
function side(subject: string, strategy: NamingStrategy): string {
  if (strategy !== "topicName") return "record"
  return subject.endsWith("-key") ? "key" : "value"
}

/**
 * A number in one of its three states, as text.
 *
 * The same rule the topic table's `Metric` cell draws, for the same reason:
 * blank means the fan-out is still out, and an em dash means it came back
 * without a number. A dash that quietly means "still loading" is how a cluster
 * looks broken for as long as it is slow.
 */
function pending(
  value: number | null,
  render: (value: number) => string,
  fetching: boolean
): string {
  if (value !== null) return render(value)
  return fetching ? "\u00b7" : "\u2014"
}

/**
 * A partition-table header that says what its column means.
 *
 * Every column here is a Kafka term with a precise meaning and a plausible
 * wrong reading — `records` is what is *retained*, not what was ever written,
 * and `epoch` counts leadership changes rather than anything about data. One
 * line each, on hover, is cheaper than a legend nobody scrolls to.
 */
function Head({
  label,
  hint,
  right,
  className,
}: {
  label: ReactNode
  hint: string
  right?: boolean
  className?: string
}) {
  return (
    <TableHead className={cn(right && "text-right", className)}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="cursor-help decoration-dotted underline-offset-4 hover:underline">
            {label}
          </span>
        </TooltipTrigger>
        <TooltipContent>{hint}</TooltipContent>
      </Tooltip>
    </TableHead>
  )
}

/**
 * Partitions, with the replica placement in the same rows.
 *
 * These were two tabs. They are one table because they were always one
 * question asked twice: the grid said *where* a partition lives and the table
 * said *what state* it is in, and answering "which broker is the out-of-sync
 * replica on, and how far behind is that partition" meant holding one view in
 * your head while looking at the other.
 *
 * The broker columns come first, right after the partition number, because
 * that block is the shape you scan — a column of `L` glyphs drifting to one
 * broker is a leader imbalance, and a gap in it is visible before any number
 * is read.
 */
function Partitions({
  partitions,
  brokerIds,
  envId,
  clusterId,
  topic,
}: {
  partitions: Partition[]
  brokerIds: number[]
  envId: string
  clusterId: string
  topic: string
}) {
  // The same query the card reads, so this costs no second fan-out — and it
  // is joined by partition index rather than by position, because a partition
  // no broker reported a copy of is absent from the size answer and would
  // otherwise slide every row below it onto the wrong number.
  const size = useTopicSize(envId, clusterId, topic)
  const sizes = new Map(
    (size.data?.items[0]?.partitions ?? []).map((partition) => [
      partition.partition,
      partition.replicatedBytes,
    ])
  )
  const lags = new Map(
    (size.data?.items[0]?.partitions ?? []).map((partition) => [
      partition.partition,
      partition.maxFollowerLag,
    ])
  )
  // From the describe, not the size answer, so the column does not pop in
  // when the sizes arrive. A topic with no followers has nobody to lag, and
  // a column of dashes would only say so repeatedly.
  const hasFollowers = partitions.some(
    (partition) => partition.replicas.length > 1
  )

  return (
    <div className="space-y-3">
      <div className="overflow-x-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <Head label="partition" hint="its index within the topic" right />
              {/* One column per broker. Narrow and centred so the block reads
                  as a grid rather than as eight more columns of data. */}
              {brokerIds.map((broker, index) => (
                <Head
                  key={broker}
                  label={broker}
                  hint={`broker ${broker} — what it holds of each partition`}
                  className={cn(
                    "px-1 text-center font-mono font-normal",
                    index === 0 && "border-line border-l",
                    index === brokerIds.length - 1 && "border-line border-r"
                  )}
                />
              ))}
              <Head
                label="epoch"
                hint="leader epoch — it bumps on every leadership change"
                right
              />
              <Head
                label="earliest"
                hint="the oldest offset still retained"
                right
              />
              <Head
                label="latest"
                hint="the offset the next record will get"
                right
              />
              <Head
                label="records"
                hint="latest − earliest: what is retained, not what was ever written"
                right
              />
              <Head
                label="size"
                hint="bytes on disk for every non-future copy of this partition"
                right
              />
              {hasFollowers ? (
                <Head
                  label="lag"
                  hint="the worst follower's offset lag — 0 is every follower caught up"
                  right
                />
              ) : null}
            </TableRow>
          </TableHeader>
          <TableBody>
            {partitions.map((partition) => {
              const records =
                partition.earliestOffset !== null &&
                partition.latestOffset !== null
                  ? partition.latestOffset - partition.earliestOffset
                  : null
              return (
                <TableRow key={partition.partition}>
                  <TableCell className="text-right font-mono whitespace-nowrap">
                    {partition.partition}
                    {/* Every other state has a glyph. "No leader at all" has
                        the *absence* of one, which reads as nothing wrong
                        unless it is said. */}
                    {partition.leader === null ? (
                      <span className="text-danger ml-1.5" title="no leader">
                        ✕
                      </span>
                    ) : null}
                  </TableCell>
                  {brokerIds.map((broker, index) => {
                    const { label, style, title, preferred } = placementCell(
                      partition,
                      broker
                    )
                    return (
                      <TableCell
                        key={broker}
                        className={cn(
                          "px-1 py-0.5",
                          index === 0 && "border-line border-l",
                          index === brokerIds.length - 1 &&
                            "border-line border-r"
                        )}
                      >
                        <div
                          title={`p${partition.partition} on broker ${broker}: ${title}`}
                          style={style}
                          className={cn(
                            "mx-auto grid h-5 w-6 place-items-center rounded-[2px] font-mono text-[12px]",
                            preferred &&
                              "outline-ink-muted outline-2 -outline-offset-1"
                          )}
                        >
                          {label}
                        </div>
                      </TableCell>
                    )
                  })}
                  <TableCell className="text-ink-faint text-right font-mono">
                    {partition.leaderEpoch}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {count(partition.earliestOffset)}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {count(partition.latestOffset)}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {count(records)}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {pending(
                      sizes.get(partition.partition) ?? null,
                      bytes,
                      size.isFetching
                    )}
                  </TableCell>
                  {hasFollowers ? (
                    <TableCell
                      className={cn(
                        "text-right font-mono",
                        (lags.get(partition.partition) ?? 0) > 0 &&
                          "text-warn-ink"
                      )}
                    >
                      {pending(
                        lags.get(partition.partition) ?? null,
                        count,
                        size.isFetching
                      )}
                    </TableCell>
                  ) : null}
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      </div>
      {brokerIds.length > 0 ? <PlacementLegend /> : null}
    </div>
  )
}

function TopicConfigs({
  envId,
  clusterId,
  topic,
}: {
  envId: string
  clusterId: string
  topic: string
}) {
  const configs = useTopicConfigs(envId, clusterId, topic)
  const [onlyExplicit, setOnlyExplicit] = useState(true)

  const entries = configs.data?.items[0]?.entries ?? []
  const shown = onlyExplicit
    ? entries.filter((entry) => entry.isExplicit)
    : entries

  return (
    <>
      <Label className="mb-3 text-[12px] font-normal text-ink-muted">
        <input
          type="checkbox"
          checked={onlyExplicit}
          onChange={(event) => setOnlyExplicit(event.target.checked)}
        />
        only values someone set on this topic
      </Label>
      <ErrorChips errors={configs.data?.errors ?? []} />
      {configs.isLoading ? (
        <Spinner />
      ) : shown.length === 0 ? (
        <Empty>this topic has no overrides — everything is inherited</Empty>
      ) : (
        <ConfigTable entries={shown} total={entries.length} />
      )}
    </>
  )
}
