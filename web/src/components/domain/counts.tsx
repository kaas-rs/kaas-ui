import { count } from "@/lib/format"
import type { ClusterCard as ClusterCardData } from "@/api/types"
import { Stat } from "./stat"

/** The summary a fleet card and a cluster header both want. */
export function ClusterCounts({ card }: { card: ClusterCardData }) {
  return (
    <dl className="grid grid-cols-3 gap-x-4 gap-y-2 text-[13px]">
      <Stat
        label="brokers"
        value={count(card.brokerCount)}
        hint="nodes in the metadata this cluster last reported — controllers that serve no partitions included."
      />
      <Stat
        label="topics"
        value={count(card.topicCount)}
        note={
          card.internalTopicCount
            ? `${card.internalTopicCount} internal`
            : undefined
        }
        hint="every topic, Kafka's own bookkeeping ones included — the note counts those separately."
      />
      <Stat
        label="partitions"
        value={count(card.partitionCount)}
        hint="partitions across every topic, counted once each rather than once per replica."
      />
      <Stat
        label="offline"
        value={count(card.offlinePartitionCount)}
        tone={card.offlinePartitionCount > 0 ? "danger" : undefined}
        hint="partitions with no leader or an offline replica — nothing can be read from or written to one."
      />
      <Stat
        label="under-replicated"
        value={count(card.underReplicatedPartitionCount)}
        tone={card.underReplicatedPartitionCount > 0 ? "warn" : undefined}
        hint="partitions whose in-sync set is short of their replica count: still serving, with less redundancy than configured."
      />
      <Stat
        label="controller"
        value={card.controllerId === null ? "—" : String(card.controllerId)}
        hint="the broker this cluster names as the active controller; blank where it answers no DescribeCluster at all."
      />
    </dl>
  )
}

/**
 * What a schema registry holds, in the shape [`ClusterCounts`] uses.
 *
 * Three numbers off one summary, which is why the card asks for `limit=0`: the
 * counts describe the whole listing and the page describes fifty rows, so
 * nothing here needs a single subject to travel. The columns that *do* need
 * one — id, format, version, compatibility — are two registry calls each and
 * stay on the page that has a table to put them in.
 *
 * `topics` is the interesting one: what these subjects cover, which is not how
 * many topics exist on any cluster that reads them.
 */
export function RegistryCounts({
  summary,
  pending,
}: {
  /** `null` until the listing answers, which is `·` rather than `0`. */
  summary: {
    total: number
    topics: number
    dangling: number | null
  } | null
  pending: boolean
}) {
  const show = (value: number | null | undefined) =>
    value === undefined || value === null ? (pending ? "·" : "—") : count(value)

  return (
    <dl className="grid grid-cols-3 gap-x-4 gap-y-2 text-[13px]">
      <Stat
        label="subjects"
        value={show(summary?.total)}
        hint="Everything registered here, across every naming strategy."
      />
      <Stat
        label="topics"
        value={show(summary?.topics)}
        hint="Distinct topics these subject names resolve to, read off the names alone."
      />
      {/* Not toned. A schema kept after its topic went is ordinary
          housekeeping on most fleets, and a number that is amber on every
          registry teaches people to stop reading the ones that are not. */}
      <Stat
        label="dangling"
        value={show(summary?.dangling)}
        hint="Subjects naming a topic that is on no cluster reading this registry — deleting a topic never touches the registry, so its schema stays. Shown as — while a cluster that reads this registry is disconnected, and where none does."
      />
    </dl>
  )
}
