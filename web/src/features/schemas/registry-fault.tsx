import { AlertTriangle } from "lucide-react"

import type { RegistryCard } from "@/api/types"
import { cn } from "@/lib/utils"

/**
 * One line, and only when the registry is not answering.
 *
 * What is left of a banner that used to be here in every state. Saying "ready"
 * on a page that is visibly full of subjects was the page repeating itself,
 * and the id and endpoint it also carried are on the nav row's tooltip. The
 * failure is the part that was load-bearing: an unreachable registry returns
 * an empty list, and an empty list renders as "holds no subjects".
 */
export function RegistryFault({ registry }: { registry: RegistryCard }) {
  if (registry.status === "ready") return null
  return (
    <p
      className={cn(
        "mb-4 flex items-start gap-2 text-xs",
        registry.status === "misconfigured" ? "text-danger" : "text-warn-ink"
      )}
    >
      <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
      <span>
        This registry is {registry.status}
        {registry.error ? `: ${registry.error}` : "."} What is listed below is
        what it last answered, which may be nothing at all.
      </span>
    </p>
  )
}
