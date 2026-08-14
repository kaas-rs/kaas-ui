import { useState } from "react"
import { Info } from "lucide-react"

import { useCapabilities, useCluster } from "@/api/client"
import { FeatureBadge, Section, Spinner } from "@/components/domain"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import { PageTitle } from "@/components/page-title"
import { ApiKeysTable } from "@/features/cluster/api-keys-table"

export function ClusterCapabilitiesPage({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  const cluster = useCluster(envId, clusterId)
  const brokers = cluster.data?.items[0]?.brokers ?? []

  // Null is "whichever the server picks", which is the lowest node id and is
  // not the same claim as picking it here: the answer names the broker that
  // actually replied, and the button lit is that one rather than a guess this
  // page made before asking.
  const [selected, setSelected] = useState<number | null>(null)
  const capabilities = useCapabilities(envId, clusterId, selected)
  const [showAll, setShowAll] = useState(false)

  const data = capabilities.data
  const answered = data?.source.nodeId ?? null

  const keys = !data
    ? []
    : showAll
      ? data.apiKeys
      : data.apiKeys.filter((key) => key.brokerAhead || key.negotiated === null)

  return (
    <>
      <PageTitle
        title="Capabilities"
        subtitle={
          data ? (
            <>
              as reported by broker{" "}
              <span className="font-mono">{data.source.nodeId ?? "?"}</span>{" "}
              <span className="text-ink-faint">({data.source.peer})</span>
            </>
          ) : (
            "asking a broker"
          )
        }
      />

      {/* Above everything that can fail, deliberately. A broker that will not
          answer renders the error card below, and if the picker went with it
          the only way back to a broker that works would be a reload. */}
      {brokers.length > 1 ? (
        <div className="mb-4 flex flex-wrap items-center gap-2">
          {brokers.map((broker) => {
            // Lit by what answered rather than by what was clicked, so the
            // default selection — nothing clicked, server chooses — shows
            // which broker that turned out to be.
            const active =
              selected === null
                ? answered === broker.nodeId
                : selected === broker.nodeId
            return (
              <Button
                key={broker.nodeId}
                size="sm"
                variant={active ? "default" : "outline"}
                className="font-mono text-[12px]"
                onClick={() => setSelected(broker.nodeId)}
              >
                broker {broker.nodeId}
              </Button>
            )
          })}
        </div>
      ) : null}

      {/* A note, not a card. A `Card` is the surface content sits on, and this
          is an aside *about* the content below it — sunken rather than raised,
          an accent rule down the edge, and the same icon-and-line shape a
          payload note has. Full width because the two sections below it are:
          a narrower column would read as a panel of its own. */}
      <div className="mb-6 flex items-start gap-2.5 rounded-sm border border-line border-l-2 border-l-rust-edge bg-surface-sunken px-3.5 py-2.5 text-[12px] text-ink-muted">
        <Info className="mt-0.5 size-3.5 shrink-0" aria-hidden />
        <p>
          The version table is{" "}
          <span className="font-medium">per connection</span>, deliberately:
          brokers mid-rolling-upgrade genuinely disagree, and a cluster-wide
          table would be wrong during exactly the window when being right
          matters. So this page names the broker it asked instead of pretending
          the answer is cluster-wide
          {brokers.length > 1
            ? ", and asking each of them in turn is how a half-finished upgrade becomes visible"
            : ""}
          .
        </p>
      </div>

      {capabilities.isLoading ? (
        <Spinner label="asking a broker" />
      ) : capabilities.error ? (
        <Card className="p-5 text-[13px]">
          <p className="mb-1 font-medium text-danger">
            the version table could not be read
            {selected === null ? "" : ` from broker ${selected}`}
          </p>
          <p className="text-ink-muted">{String(capabilities.error)}</p>
        </Card>
      ) : !data ? (
        <Spinner />
      ) : (
        <>
          <Section title="Features">
            <div className="grid grid-cols-[repeat(auto-fill,minmax(22rem,1fr))] gap-2">
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
              <Button
                variant="link"
                size="sm"
                onClick={() => setShowAll(!showAll)}
              >
                {showAll ? "show only the interesting ones" : "show all"}
              </Button>
            }
          >
            <ApiKeysTable keys={keys} />
          </Section>
        </>
      )}
    </>
  )
}
