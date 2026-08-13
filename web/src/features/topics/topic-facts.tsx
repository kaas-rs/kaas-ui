import { useTopicConfigs, useTopicSize } from "@/api/client"
import type { TopicDetail as TopicDetailData } from "@/api/types"
import { ErrorChips, Stat } from "@/components/domain"
import { bytes, count } from "@/lib/format"
import { Card, CardContent } from "@/components/ui/card"

import { pending } from "@/features/topics/pending"

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
export function TopicFacts({
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
          <Stat
            label="partitions"
            value={count(partitions.length)}
            hint="how many the topic is split into — the unit of both ordering and parallelism"
          />
          {replicated ? (
            <Stat
              label="replication factor"
              value={count(replicationFactor)}
              hint="the smallest replica count across partitions, which is the guarantee every partition actually meets"
            />
          ) : null}
          {replicated ? (
            <Stat
              label="under-replicated"
              value={count(underReplicated)}
              tone={underReplicated > 0 ? "warn" : undefined}
              hint="partitions whose in-sync set is short of their replica count — replicas exist but are behind or offline"
            />
          ) : null}
          {replicated ? (
            <Stat
              label="in-sync replicas"
              value={count(inSync)}
              note={`of ${count(replicas)}`}
              tone={inSync < replicas ? "warn" : undefined}
              hint="both sums are across every partition, so this is copies rather than partitions"
            />
          ) : null}
          <Stat
            label="type"
            value={info.internal ? "internal" : "external"}
            hint="internal is Kafka's own bookkeeping — `__consumer_offsets` and its friends, which kaas-ui lists but never parses"
          />
          <Stat
            label="segment size"
            value={pending(onDisk, bytes, size.isFetching)}
            hint="bytes on disk from `DescribeLogDirs`; the headline is every replica summed, so it is roughly one copy times the replication factor"
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
            hint="what other UIs call a segment count — `DescribeLogDirs` reports no segment files, so this counts replica copies per directory"
            note={entries === null ? undefined : "one per copy, per directory"}
          />
          <Stat
            label="cleanup policy"
            value={cleanup ?? (configs.isLoading ? "\u00b7" : "\u2014")}
            hint="delete drops old segments, compact keeps the newest record per key — which is why a compacted topic holds fewer records than were written to it"
          />
          <Stat
            label="messages"
            value={pending(messages, count, false)}
            hint="latest − earliest summed across partitions: what is retained, not what was ever written"
            note={messages === null ? undefined : "retained"}
          />
        </dl>
      </CardContent>
    </Card>
  )
}
