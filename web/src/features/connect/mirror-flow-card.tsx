import { Link } from "@tanstack/react-router"
import { ArrowRight, Radar } from "lucide-react"

import type { FlowEnd, MirrorFlow, MirrorKind } from "@/api/types"
import { Card, CardContent, CardHeader } from "@/components/ui/card"
import { ConnectorStatusBadge } from "@/features/connect/connector-status"

const KIND: Record<MirrorKind, string> = {
  source: "source",
  checkpoint: "checkpoint",
  heartbeat: "heartbeat",
}

/** What each missing connector costs, in one line. */
const MISSING: Record<MirrorKind, string> = {
  source: "no source connector — nothing is replicated",
  checkpoint:
    "no checkpoint connector — offsets are not translated; a consumer failing over starts from auto.offset.reset",
  heartbeat:
    "no heartbeat connector — liveness is read from heartbeats replicated from the source, if any",
}

/** An alias, linked to the cluster it resolves to where it does. */
export function FlowEndLink({
  alias,
  end,
}: {
  alias: string
  end: FlowEnd | null
}) {
  if (!end) {
    return (
      <span
        className="font-mono text-ink-muted"
        title="this alias names no cluster kaas-ui shows — map it under mirror_aliases"
      >
        {alias}
      </span>
    )
  }
  return (
    <Link
      to="/environments/$envId/clusters/$clusterId"
      params={{ envId: end.environment, clusterId: end.id }}
      className="font-mono hover:underline"
      style={{ color: "var(--rust-ink)" }}
      title={`${end.environment}/${end.id}`}
    >
      {alias}
    </Link>
  )
}

/**
 * One flow, `source → target`, and where each of its connectors runs.
 *
 * The Connect cluster on every row is the point: the same flow's pieces can
 * run on a dedicated MirrorMaker and a general Connect cluster at once, and a
 * restart that should go to one of them goes nowhere if the page hides which.
 */
export function MirrorFlowCard({
  envId,
  flow,
}: {
  envId: string
  flow: MirrorFlow
}) {
  return (
    <Card className="gap-3 py-4">
      <CardHeader className="gap-1 px-4">
        <div className="flex items-center justify-between gap-3">
          <Link
            to="/environments/$envId/mirrors/$source/$target"
            params={{
              envId,
              source: flow.sourceAlias,
              target: flow.targetAlias,
            }}
            className="flex items-center gap-2 font-semibold hover:underline"
          >
            <span className="font-mono">{flow.sourceAlias}</span>
            <ArrowRight aria-hidden className="size-4 text-ink-faint" />
            <span className="font-mono">{flow.targetAlias}</span>
          </Link>
          {flow.discovered ? (
            <span
              className="inline-flex items-center gap-1 text-[12px] text-ink-muted"
              title="found from the clusters' internal topics; no Connect API runs it that kaas-ui knows"
            >
              <Radar aria-hidden className="size-3.5" />
              discovered from topics
            </span>
          ) : null}
        </div>
        {flow.topics ? (
          <p className="truncate text-[12px] text-ink-faint">
            topics <span className="font-mono">{flow.topics}</span>
          </p>
        ) : null}
      </CardHeader>
      <CardContent className="space-y-2 px-4">
        {flow.connectors.length === 0 ? (
          <p className="text-[12px] text-ink-muted">
            No Connect API reports this flow: a dedicated-mode MirrorMaker 2, or
            one on a Connect cluster kaas-ui is not configured with. Task state
            is unknown.
          </p>
        ) : (
          <ul className="space-y-1.5">
            {flow.connectors.map((connector) => (
              <li
                key={`${connector.connectCluster}/${connector.name}`}
                className="flex flex-wrap items-center justify-between gap-2 text-[13px]"
              >
                <span className="flex min-w-0 items-center gap-2">
                  <span className="w-20 shrink-0 text-[12px] text-ink-muted">
                    {KIND[connector.kind]}
                  </span>
                  <Link
                    to="/environments/$envId/connect-clusters/$connectId/connectors/$connector"
                    params={{
                      envId,
                      connectId: connector.connectCluster,
                      connector: connector.name,
                    }}
                    search={{ pane: "tasks" }}
                    className="truncate font-mono hover:underline"
                  >
                    {connector.name}
                  </Link>
                  <span className="truncate text-[12px] text-ink-faint">
                    on {connector.connectName}
                  </span>
                </span>
                <ConnectorStatusBadge status={connector.status} />
              </li>
            ))}
          </ul>
        )}
        {flow.missing.map((kind) => (
          <p key={kind} className="text-[12px] text-warn-ink">
            {MISSING[kind]}
          </p>
        ))}
      </CardContent>
    </Card>
  )
}
