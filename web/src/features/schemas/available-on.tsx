import { Link } from "@tanstack/react-router"

import { useEnvironment, useTopics } from "@/api/client"
import type { SubjectNaming } from "@/api/types"
import { Empty, HintHead, Mono, Section } from "@/components/domain"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

import { NamingNote } from "./naming-note"

/** How many topics to search when resolving a subject's topic. */
const PAGE = 50

/**
 * Every cluster this schema resolves on, and where its topic actually is.
 *
 * The question the page could not answer before. A registry serves an
 * *environment*, so a subject is not a fact about one cluster — every cluster
 * that decodes against this registry resolves schema id 1 to this schema, from
 * the same handle and the same cache. The old button picked the first such
 * cluster and linked to it, which was a guess dressed as an answer: on a
 * two-cluster environment it silently hid one of them.
 *
 * Two different claims, kept apart because they can disagree:
 *
 * * **the schema resolves here** — true of every cluster in `usedBy`, by
 *   configuration, whether or not anything has ever produced against it;
 * * **the topic is here** — under the two strategies whose subject contains a
 *   topic, and only where the cluster actually holds it. A subject outlives its
 *   topic, and a link to a topic that is not there is worse than no link.
 *
 * Which strategy that is comes from the server, which has the schema and can
 * therefore take a declared record name off the end of a subject exactly. The
 * page used to strip `-value` and give up on anything else, so every
 * `TopicRecordNameStrategy` subject read as unlinkable when its topic was
 * sitting in the name.
 */
export function AvailableOn({
  envId,
  registryId,
  subject,
  naming,
}: {
  envId: string
  registryId: string
  subject: string
  naming: SubjectNaming
}) {
  const environment = useEnvironment(envId)
  const usedBy =
    environment.data?.items[0]?.schemaRegistries.find(
      (entry) => entry.registry.id === registryId
    )?.usedBy ?? []
  const topic = naming.topic

  if (usedBy.length === 0) {
    return (
      <Section title="Available on">
        <Empty>
          No cluster in this environment decodes against this registry, so
          nothing resolves <Mono>{subject}</Mono> today. The subject is
          registered all the same.
        </Empty>
      </Section>
    )
  }

  return (
    <Section title="Available on">
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <HintHead
                label="cluster"
                hint="a cluster in this environment that names this registry — not one that necessarily uses this subject"
              />
              <HintHead
                label="schema resolves"
                hint="whether a record carrying this schema id decodes here, which is a property of the registry rather than of the cluster"
              />
              {/* The column exists only when the subject holds a topic. A
                  `RecordNameStrategy` subject would otherwise repeat one
                  sentence down every row, saying the same thing about the
                  subject each time as though it were a fact about the cluster
                  — and it would cost a topic listing per cluster to say it. */}
              {topic === null ? null : (
                <HintHead
                  label={
                    <>
                      topic <span className="font-mono">{topic}</span>
                    </>
                  }
                  hint="whether the topic this subject names exists on that cluster"
                />
              )}
            </TableRow>
          </TableHeader>
          <TableBody>
            {usedBy.map((clusterId) =>
              topic === null ? (
                <TableRow key={clusterId}>
                  <TableCell>
                    <ClusterLink envId={envId} clusterId={clusterId} />
                  </TableCell>
                  <TableCell className="text-ok-ink text-[12px]">yes</TableCell>
                </TableRow>
              ) : (
                <ClusterRow
                  key={clusterId}
                  envId={envId}
                  clusterId={clusterId}
                  topic={topic}
                />
              )
            )}
          </TableBody>
        </Table>
      </div>
      <NamingNote naming={naming} subject={subject} />
      <p className="mt-2 text-[11px] text-ink-faint">
        Every cluster here holds the same <Mono>Arc&lt;RegistryHandle&gt;</Mono>
        , so schema id {""}
        <Mono>1</Mono> is genuinely the same schema on all of them — one set of
        decoders, one id→schema cache.
      </p>
    </Section>
  )
}

/** The cluster's own page. Both row shapes need it, only one has a topic. */
function ClusterLink({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  return (
    <Link
      to="/environments/$envId/clusters/$clusterId"
      params={{ envId, clusterId }}
      className="font-mono hover:underline"
      style={{ color: "var(--rust-ink)" }}
    >
      {clusterId}
    </Link>
  )
}

/**
 * One cluster's row, for a subject that names a topic.
 *
 * A hook per row rather than one lookup for all of them, because `useTopics`
 * is per cluster. It costs nothing at the broker — the topic list is served
 * from the metadata snapshot — so the honest answer is worth the extra query.
 * Only rendered where there is a topic to look for, so the query is never the
 * empty search that used to fetch fifty topics to display nothing.
 */
function ClusterRow({
  envId,
  clusterId,
  topic,
}: {
  envId: string
  clusterId: string
  topic: string
}) {
  const topics = useTopics(envId, clusterId, { search: topic, limit: PAGE })
  const exists = topics.data?.items.some((entry) => entry.name === topic)

  return (
    <TableRow>
      <TableCell>
        <ClusterLink envId={envId} clusterId={clusterId} />
      </TableCell>
      <TableCell className="text-ok-ink text-[12px]">yes</TableCell>
      <TableCell className="text-[12px]">
        {topics.isLoading ? (
          <span className="text-ink-faint">·</span>
        ) : exists ? (
          <Link
            to="/environments/$envId/clusters/$clusterId/topics/$topic"
            params={{ envId, clusterId, topic }}
            className="font-mono hover:underline"
            style={{ color: "var(--rust-ink)" }}
          >
            {topic}
          </Link>
        ) : (
          <span className="text-ink-faint">absent</span>
        )}
      </TableCell>
    </TableRow>
  )
}
