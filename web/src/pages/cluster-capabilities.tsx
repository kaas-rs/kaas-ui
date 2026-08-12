import { useState } from "react"

import { useCapabilities } from "@/api/client"
import { FeatureBadge, Section, Spinner } from "@/components/domain"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { PageTitle } from "@/components/page-title"
import { ApiKeysTable } from "@/features/cluster/api-keys-table"

export function ClusterCapabilitiesPage({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  const capabilities = useCapabilities(envId, clusterId)
  const [showAll, setShowAll] = useState(false)

  if (capabilities.isLoading) return <Spinner label="asking a broker" />
  if (capabilities.error) {
    return (
      <Card className="p-5 text-[13px]">
        <p className="mb-1 font-medium text-danger">
          the version table could not be read
        </p>
        <p className="text-ink-muted">{String(capabilities.error)}</p>
      </Card>
    )
  }

  const data = capabilities.data
  if (!data) return <Spinner />

  const keys = showAll
    ? data.apiKeys
    : data.apiKeys.filter((key) => key.brokerAhead || key.negotiated === null)

  return (
    <>
      <PageTitle
        title="Capabilities"
        subtitle={
          <>
            as reported by broker{" "}
            <span className="font-mono">{data.source.nodeId ?? "?"}</span>{" "}
            <span className="text-ink-faint">({data.source.peer})</span>
          </>
        }
      />

      <Card className="mb-6 max-w-3xl">
        <CardContent className="text-[13px] text-ink-muted">
          The version table is <strong>per connection</strong>, deliberately:
          brokers mid-rolling-upgrade genuinely disagree, and a cluster-wide
          table would be wrong during exactly the window when being right
          matters. So this page names the broker it asked instead of pretending
          the answer is cluster-wide.
        </CardContent>
      </Card>

      <Section title="Features">
        <div className="grid gap-2 grid-cols-[repeat(auto-fill,minmax(22rem,1fr))]">
          {data.features.map((entry) => (
            <div
              key={entry.feature}
              className="flex items-center justify-between gap-3 rounded-sm border bg-card px-3 py-2"
            >
              <span className="text-[13px]">{entry.feature}</span>
              <FeatureBadge entry={entry} />
            </div>
          ))}
        </div>
      </Section>

      <Section
        title={`API keys (${data.apiKeys.length} advertised, ${data.brokerAheadCount} ahead of this build)`}
        actions={
          <Button variant="link" size="sm" onClick={() => setShowAll(!showAll)}>
            {showAll ? "show only the interesting ones" : "show all"}
          </Button>
        }
      >
        <ApiKeysTable keys={keys} />
      </Section>
    </>
  )
}
