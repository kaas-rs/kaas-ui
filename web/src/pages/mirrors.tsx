// The MirrorMaker 2 flows of one environment.
//
// Gathered from every Connect cluster here — dedicated to MirrorMaker or not
// — and from the clusters' own internal topics, so a dedicated-mode
// MirrorMaker with no Connect API still appears. A flow whose end you cannot
// see is not listed at all.

import { AlertTriangle } from "lucide-react"

import { useMirrors } from "@/api/client"
import { Empty, ErrorChips, Mono, Spinner } from "@/components/domain"
import { PageTitle } from "@/components/page-title"
import { MirrorFlowCard } from "@/features/connect/mirror-flow-card"

export function MirrorsPage({ envId }: { envId: string }) {
  const mirrors = useMirrors(envId)

  if (mirrors.isLoading)
    return <Spinner label="looking for MirrorMaker 2 flows" />
  if (mirrors.error) {
    return (
      <>
        <PageTitle title="MirrorMaker 2" />
        <p className="flex items-start gap-2 text-xs text-danger">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          {(mirrors.error as Error).message}
        </p>
      </>
    )
  }
  const flows = mirrors.data?.flows ?? []

  return (
    <>
      <PageTitle
        title="MirrorMaker 2"
        subtitle={`${flows.length} flow${flows.length === 1 ? "" : "s"}, from every Connect cluster here and from the clusters' internal topics`}
      />
      <ErrorChips errors={mirrors.data?.errors ?? []} />
      {flows.length === 0 ? (
        <Empty>
          No MirrorMaker 2 flow here: no Connect cluster runs a Mirror
          connector, and no cluster holds a <Mono>*.checkpoints.internal</Mono>{" "}
          or <Mono>mm2-offset-syncs.*.internal</Mono> topic.
        </Empty>
      ) : (
        <div className="grid gap-4 grid-cols-[repeat(auto-fill,minmax(min(24rem,100%),1fr))]">
          {flows.map((flow) => (
            <MirrorFlowCard
              key={`${flow.sourceAlias}->${flow.targetAlias}`}
              envId={envId}
              flow={flow}
            />
          ))}
        </div>
      )}
    </>
  )
}
