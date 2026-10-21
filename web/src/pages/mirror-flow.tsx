// One MirrorMaker 2 flow: its connectors, the topics it carries, and what the
// clusters themselves say about it — heartbeats, checkpoints, offset syncs —
// decoded from MirrorMaker's internal topics.

import { Link } from "@tanstack/react-router"
import { AlertTriangle, ArrowRight } from "lucide-react"

import { useMirrorFlow } from "@/api/client"
import type { FlowEnd, MirrorFlowDetail } from "@/api/types"
import { Empty, ErrorChips, Mono, Section, Spinner } from "@/components/domain"
import { PageTitle } from "@/components/page-title"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  FlowEndLink,
  MirrorFlowCard,
} from "@/features/connect/mirror-flow-card"
import { count, duration } from "@/lib/format"
import { cn } from "@/lib/utils"

/** Older than this, a heartbeat says replication has stalled. MirrorMaker
    emits one a second by default; the fixtures, every ten. */
const STALE_MS = 60_000

export function MirrorFlowPage({
  envId,
  source,
  target,
}: {
  envId: string
  source: string
  target: string
}) {
  const detail = useMirrorFlow(envId, source, target)

  if (detail.isLoading) return <Spinner label="reading the flow" />
  if (detail.error) {
    return (
      <>
        <PageTitle title={`${source} → ${target}`} />
        <p className="flex items-start gap-2 text-xs text-danger">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          {(detail.error as Error).message}
        </p>
      </>
    )
  }
  const data = detail.data
  if (!data) return null
  const { flow } = data

  return (
    <>
      <PageTitle
        title={
          <span className="flex items-center gap-2 font-mono">
            {source}
            <ArrowRight aria-hidden className="size-5 text-ink-faint" />
            {target}
          </span>
        }
        subtitle={
          <span className="flex flex-wrap items-center gap-x-2">
            <span>MirrorMaker 2 flow from</span>
            <FlowEndLink alias={flow.sourceAlias} end={flow.source} />
            <span>to</span>
            <FlowEndLink alias={flow.targetAlias} end={flow.target} />
            {flow.policy ? (
              <span className="text-ink-faint">
                ·{" "}
                {flow.policy.kind === "default"
                  ? `topics renamed ${source}${flow.policy.separator}<topic>`
                  : flow.policy.kind === "identity"
                    ? "topic names kept"
                    : `custom policy ${flow.policy.class}`}
              </span>
            ) : null}
          </span>
        }
      />
      <ErrorChips errors={data.errors} />

      <div className="mb-8 max-w-2xl">
        <MirrorFlowCard envId={envId} flow={flow} />
      </div>

      <Section title="Liveness">
        <Heartbeats data={data} />
      </Section>
      <Section title="Mirrored topics">
        <Topics data={data} />
      </Section>
      <Section title="Failover: where groups would resume">
        <Checkpoints envId={envId} data={data} />
      </Section>
      <Section title="Replication progress">
        <Syncs data={data} />
      </Section>
    </>
  )
}

function TopicLink({ end, topic }: { end: FlowEnd | null; topic: string }) {
  if (!end) return <Mono>{topic}</Mono>
  return (
    <Link
      to="/environments/$envId/clusters/$clusterId/topics/$topic"
      params={{ envId: end.environment, clusterId: end.id, topic }}
      className="font-mono hover:underline"
      style={{ color: "var(--rust-ink)" }}
    >
      {topic}
    </Link>
  )
}

function Heartbeats({ data }: { data: MirrorFlowDetail }) {
  if (data.heartbeats.length === 0) {
    return (
      <Empty>
        No heartbeat for this flow on the target: no heartbeat connector emits
        one, none is replicated from the source, or the target is not connected.
      </Empty>
    )
  }
  return (
    <ul className="space-y-2 text-[13px]">
      {data.heartbeats.map((seen) => {
        const stale = seen.ageMs > STALE_MS
        const replicated = seen.topic !== "heartbeats"
        return (
          <li
            key={seen.topic}
            className="flex flex-wrap items-baseline gap-x-3"
          >
            <span
              className={cn(
                "font-mono font-medium",
                stale ? "text-danger" : "text-ok"
              )}
            >
              {duration(Math.max(0, seen.ageMs))} ago
            </span>
            <span className="text-ink-muted">
              {replicated
                ? "newest heartbeat replicated from the source"
                : "newest heartbeat the target's heartbeat connector emitted"}
            </span>
            <Mono>{seen.topic}</Mono>
            <span className="text-[12px] text-ink-faint">
              ({seen.heartbeat.sourceAlias} → {seen.heartbeat.targetAlias})
            </span>
            {stale ? (
              <span className="text-[12px] text-danger">
                older than a minute — replication may have stalled
              </span>
            ) : null}
          </li>
        )
      })}
    </ul>
  )
}

function Topics({ data }: { data: MirrorFlowDetail }) {
  if (data.topics.length === 0) {
    return <Empty>No mirrored topic recorded yet.</Empty>
  }
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>on the source</TableHead>
            <TableHead>on the target</TableHead>
            <TableHead>partitions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {data.topics.map((topic) => {
            const drift =
              topic.sourcePartitions !== null &&
              topic.targetPartitions !== null &&
              topic.sourcePartitions !== topic.targetPartitions
            return (
              <TableRow key={topic.targetTopic}>
                <TableCell>
                  {topic.sourceTopic ? (
                    <TopicLink
                      end={data.flow.source}
                      topic={topic.sourceTopic}
                    />
                  ) : (
                    <span className="text-ink-faint">
                      unknown under this policy
                    </span>
                  )}
                </TableCell>
                <TableCell>
                  <TopicLink end={data.flow.target} topic={topic.targetTopic} />
                </TableCell>
                <TableCell
                  className={cn(
                    "font-mono text-[12px]",
                    drift && "text-warn-ink"
                  )}
                  title={
                    drift
                      ? "the two sides have different partition counts"
                      : undefined
                  }
                >
                  {topic.sourcePartitions ?? "?"} →{" "}
                  {topic.targetPartitions ?? "?"}
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}

function Checkpoints({
  envId,
  data,
}: {
  envId: string
  data: MirrorFlowDetail
}) {
  if (data.checkpoints.length === 0) {
    return (
      <Empty>
        No checkpoint: no checkpoint connector, no group consuming a mirrored
        topic on the source, or the target is not connected.
      </Empty>
    )
  }
  const target = data.flow.target
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>group</TableHead>
            <TableHead>topic</TableHead>
            <TableHead>partition</TableHead>
            <TableHead>committed on the source</TableHead>
            <TableHead>would resume at on the target</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {data.checkpoints.map(({ checkpoint, targetTopic }) => (
            <TableRow
              key={`${checkpoint.group}/${checkpoint.topic}/${checkpoint.partition}`}
            >
              <TableCell>
                {target && target.environment === envId ? (
                  <Link
                    to="/environments/$envId/clusters/$clusterId/groups/$groupId"
                    params={{
                      envId,
                      clusterId: target.id,
                      groupId: checkpoint.group,
                    }}
                    className="font-mono hover:underline"
                  >
                    {checkpoint.group}
                  </Link>
                ) : (
                  <Mono>{checkpoint.group}</Mono>
                )}
              </TableCell>
              <TableCell className="font-mono text-[12px]">
                {targetTopic ?? checkpoint.topic}
              </TableCell>
              <TableCell className="font-mono text-[12px]">
                {checkpoint.partition}
              </TableCell>
              <TableCell className="font-mono text-[12px]">
                {count(checkpoint.upstreamOffset)}
              </TableCell>
              <TableCell className="font-mono text-[12px]">
                {count(checkpoint.downstreamOffset)}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

function Syncs({ data }: { data: MirrorFlowDetail }) {
  if (data.offsetSyncs.length === 0) {
    return (
      <Empty>
        No offset sync on the source yet. MirrorMaker writes one when a
        partition has moved far enough since the last, so a quiet topic can have
        none.
      </Empty>
    )
  }
  return (
    <div className="space-y-2">
      <p className="text-[12px] text-ink-faint">
        The newest offset sync per partition: replication has got at least this
        far. Syncs are sparse by design, so this is a lower bound, not a cursor.
      </p>
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>topic</TableHead>
              <TableHead>partition</TableHead>
              <TableHead>source offset</TableHead>
              <TableHead>landed at</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {data.offsetSyncs.map((sync) => (
              <TableRow key={`${sync.topic}/${sync.partition}`}>
                <TableCell className="font-mono text-[12px]">
                  {sync.topic}
                </TableCell>
                <TableCell className="font-mono text-[12px]">
                  {sync.partition}
                </TableCell>
                <TableCell className="font-mono text-[12px]">
                  {count(sync.upstreamOffset)}
                </TableCell>
                <TableCell className="font-mono text-[12px]">
                  {count(sync.downstreamOffset)}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}
