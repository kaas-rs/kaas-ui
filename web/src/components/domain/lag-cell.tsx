import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { count } from "@/lib/format"
import type { Lag } from "@/api/types"

/** Three states that must not all render as `0`, plus "not known". */
export function LagCell({ lag }: { lag: Lag }) {
  const render = () => {
    switch (lag.state) {
      case "noCommit":
        return {
          text: "—",
          className: "text-ink-faint",
          why: "the group has never committed here",
        }
      case "emptyPartition":
        return {
          text: "∅",
          className: "text-ink-faint",
          why: "the partition is empty",
        }
      case "caughtUp":
        return {
          text: "0",
          className: "font-medium text-ok",
          why: "committed at the log end",
        }
      case "lagging":
        return {
          text: count(lag.records),
          className: "font-mono text-warn-ink",
          why: "records behind the log end",
        }
      case "unknown":
        return {
          text: "?",
          className: "text-ink-faint",
          why: "the log end could not be read",
        }
    }
  }

  const { text, className, why } = render()
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className={className}>{text}</span>
      </TooltipTrigger>
      <TooltipContent>{why}</TooltipContent>
    </Tooltip>
  )
}
