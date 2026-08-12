import { Link } from "@tanstack/react-router"
import { ArrowLeft } from "lucide-react"

import { useGroup } from "@/api/client"
import { ErrorChips, Mono, Spinner } from "@/components/domain"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import { PageTitle } from "@/components/page-title"
import { GroupMembers } from "@/features/groups/group-members"
import { GroupOffsets } from "@/features/groups/group-offsets"
import { GroupSubtitle } from "@/features/groups/group-subtitle"

export function GroupDetailPage({
  envId,
  clusterId,
  groupId,
}: {
  envId: string
  clusterId: string
  groupId: string
}) {
  const group = useGroup(envId, clusterId, groupId)

  const detail = group.data?.items[0]

  return (
    <>
      <PageTitle
        title={<span className="font-mono text-[18px]">{groupId}</span>}
        subtitle={detail ? <GroupSubtitle detail={detail} /> : undefined}
        actions={
          <Button variant="ghost" size="sm" asChild>
            <Link
              to="/environments/$envId/clusters/$clusterId/groups"
              params={{ envId, clusterId }}
            >
              <ArrowLeft aria-hidden />
              all groups
            </Link>
          </Button>
        }
      />

      <ErrorChips errors={group.data?.errors ?? []} />

      {group.isLoading ? (
        <Spinner />
      ) : !detail ? (
        <Card className="p-5 text-[13px] text-ink-muted">
          the cluster did not describe this group
        </Card>
      ) : detail.kind === "unrecognized" ? (
        // A *successful* description of an undescribable group: it exists, it
        // is listed, and this build has no schema for its kind. That is a
        // different thing from a failure and it renders differently.
        <Card className="max-w-2xl p-5">
          <h3 className="mb-2 font-semibold">This group cannot be opened</h3>
          <p className="text-[13px] text-ink-muted">
            The cluster reports it as{" "}
            <Mono>{detail.groupType || "an unnamed type"}</Mono>, which this
            build of kaas-ui has no schema for. The group is real and its state
            is <Mono>{detail.state}</Mono>; what is missing is the ability to
            describe its members. Upgrading kaas-ui is what changes this.
          </p>
        </Card>
      ) : (
        <GroupMembers members={detail.members} />
      )}

      <GroupOffsets envId={envId} clusterId={clusterId} groupId={groupId} />
    </>
  )
}
