import { useState } from "react"

import { useTopicConfigs } from "@/api/client"
import { Empty, ErrorChips, Spinner } from "@/components/domain"
import { Label } from "@/components/ui/label"
import { ConfigTable } from "@/features/cluster/config-table"

export function TopicConfigs({
  envId,
  clusterId,
  topic,
}: {
  envId: string
  clusterId: string
  topic: string
}) {
  const configs = useTopicConfigs(envId, clusterId, topic)
  const [onlyExplicit, setOnlyExplicit] = useState(true)

  const entries = configs.data?.items[0]?.entries ?? []
  const shown = onlyExplicit
    ? entries.filter((entry) => entry.isExplicit)
    : entries

  return (
    <>
      <Label className="mb-3 text-[12px] font-normal text-ink-muted">
        <input
          type="checkbox"
          checked={onlyExplicit}
          onChange={(event) => setOnlyExplicit(event.target.checked)}
        />
        only values someone set on this topic
      </Label>
      <ErrorChips errors={configs.data?.errors ?? []} />
      {configs.isLoading ? (
        <Spinner />
      ) : shown.length === 0 ? (
        <Empty>this topic has no overrides — everything is inherited</Empty>
      ) : (
        <ConfigTable entries={shown} total={entries.length} />
      )}
    </>
  )
}
