// The toolbar. Driven entirely by SEEK_MODES — no component below here asks
// which mode is selected.

import { useEffect, useState } from "react"
import { RotateCw } from "lucide-react"

import type { PartitionOffsets } from "@/api/types"
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
  onRestart(): void
}

export function Toolbar({
  mode,
  offset,
  timestamp,
  filter,
  partitions,
  visibility,
  bounds,
  retentionStart,
  timeZone,
  onApply,
  onFilterChange,
  onPartitionsChange,
  onVisibilityChange,
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

function min(values: (number | null)[]): number | null {
  const present = values.filter((value): value is number => value !== null)
  return present.length ? Math.min(...present) : null
}

function max(values: (number | null)[]): number | null {
  const present = values.filter((value): value is number => value !== null)
  return present.length ? Math.max(...present) : null
}
