import { Check, X } from "lucide-react"

import { Badge } from "@/components/ui/badge"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import type { FeatureEntry } from "@/api/types"

/** A feature the cluster does or does not have. */
export function FeatureBadge({ entry }: { entry: FeatureEntry }) {
  if (entry.state === "available") {
    return (
      <Badge variant="outline" className="text-ok">
        <Check aria-hidden className="size-3" />
        available
      </Badge>
    )
  }
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge variant="outline" className="font-mono text-ink-faint">
          <X aria-hidden className="size-3" />
          {entry.api}
        </Badge>
      </TooltipTrigger>
      <TooltipContent>
        {entry.api} (key {entry.apiKey}): broker{" "}
        {entry.broker
          ? `v${entry.broker[0]}–v${entry.broker[1]}`
          : "does not implement it"}
        , kaas-ui{" "}
        {entry.ours ? `v${entry.ours[0]}–v${entry.ours[1]}` : "has no schema"}
      </TooltipContent>
    </Tooltip>
  )
}

/** Look a feature up in the capability answer. */
export function featureState(
  features: FeatureEntry[] | undefined,
  feature: FeatureEntry["feature"]
): FeatureEntry | undefined {
  return features?.find((entry) => entry.feature === feature)
}
