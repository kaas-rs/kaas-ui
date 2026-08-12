import { useCluster } from "@/api/client"
import {
  ClusterCounts,
  ErrorChips,
  Mono,
  Section,
  SnapshotAge,
  Spinner,
  StatusBadge,
} from "@/components/domain"
import { Card, CardContent } from "@/components/ui/card"
import { PageTitle } from "@/components/page-title"
import { BrokerTable } from "@/features/cluster/broker-table"

export function ClusterOverviewPage({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  const cluster = useCluster(envId, clusterId)

  if (cluster.isLoading) return <Spinner label={`connecting to ${clusterId}`} />
  if (cluster.error) {
    return (
      <Card className="p-5">
        <p className="mb-1 font-medium text-danger">
          {clusterId} is not available
        </p>
        <p className="text-[13px] text-ink-muted">{String(cluster.error)}</p>
        <p className="mt-3 text-[13px] text-ink-muted">
          kaas-ui keeps retrying in the background; this page will fill in when
          the cluster answers. Nothing else in the fleet is affected.
        </p>
      </Card>
    )
  }

  const detail = cluster.data?.items[0]
  if (!detail) return <Spinner />
  const card = detail.cluster

  return (
    <>
      <PageTitle
        title={card.name}
        subtitle={
          <span className="flex items-center gap-3">
            <StatusBadge status={card.status} />
            {card.clusterId ? <Mono>{card.clusterId}</Mono> : null}
          </span>
        }
        actions={
          <SnapshotAge
            ageMs={card.snapshotAgeMs}
            asOfMs={cluster.dataUpdatedAt}
            maxStalenessMs={card.maxStalenessMs}
          />
        }
      />

      <ErrorChips errors={cluster.data?.errors ?? []} />

      <Section title="Cluster">
        <Card>
          <CardContent>
            <ClusterCounts card={card} />
            {detail.description === null ? (
              <p className="mt-4 border-t pt-3 text-[12px] text-ink-muted">
                This cluster does not answer <Mono>DescribeCluster</Mono>, so
                the broker list below comes from the metadata snapshot alone.
                Everything on this page is real; the one thing missing is
                whether the controller has fenced a broker.
              </p>
            ) : null}
          </CardContent>
        </Card>
      </Section>

      <Section title="Brokers">
        <BrokerTable
          envId={envId}
          clusterId={clusterId}
          brokers={detail.brokers}
        />
      </Section>
    </>
  )
}
