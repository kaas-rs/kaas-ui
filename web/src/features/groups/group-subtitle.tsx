import type { GroupDetail } from "@/api/types"
import { Mono } from "@/components/domain"

export function GroupSubtitle({ detail }: { detail: GroupDetail }) {
  if (detail.kind === "unrecognized") {
    return (
      <span className="flex gap-3">
        <span>{detail.state}</span>
        <Mono>{detail.groupType || "unnamed kind"}</Mono>
      </span>
    )
  }
  if (detail.kind === "classic") {
    return (
      <span className="flex gap-3">
        <span>classic · {detail.state}</span>
        <Mono>{detail.protocol || detail.protocolType}</Mono>
        <span>{detail.members.length} members</span>
      </span>
    )
  }
  return (
    <span className="flex gap-3">
      <span>
        {detail.kind} · {detail.state}
      </span>
      <Mono>{detail.assignor}</Mono>
      <span>
        epoch {detail.groupEpoch}/{detail.assignmentEpoch}
      </span>
      <span>{detail.members.length} members</span>
    </span>
  )
}
