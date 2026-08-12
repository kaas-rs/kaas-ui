import { useState } from "react"

import { useCluster, useClusterConfigs } from "@/api/client"
import { ErrorChips, Spinner } from "@/components/domain"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import { PageTitle } from "@/components/page-title"
import { ConfigTable } from "@/features/cluster/config-table"

export function ClusterConfigsPage({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  const cluster = useCluster(envId, clusterId)
  const brokers = cluster.data?.items[0]?.brokers ?? []
  const [selected, setSelected] = useState<string | null>(null)
  const resource =
    selected ?? (brokers[0] ? `broker:${brokers[0].nodeId}` : null)
  const configs = useClusterConfigs(envId, clusterId, resource)
  const [onlyExplicit, setOnlyExplicit] = useState(false)

  const entries = configs.data?.items[0]?.entries ?? []
  const shown = onlyExplicit
    ? entries.filter((entry) => entry.isExplicit)
    : entries

  return (
    <>
      <PageTitle
        title="Configuration"
        subtitle="A viewer. AlterConfigs is a mutating api and is absent from kaas-ui entirely."
      />

      <div className="mb-4 flex flex-wrap items-center gap-2">
        {brokers.map((broker) => {
          const value = `broker:${broker.nodeId}`
          const active = resource === value
          return (
            <Button
              key={broker.nodeId}
              size="sm"
              variant={active ? "default" : "outline"}
              className="font-mono text-[12px]"
              onClick={() => setSelected(value)}
            >
              broker {broker.nodeId}
            </Button>
          )
        })}
        <Label className="ml-auto text-[12px] font-normal text-ink-muted">
          <input
            type="checkbox"
            checked={onlyExplicit}
            onChange={(event) => setOnlyExplicit(event.target.checked)}
          />
          only values someone set
        </Label>
      </div>

      <ErrorChips errors={configs.data?.errors ?? []} />

      {configs.isLoading ? (
        <Spinner />
      ) : (
        <ConfigTable entries={shown} total={entries.length} />
      )}
    </>
  )
}
