import { Link } from "@tanstack/react-router"

import { useTopicConnectors } from "@/api/client"
import { ConnectorStatusBadge } from "@/features/connect/connector-status"

/**
 * Which connectors write or read this topic.
 *
 * Silent when there are none — most topics on most clusters have no
 * connector, and a heading saying so on every one of them is noise. From the
 * workers' *active* topics: what each connector was seen doing, not what its
 * config would allow, which is the only answer a `topics.regex` permits.
 */
export function TopicConnectorsLine({
  envId,
  clusterId,
  topic,
}: {
  envId: string
  clusterId: string
  topic: string
}) {
  const query = useTopicConnectors(envId, clusterId, topic)
  const connectors = query.data?.connectors ?? []
  const partial = (query.data?.errors.length ?? 0) > 0
  if (connectors.length === 0 && !partial) return null

  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-[13px]">
      {connectors.map((connector) => (
        <span
          key={`${connector.connectCluster}/${connector.name}`}
          className="inline-flex items-center gap-2"
        >
          <span className="text-ink-muted">
            {connector.type === "sink" ? "read by" : "written by"}
          </span>
          <Link
            to="/environments/$envId/connect-clusters/$connectId/connectors/$connector"
            params={{
              envId,
              connectId: connector.connectCluster,
              connector: connector.name,
            }}
            search={{ pane: "tasks" }}
            className="font-mono hover:underline"
            style={{ color: "var(--rust-ink)" }}
            title={`on ${connector.connectName}`}
          >
            {connector.name}
          </Link>
          <ConnectorStatusBadge status={connector.status} />
        </span>
      ))}
      {partial ? (
        <span
          className="text-[12px] text-warn-ink"
          title={query.data?.errors.map((e) => e.message).join("\n")}
        >
          some connectors could not be asked
        </span>
      ) : null}
    </div>
  )
}
