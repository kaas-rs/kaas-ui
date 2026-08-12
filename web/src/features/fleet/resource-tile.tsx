import type { ResourceCard as ResourceCardData } from "@/api/types"
import { RESOURCE_KINDS } from "@/components/domain"
import { Card, CardContent, CardHeader } from "@/components/ui/card"

/**
 * Something in this environment that is not a Kafka cluster.
 *
 * Inventory, not monitoring. kaas-ui dials none of these, so the card carries
 * no status badge and says so — a green dot earned by a correctly typed URL
 * would be worse than no dot at all.
 *
 * It used to be `self-start`, its own height rather than a cluster card's. It
 * is not any more: a row of cards that each stop where their content does is a
 * ragged bottom edge, and the reader reads that as one card being *less*
 * rather than as one card having less to say. Every tile stretches now, and
 * what fills the difference is the space above the footer.
 */
export function ResourceTile({ card }: { card: ResourceCardData }) {
  const kind = RESOURCE_KINDS[card.kind]
  const Icon = kind.icon

  return (
    <Card className="gap-3 py-4">
      <CardHeader className="gap-2 px-4">
        <div className="flex items-start justify-between gap-3">
          <span className="font-semibold">{card.name}</span>
          <span
            className="shrink-0 rounded-sm border border-dashed px-1.5 py-0.5 text-[11px] text-ink-faint"
            title="kaas-ui does not connect to this, so it has no health to report"
          >
            not probed
          </span>
        </div>
        <div className="flex items-center gap-1.5 text-[12px] text-ink-muted">
          <Icon aria-hidden className="size-3.5" />
          {kind.label}
          <span className="font-mono text-[11px] text-ink-faint">
            {card.id}
          </span>
        </div>
      </CardHeader>

      <CardContent className="px-4">
        {card.endpoint ? (
          <p className="break-all font-mono text-[12px] text-ink-muted">
            {card.endpoint}
          </p>
        ) : null}
        {card.note ? (
          <p className="mt-2 text-[12px] text-ink-muted">{card.note}</p>
        ) : null}
      </CardContent>
    </Card>
  )
}
