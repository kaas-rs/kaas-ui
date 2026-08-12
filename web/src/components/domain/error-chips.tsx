import { AlertTriangle } from "lucide-react"

import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import type { ResourceError } from "@/api/types"

/** `ErrorCode::Unknown(30000)` — the number is the only searchable thing. */
export function UnknownCodeChip({ code }: { code: number }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className="rounded-sm px-1.5 py-0.5 font-mono text-[12px]"
          style={{ background: "var(--warn-soft)", color: "var(--warn-ink)" }}
        >
          code {code}
        </span>
      </TooltipTrigger>
      <TooltipContent>
        this build has no name for this error code
      </TooltipContent>
    </Tooltip>
  )
}

/**
 * The per-resource failures that rode along with a successful request.
 *
 * These are data, not a failed request: the page renders, and these say which
 * parts of it did not.
 */
export function ErrorChips({ errors }: { errors: ResourceError[] }) {
  if (errors.length === 0) return null
  return (
    <div className="mb-4 flex flex-wrap gap-2">
      {errors.map((error, index) => (
        <Tooltip key={`${error.resource}-${index}`}>
          <TooltipTrigger asChild>
            <span
              className="inline-flex items-center gap-2 rounded-sm border px-2 py-1 text-[12px]"
              style={{
                background: "var(--warn-soft)",
                color: "var(--warn-ink)",
                borderColor: "var(--rust-edge)",
              }}
            >
              <AlertTriangle aria-hidden className="size-3.5" />
              <span className="font-mono">{error.resource}</span>
              {error.code ? (
                <span className="font-mono opacity-80">{error.code}</span>
              ) : error.codeNumber !== null ? (
                <span className="font-mono opacity-80">
                  code {error.codeNumber}
                </span>
              ) : (
                <span className="opacity-80">{error.kind}</span>
              )}
            </span>
          </TooltipTrigger>
          <TooltipContent className="max-w-md">{error.message}</TooltipContent>
        </Tooltip>
      ))}
    </div>
  )
}
