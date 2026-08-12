import { useCapabilities, useGroups } from "@/api/client"
import {
  Empty,
  ErrorChips,
  Spinner,
  UnsupportedApiPanel,
  featureState,
} from "@/components/domain"
import { count } from "@/lib/format"
import { PageTitle } from "@/components/page-title"
import { GroupTable } from "@/features/groups/group-table"

export function GroupsPage({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  const capabilities = useCapabilities(envId, clusterId)
  const groups = useGroups(envId, clusterId)

  // The route exists even where the api does not, so a URL shared from one
  // cluster and opened against another degrades into an explanation rather
  // than a dead end.
  const state = featureState(capabilities.data?.features, "consumerGroups")
  if (state?.state === "unsupported") {
    return (
      <>
        <PageTitle title="Consumer groups" />
        <UnsupportedApiPanel
          api={state.api}
          apiKey={state.apiKey}
          broker={state.broker}
          ours={state.ours}
          what="the group list"
        />
      </>
    )
  }

  const items = groups.data?.items ?? []

  return (
    <>
      <PageTitle
        title="Consumer groups"
        subtitle={`${count(items.length)} listed`}
      />
      <ErrorChips errors={groups.data?.errors ?? []} />

      {groups.isLoading ? (
        <Spinner />
      ) : items.length === 0 ? (
        <Empty>this cluster has no groups</Empty>
      ) : (
        <GroupTable envId={envId} clusterId={clusterId} items={items} />
      )}
    </>
  )
}
