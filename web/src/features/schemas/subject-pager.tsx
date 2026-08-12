import { Button } from "@/components/ui/button"

export function SubjectPager({
  offset,
  total,
  page,
  onOffsetChange,
}: {
  offset: number
  total: number
  page: number
  onOffsetChange: (offset: number) => void
}) {
  if (total <= page) return null
  return (
    <div className="mt-3 flex items-center gap-3 text-[12px]">
      <Button
        variant="outline"
        size="sm"
        disabled={offset === 0}
        onClick={() => onOffsetChange(Math.max(0, offset - page))}
      >
        previous
      </Button>
      <span className="text-ink-muted">
        {offset + 1}–{Math.min(offset + page, total)} of {total}
      </span>
      <Button
        variant="outline"
        size="sm"
        disabled={offset + page >= total}
        onClick={() => onOffsetChange(offset + page)}
      >
        next
      </Button>
    </div>
  )
}
