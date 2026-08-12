import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"

export function TopicListControls({
  search,
  internal,
  replication,
  onSearch,
  onInternal,
  onReplication,
}: {
  search: string
  internal: boolean
  replication: boolean
  onSearch: (value: string) => void
  onInternal: (checked: boolean) => void
  onReplication: (checked: boolean) => void
}) {
  return (
    <div className="mb-4 flex flex-wrap items-center gap-4">
      <Input
        value={search}
        onChange={(event) => onSearch(event.target.value)}
        placeholder="filter by name"
        className="h-8 max-w-xs"
      />
      <Label className="text-[12px] font-normal text-ink-muted">
        <input
          type="checkbox"
          checked={internal}
          onChange={(event) => onInternal(event.target.checked)}
        />
        internal topics
      </Label>
      <Label className="text-[12px] font-normal text-ink-muted">
        <input
          type="checkbox"
          checked={replication}
          onChange={(event) => onReplication(event.target.checked)}
        />
        replication
      </Label>
    </div>
  )
}
