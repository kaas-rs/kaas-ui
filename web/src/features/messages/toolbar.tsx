// The toolbar. Driven entirely by SEEK_MODES — no component below here asks
// which mode is selected.

import { useEffect, useState } from "react"
import { RotateCw } from "lucide-react"

import type { Codec, PartitionOffsets } from "@/api/types"
import { Button } from "@/components/ui/button"
import { DateTimePicker } from "@/components/date-time-picker"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { SEEK_GROUPS, SEEK_MODES, type SeekMode } from "./seek-modes"

export interface ToolbarProps {
  mode: SeekMode
  offset?: number
  timestamp?: number
  filter?: string
  partitions?: string
  visibility: "all" | "committed"
  /**
   * How keys and values are read, where this view overrode the configuration.
   *
   * Only the four that need no schema: falling *back* to hex or string is
   * always possible, and asking for Avro cannot invent a schema id.
   */
  keyCodec?: ChoosableCodec
  valueCodec?: ChoosableCodec
  /** A JavaScript expression over the decoded value. */
  predicate?: string
  /** Both ends of every partition, for clamping. */
  bounds: PartitionOffsets[]
  /** The oldest record the topic still holds, bounding the calendar. */
  retentionStart?: Date
  /** The zone the app displays times in. */
  timeZone: string
  onApply(next: { mode: SeekMode; offset?: number; timestamp?: number }): void
  onFilterChange(filter: string | undefined): void
  onPartitionsChange(partitions: string | undefined): void
  onVisibilityChange(visibility: "all" | "committed"): void
  onCodecChange(next: {
    keyCodec?: ChoosableCodec
    valueCodec?: ChoosableCodec
  }): void
  onPredicateChange(predicate: string | undefined): void
  onRestart(): void
}

/** The codecs a reader may pick. See `Codec` for why the other three are not. */
export type ChoosableCodec = Extract<Codec, "auto" | "string" | "hex" | "json">

const CODEC_LABELS: Record<ChoosableCodec, string> = {
  auto: "Auto",
  string: "String",
  hex: "Hex",
  json: "JSON",
}

export function Toolbar({
  mode,
  offset,
  timestamp,
  filter,
  partitions,
  visibility,
  keyCodec,
  valueCodec,
  predicate,
  bounds,
  retentionStart,
  timeZone,
  onApply,
  onFilterChange,
  onPartitionsChange,
  onVisibilityChange,
  onCodecChange,
  onPredicateChange,
  onRestart,
}: ToolbarProps) {
  const config = SEEK_MODES[mode]

  // Held locally until Apply, so typing an offset does not tear down and
  // reopen the stream on every keystroke.
  const [draftMode, setDraftMode] = useState<SeekMode>(mode)
  const [draftOffset, setDraftOffset] = useState<string>(
    offset?.toString() ?? ""
  )
  const [draftInstant, setDraftInstant] = useState<Date | undefined>(
    timestamp !== undefined ? new Date(timestamp) : undefined
  )
  const [draftFilter, setDraftFilter] = useState(filter ?? "")
  const [draftPredicate, setDraftPredicate] = useState(predicate ?? "")

  useEffect(() => setDraftMode(mode), [mode])
  useEffect(() => setDraftOffset(offset?.toString() ?? ""), [offset])
  useEffect(
    () =>
      setDraftInstant(
        timestamp !== undefined ? new Date(timestamp) : undefined
      ),
    [timestamp]
  )
  useEffect(() => setDraftFilter(filter ?? ""), [filter])
  useEffect(() => setDraftPredicate(predicate ?? ""), [predicate])

  const draftConfig = SEEK_MODES[draftMode]

  // Partitions that failed leave the control unclamped rather than blocking
  // it: a partition mid-election must not stop someone seeking in the fifteen
  // that are fine.
  const earliest = min(bounds.map((partition) => partition.earliestOffset))
  const latest = max(bounds.map((partition) => partition.latestOffset))

  function apply() {
    const parsedOffset =
      draftOffset.trim() === "" ? undefined : Number(draftOffset)
    onApply({
      mode: draftMode,
      offset:
        draftConfig.input === "offset" && Number.isFinite(parsedOffset)
          ? parsedOffset
          : undefined,
      timestamp:
        draftConfig.input === "datetime" && draftInstant
          ? draftInstant.getTime()
          : undefined,
    })
  }

  const incomplete =
    (draftConfig.input === "offset" && draftOffset.trim() === "") ||
    (draftConfig.input === "datetime" && !draftInstant)

  return (
    <div className="flex flex-wrap items-center gap-2 border-b border-line px-4 py-2">
      <Select
        value={draftMode}
        onValueChange={(value) => setDraftMode(value as SeekMode)}
      >
        <SelectTrigger className="w-[150px]" aria-label="Seek mode">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {SEEK_GROUPS.map((group) => (
            <SelectGroup key={group}>
              <SelectLabel>{group}</SelectLabel>
              {(
                Object.entries(SEEK_MODES) as [
                  SeekMode,
                  (typeof SEEK_MODES)[SeekMode],
                ][]
              )
                .filter(([, entry]) => entry.group === group)
                .map(([key, entry]) => (
                  <SelectItem key={key} value={key}>
                    {entry.label}
                  </SelectItem>
                ))}
            </SelectGroup>
          ))}
        </SelectContent>
      </Select>

      {draftConfig.input === "offset" ? (
        <Input
          type="number"
          inputMode="numeric"
          value={draftOffset}
          min={earliest ?? undefined}
          max={latest ?? undefined}
          onChange={(event) => setDraftOffset(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !incomplete) apply()
          }}
          className="w-[150px] tabular-nums"
          aria-label="Offset"
          placeholder={
            earliest !== null && latest !== null
              ? `${earliest}–${latest}`
              : "offset"
          }
        />
      ) : null}

      {draftConfig.input === "datetime" ? (
        <DateTimePicker
          value={draftInstant}
          onChange={setDraftInstant}
          timeZone={timeZone}
          disabled={{ before: retentionStart, after: new Date() }}
        />
      ) : null}

      {/* Default size, not `sm`: every control on this row is `h-9` — the
          selects, the inputs, the date picker — and a button a notch shorter
          than its neighbours reads as a different kind of thing. */}
      {draftConfig.input !== "none" ? (
        <Button onClick={apply} disabled={incomplete}>
          Apply
        </Button>
      ) : draftMode !== mode ? (
        <Button onClick={apply}>Apply</Button>
      ) : null}

      <span className="text-[11px] text-ink-faint">
        {SEEK_MODES[draftMode].hint}
      </span>

      <span className="flex-1" />

      <Input
        value={draftFilter}
        placeholder="Filter payload…"
        aria-label="Filter payload"
        className="w-[200px]"
        onChange={(event) => setDraftFilter(event.target.value)}
        onKeyDown={(event) => {
          // Applied on Enter, not on change. The filter runs in the Rust
          // process — see below — so every keystroke would reopen the stream.
          if (event.key === "Enter")
            onFilterChange(draftFilter.trim() || undefined)
        }}
        onBlur={() => onFilterChange(draftFilter.trim() || undefined)}
      />

      {/* The second tier, and it says so. `filter` above is a substring match
          the server applies before a record is deserialised; this one runs on
          the decoded value in a sandbox, after it. Applied on Enter or blur
          for the same reason the substring filter is: it reopens the stream. */}
      <Input
        value={draftPredicate}
        placeholder="v =&gt; v.amount &gt; 100"
        aria-label="Filter expression (JavaScript)"
        title="A JavaScript expression over the decoded value. Runs after the cheap filters, in a sandbox with a memory cap and a per-record time budget."
        className="w-[220px] font-mono text-[11px]"
        spellCheck={false}
        onChange={(event) => setDraftPredicate(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter")
            onPredicateChange(draftPredicate.trim() || undefined)
        }}
        onBlur={() => onPredicateChange(draftPredicate.trim() || undefined)}
      />

      <CodecSelect
        label="Key codec"
        value={keyCodec}
        onChange={(next) => onCodecChange({ keyCodec: next })}
      />
      <CodecSelect
        label="Value codec"
        value={valueCodec}
        onChange={(next) => onCodecChange({ valueCodec: next })}
      />

      {/* `committed` is read_committed: records from transactions that were
          aborted disappear, and the log's usable end becomes the last stable
          offset rather than the high watermark. On a topic nobody writes
          transactionally the two are identical, which is why the default is
          `all` — hiding nothing, and saying so. */}
      <Select
        value={visibility}
        onValueChange={(value) =>
          onVisibilityChange(value as "all" | "committed")
        }
      >
        <SelectTrigger
          className="w-[130px]"
          aria-label="Transaction visibility"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">All records</SelectItem>
          <SelectItem value="committed">Committed only</SelectItem>
        </SelectContent>
      </Select>

      <Input
        value={partitions ?? ""}
        placeholder="Partitions"
        aria-label="Partitions"
        className="w-[110px] tabular-nums"
        onChange={(event) =>
          onPartitionsChange(event.target.value.trim() || undefined)
        }
      />

      {/* For the bounded modes, and only those. A window is a snapshot with no
          way to age visibly, so re-reading it has to be something you can ask
          for — it used to happen by accident, when the finished stream
          reconnected. A live tail is already the answer to that question, and
          a button offering to restart what has not stopped is a button that
          only ever loses your place. */}
      {config.live ? null : (
        <Button
          size="icon"
          variant="outline"
          onClick={onRestart}
          aria-label="Read this window again"
          title="Read this window again"
        >
          <RotateCw className="size-3.5" aria-hidden />
        </Button>
      )}
    </div>
  )
}

/**
 * The override control, as a select.
 *
 * "Auto" is the absence of an override rather than a value: it is what the
 * per-topic configuration and the framing decide between, and pinning it in
 * the URL would make a link outlive a configuration change it should have
 * followed.
 */
function CodecSelect({
  label,
  value,
  onChange,
}: {
  label: string
  value?: ChoosableCodec
  onChange(next: ChoosableCodec | undefined): void
}) {
  return (
    <Select
      value={value ?? "auto"}
      onValueChange={(next) =>
        onChange(next === "auto" ? undefined : (next as ChoosableCodec))
      }
    >
      <SelectTrigger className="w-[110px]" aria-label={label} title={label}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          <SelectLabel>{label}</SelectLabel>
          {(Object.keys(CODEC_LABELS) as ChoosableCodec[]).map((codec) => (
            <SelectItem key={codec} value={codec}>
              {CODEC_LABELS[codec]}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

function min(values: (number | null)[]): number | null {
  const present = values.filter((value): value is number => value !== null)
  return present.length ? Math.min(...present) : null
}

function max(values: (number | null)[]): number | null {
  const present = values.filter((value): value is number => value !== null)
  return present.length ? Math.max(...present) : null
}
