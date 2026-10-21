// One connector: its tasks, its config with the secrets withheld, the topics
// it touches and the offsets it has stored.
//
// The links are the point of having this inside kaas-ui rather than beside
// it: a topic it writes opens that topic, a sink's consumer group opens the
// lag view, and both are pages that already exist.

import { Link, useNavigate, useSearch } from "@tanstack/react-router"
import { AlertTriangle } from "lucide-react"

import { useConnector } from "@/api/client"
import type { ConnectorPage as ConnectorPageData } from "@/api/types"
import { Empty, ErrorChips, Mono, Spinner, Stat } from "@/components/domain"
import { PageTitle } from "@/components/page-title"
import { Card, CardContent } from "@/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ConfigTable } from "@/features/connect/config-table"
import {
  ConnectorStatusBadge,
  TaskState,
  connectorTypeLabel,
  shortClass,
} from "@/features/connect/connector-status"

export const CONNECTOR_PANES = ["tasks", "config", "topics", "offsets"] as const
export type ConnectorPane = (typeof CONNECTOR_PANES)[number]

export function ConnectorPage({
  envId,
  connectId,
  connector,
}: {
  envId: string
  connectId: string
  connector: string
}) {
  const { pane } = useSearch({
    from: "/environments/$envId/connect-clusters/$connectId/connectors/$connector",
  })
  const navigate = useNavigate()
  const page = useConnector(envId, connectId, connector)

  if (page.isLoading) return <Spinner label="describing the connector" />
  if (page.error) {
    return (
      <>
        <PageTitle
          title={connector}
          subtitle="This connector could not be read."
        />
        <p className="flex items-start gap-2 text-xs text-danger">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          {(page.error as Error).message}
        </p>
      </>
    )
  }
  const data = page.data
  if (!data) return null
  const { summary } = data.connector
  const kafkaCluster = data.card.kafkaCluster

  return (
    <>
      <PageTitle
        title={<span className="font-mono">{summary.name}</span>}
        subtitle={
          <span className="flex flex-wrap items-center gap-x-2">
            <span>{connectorTypeLabel(summary.type)} connector on</span>
            <Link
              to="/environments/$envId/connect-clusters/$connectId"
              params={{ envId, connectId }}
              search={{ listing: "connectors" }}
              className="hover:underline"
            >
              {data.card.name}
            </Link>
            <span
              className="font-mono text-ink-faint"
              title={summary.class ?? undefined}
            >
              {summary.class ?? ""}
            </span>
          </span>
        }
        actions={<ConnectorStatusBadge status={summary.status} />}
      />
      <ErrorChips errors={data.errors} />

      <Card className="mb-6 py-4">
        <CardContent className="px-5">
          <dl className="grid grid-cols-2 gap-4 sm:grid-cols-4">
            <Stat
              label="state"
              value={summary.state ?? "—"}
              hint="the connector's own state, verbatim"
            />
            <Stat
              label="tasks"
              value={`${summary.tasksRunning}/${summary.tasks.length}`}
              note={
                summary.tasksFailed > 0
                  ? `${summary.tasksFailed} failed`
                  : undefined
              }
              tone={summary.tasksFailed > 0 ? "danger" : undefined}
            />
            <Stat
              label="plugin"
              value={summary.version ?? "—"}
              hint="the plugin version the connector runs"
            />
            <div>
              <dt className="text-[12px] text-ink-muted">consumer group</dt>
              <dd className="truncate font-mono text-[15px]">
                {summary.consumerGroup ? (
                  kafkaCluster ? (
                    <Link
                      to="/environments/$envId/clusters/$clusterId/groups/$groupId"
                      params={{
                        envId,
                        clusterId: kafkaCluster,
                        groupId: summary.consumerGroup,
                      }}
                      className="hover:underline"
                      style={{ color: "var(--rust-ink)" }}
                    >
                      {summary.consumerGroup}
                    </Link>
                  ) : (
                    summary.consumerGroup
                  )
                ) : (
                  <span
                    className="text-ink-faint"
                    title="a source connector commits to no group"
                  >
                    —
                  </span>
                )}
              </dd>
            </div>
          </dl>
          {summary.trace ? <Trace trace={summary.trace} /> : null}
        </CardContent>
      </Card>

      <Tabs
        value={pane}
        onValueChange={(next) =>
          void navigate({
            to: "/environments/$envId/connect-clusters/$connectId/connectors/$connector",
            params: { envId, connectId, connector },
            search: { pane: next as ConnectorPane },
            replace: true,
          })
        }
      >
        <TabsList>
          <TabsTrigger value="tasks">tasks</TabsTrigger>
          <TabsTrigger value="config">config</TabsTrigger>
          {data.topicsAbsent ? null : (
            <TabsTrigger value="topics">topics</TabsTrigger>
          )}
          {data.offsetsAbsent ? null : (
            <TabsTrigger value="offsets">offsets</TabsTrigger>
          )}
        </TabsList>
        <TabsContent value="tasks" className="mt-4">
          <Tasks data={data} />
        </TabsContent>
        <TabsContent value="config" className="mt-4 space-y-6">
          <ConfigTable config={data.connector.config} />
        </TabsContent>
        <TabsContent value="topics" className="mt-4">
          <Topics envId={envId} kafkaCluster={kafkaCluster} data={data} />
        </TabsContent>
        <TabsContent value="offsets" className="mt-4">
          <Offsets data={data} />
        </TabsContent>
      </Tabs>
    </>
  )
}

function Trace({ trace }: { trace: string }) {
  return (
    <details className="mt-4">
      <summary className="cursor-pointer text-[12px] text-danger">
        stack trace
      </summary>
      <pre className="mt-2 max-h-80 overflow-auto rounded-sm border bg-muted/40 p-3 font-mono text-[11px] leading-relaxed">
        {trace}
      </pre>
    </details>
  )
}

function Tasks({ data }: { data: ConnectorPageData }) {
  const { summary, taskConfigs } = data.connector
  if (summary.tasks.length === 0) {
    return (
      <Empty>
        This connector has no tasks — configured, and doing nothing.
      </Empty>
    )
  }
  return (
    <div className="space-y-6">
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-16">task</TableHead>
              <TableHead>state</TableHead>
              <TableHead>worker</TableHead>
              <TableHead>plugin</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {summary.tasks.map((task) => (
              <TableRow key={task.id} className="align-top">
                <TableCell className="font-mono text-[12px]">
                  {task.id}
                </TableCell>
                <TableCell>
                  <TaskState state={task.state} />
                  {task.trace ? <Trace trace={task.trace} /> : null}
                </TableCell>
                <TableCell className="font-mono text-[12px] text-ink-faint">
                  {task.workerId ?? "—"}
                </TableCell>
                <TableCell className="font-mono text-[12px] text-ink-faint">
                  {task.version ?? "—"}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
      {taskConfigs.map((task) => (
        <details key={task.id}>
          <summary className="cursor-pointer text-[13px] text-ink-muted">
            task {task.id} config{" "}
            <span className="text-ink-faint">— generated by the connector</span>
          </summary>
          <div className="mt-3">
            <ConfigTable config={task.config} />
          </div>
        </details>
      ))}
    </div>
  )
}

function Topics({
  envId,
  kafkaCluster,
  data,
}: {
  envId: string
  kafkaCluster: string | null
  data: ConnectorPageData
}) {
  const topics = data.topics
  if (!topics)
    return (
      <Empty>The worker did not say which topics this connector uses.</Empty>
    )
  if (topics.length === 0) {
    return (
      <Empty>
        No topics recorded yet — the worker tracks a topic once it is used.
      </Empty>
    )
  }
  return (
    <div className="space-y-2">
      <p className="text-[12px] text-ink-faint">
        The topics the worker has seen this connector{" "}
        {data.connector.summary.type === "sink" ? "read" : "write"}, which is
        what it did rather than what its config allows.
      </p>
      <ul className="flex flex-wrap gap-2">
        {topics.map((topic) => (
          <li key={topic}>
            {kafkaCluster ? (
              <Link
                to="/environments/$envId/clusters/$clusterId/topics/$topic"
                params={{ envId, clusterId: kafkaCluster, topic }}
                className="rounded-sm border px-2 py-0.5 font-mono text-[12px] hover:underline"
                style={{ color: "var(--rust-ink)" }}
              >
                {topic}
              </Link>
            ) : (
              <span className="rounded-sm border px-2 py-0.5 font-mono text-[12px]">
                {topic}
              </span>
            )}
          </li>
        ))}
      </ul>
    </div>
  )
}

function Offsets({ data }: { data: ConnectorPageData }) {
  const offsets = data.offsets
  if (!offsets)
    return <Empty>The worker did not report this connector's offsets.</Empty>
  if (offsets.length === 0) {
    return (
      <Empty>
        No offsets stored. {shortClass(data.connector.summary.class)} has not
        committed a position yet, or does not keep one.
      </Empty>
    )
  }
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>partition</TableHead>
            <TableHead>offset</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {offsets.map((offset, index) => (
            <TableRow key={index}>
              <TableCell className="font-mono text-[12px]">
                <KeyValues values={offset.partition} />
              </TableCell>
              <TableCell className="font-mono text-[12px]">
                {offset.offset ? (
                  <KeyValues values={offset.offset} />
                ) : (
                  <span
                    className="text-ink-faint"
                    title="the worker reports null: this position was cleared"
                  >
                    cleared
                  </span>
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

function KeyValues({ values }: { values: Record<string, unknown> }) {
  const entries = Object.entries(values)
  if (entries.length === 0) return <span className="text-ink-faint">—</span>
  return (
    <span className="flex flex-col gap-0.5">
      {entries.map(([key, value]) => (
        <span key={key}>
          <span className="text-ink-faint">{key}</span>{" "}
          <Mono>
            {typeof value === "string" ? value : JSON.stringify(value)}
          </Mono>
        </span>
      ))}
    </span>
  )
}
