// How a payload says what it is.
//
// Auto-detection that cannot be seen is worse than none: the reader has to be
// able to tell text the producer wrote from kaas-ui's guess, and a schema the
// registry resolved from either. So every rendering carries the codec that
// produced it, and — where a registry was involved — which registry, which
// subject and which id.
//
// The chip is the *override control* rather than a label. It is in the
// toolbar rather than on each row, because the override is per view and
// belongs in the URL: a link to "this topic, read as hex" has to survive being
// sent to somebody.

import { useState } from "react"
import { AlertTriangle, Info } from "lucide-react"

import type { NoteKind, Payload, PayloadNote } from "@/api/types"
import { Badge } from "@/components/ui/badge"
import { bytes } from "@/lib/format"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"

/**
 * The one-line summary of how a payload was read.
 *
 * `auto` is deliberately silent: it is the absence of a decision, and a badge
 * on every row saying "we guessed" would be noise on the topics where there is
 * nothing to say. Everything else — a forced codec, a resolved schema, a note
 * — is worth a mark.
 */
export function CodecChip({
  payload,
  className,
}: {
  payload: Payload
  className?: string
}) {
  const schema = payload.schema
  const note = payload.note
  if (payload.codec === "auto" && !schema && !note) return null

  const label = schema
    ? `${payload.codec} #${schema.id}`
    : payload.codec === "auto"
      ? payload.encoding
      : payload.codec

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge
          variant="outline"
          className={cn(
            "h-4 shrink-0 px-1 font-mono text-[10px] font-normal",
            note ? noteTone(note.kind) : undefined,
            className
          )}
        >
          {label}
        </Badge>
      </TooltipTrigger>
      <TooltipContent className="max-w-[420px]">
        <div className="space-y-1 text-xs">
          <p>
            Read as <span className="font-mono">{payload.codec}</span>
            {payload.truncated ? ", truncated" : ""} — {bytes(payload.bytes)}
          </p>
          {schema ? (
            <p className="text-[11px]">
              schema <span className="font-mono">{schema.id}</span> (
              {schema.format}) from registry{" "}
              <span className="font-mono">{schema.registry}</span>
              {schema.subject ? (
                <>
                  , subject <span className="font-mono">{schema.subject}</span>
                  {schema.version ? ` v${schema.version}` : ""}
                </>
              ) : null}
              {schema.name ? (
                <>
                  {" — "}
                  <span className="font-mono">{schema.name}</span>
                </>
              ) : null}
            </p>
          ) : null}
          {note ? <p className="text-[11px]">{note.message}</p> : null}
        </div>
      </TooltipContent>
    </Tooltip>
  )
}

/**
 * A note about why a payload is not what was asked for.
 *
 * Six kinds, rendered differently on purpose. A registry that is *down* heals
 * on its own and is a warning; a registry answering the wrong API is a line in
 * a configuration file and does not heal, so it is an error however calmly the
 * page around it is behaving.
 */
export function PayloadNoteLine({ note }: { note: PayloadNote }) {
  const loud =
    note.kind === "decodeError" || note.kind === "registryMisconfigured"
  const Icon = loud ? AlertTriangle : Info
  return (
    <p
      className={cn(
        "flex items-start gap-2 text-[11px]",
        loud ? "text-danger" : "text-warn-ink"
      )}
    >
      <Icon className="mt-0.5 size-3.5 shrink-0" aria-hidden />
      <span>
        <span className="font-medium">{NOTE_TITLES[note.kind]}. </span>
        {note.message}
      </span>
    </p>
  )
}

const NOTE_TITLES: Record<NoteKind, string> = {
  decodeError: "This payload did not decode",
  registryUnavailable: "The schema registry could not be reached",
  registryAbsent: "No schema registry is configured for this cluster",
  registryMisconfigured: "The schema registry is misconfigured",
  overrideRefused: "That codec could not be used",
  nonConforming: "This record does not match its schema",
}

function noteTone(kind: NoteKind): string {
  switch (kind) {
    case "decodeError":
    case "registryMisconfigured":
      return "border-danger/50 text-danger"
    default:
      return "border-warn-ink/50 text-warn-ink"
  }
}

/** What the detail panel can show a payload as, without asking the server. */
type View = "decoded" | "hex" | "string"

/**
 * One payload, whole, with the local views of it.
 *
 * The raw bytes travel beside the decoded value precisely so that dropping to
 * hex or string is a render and not a refetch. The other direction is not
 * offered here at all: nothing in a browser can invent a schema id, so
 * "read this as Avro" is a toolbar decision the server has to answer.
 */
export function PayloadBlock({
  payload,
  label,
}: {
  payload: Payload
  label?: string
}) {
  const [view, setView] = useState<View>("decoded")
  const hex =
    payload.raw?.hex ?? (payload.encoding === "hex" ? payload.text : null)
  const shown = render(payload, view, hex)

  return (
    <div className="space-y-1 py-2">
      <div className="flex flex-wrap items-center gap-2 text-[11px] text-ink-faint">
        {label ? <span>{label}</span> : null}
        {/* What produced the text is said out loud, always. */}
        <Badge variant="outline" className="h-4 px-1 font-mono text-[10px]">
          {payload.codec}
        </Badge>
        <Badge variant="outline" className="h-4 px-1 text-[10px]">
          {payload.encoding}
        </Badge>
        <span className="tabular-nums">{bytes(payload.bytes)}</span>
        {payload.truncated ? (
          <span className="text-warn-ink">truncated</span>
        ) : null}
        {payload.schema ? (
          <span className="font-mono">
            {payload.schema.subject ?? `schema ${payload.schema.id}`}
            {payload.schema.version ? ` v${payload.schema.version}` : ""}
            {" @ "}
            {payload.schema.registry}
          </span>
        ) : null}
        <span className="flex-1" />
        {hex ? (
          <Tabs value={view} onValueChange={(next) => setView(next as View)}>
            <TabsList className="p-[2px] group-data-[orientation=horizontal]/tabs:h-6">
              {(["decoded", "string", "hex"] as View[]).map((option) => (
                <TabsTrigger
                  key={option}
                  value={option}
                  className="px-1.5 py-0 text-[10px]"
                >
                  {option}
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
        ) : null}
      </div>
      {payload.note ? <PayloadNoteLine note={payload.note} /> : null}
      <pre className="max-h-full overflow-auto rounded-md border border-line bg-surface-sunken p-3 font-mono text-[11px] leading-relaxed break-all whitespace-pre-wrap">
        {shown}
      </pre>
    </div>
  )
}

function render(payload: Payload, view: View, hex: string | null): string {
  if (view === "decoded" || !hex) return payload.text
  if (view === "hex") return hex
  return utf8(hex)
}

/** Hex back to text, replacing what is not. Enough for a preview, not a codec. */
function utf8(hex: string): string {
  const bytes = new Uint8Array(Math.floor(hex.length / 2))
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = Number.parseInt(hex.slice(index * 2, index * 2 + 2), 16)
  }
  try {
    return new TextDecoder("utf-8", { fatal: false }).decode(bytes)
  } catch {
    return hex
  }
}
