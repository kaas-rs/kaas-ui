// A date and a time, in the timezone the app displays.
//
// shadcn's own composition — Popover + Calendar + a native `input type="time"`
// — rather than a datetime package. The native input gives keyboard entry,
// locale-correct 12/24h display and a real picker on mobile for nothing.
//
// The one rule that matters: this component is the **only** place a timezone
// is applied. Everything downstream of `onChange` is an absolute instant, and
// the wire format is epoch milliseconds, which is also what `ListOffsets`
// takes. A second conversion anywhere between here and the broker is how
// "14:30" becomes 12:30 in the list and nobody can say which layer did it.

import * as React from "react"
import { CalendarIcon } from "lucide-react"
import { set } from "date-fns"
import { formatInTimeZone, fromZonedTime, toZonedTime } from "date-fns-tz"

import { formatDate, useResolvedDateOrder } from "@/lib/settings"
import type { Matcher } from "react-day-picker"

import { Button } from "@/components/ui/button"
import { Calendar } from "@/components/ui/calendar"
import { Input } from "@/components/ui/input"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { cn } from "@/lib/utils"

export interface DateTimePickerProps {
  value: Date | undefined
  onChange(next: Date): void
  /** The zone shown in the app header, not the browser's. */
  timeZone: string
  /** Days outside retention, so a reader cannot seek to a time with no data. */
  disabled?: { before?: Date; after?: Date }
  className?: string
}

/** The presets that cover most of what anyone actually types. */
const PRESETS: { label: string; at(): Date }[] = [
  { label: "15 min ago", at: () => new Date(Date.now() - 15 * 60_000) },
  { label: "1 hour ago", at: () => new Date(Date.now() - 60 * 60_000) },
  { label: "6 hours ago", at: () => new Date(Date.now() - 6 * 60 * 60_000) },
]

export function DateTimePicker({
  value,
  onChange,
  timeZone,
  disabled,
  className,
}: DateTimePickerProps) {
  const [open, setOpen] = React.useState(false)
  const dateOrder = useResolvedDateOrder()

  // `step="1"` and seconds throughout: Kafka offsets move fast enough that
  // minute granularity is useless for the debugging this exists for.
  const time = value
    ? formatInTimeZone(value, timeZone, "HH:mm:ss")
    : "00:00:00"

  function applyDay(day: Date | undefined) {
    if (!day) return
    const [hours = 0, minutes = 0, seconds = 0] = time.split(":").map(Number)
    onChange(fromZonedTime(set(day, { hours, minutes, seconds }), timeZone))
    setOpen(false)
  }

  function applyTime(raw: string) {
    const [hours, minutes, seconds = 0] = raw.split(":").map(Number)
    if (!Number.isFinite(hours) || !Number.isFinite(minutes)) return
    const zoned = toZonedTime(value ?? new Date(), timeZone)
    onChange(fromZonedTime(set(zoned, { hours, minutes, seconds }), timeZone))
  }

  // `Calendar` selects in the display zone, so the *day* it is given has to be
  // the day as seen there — otherwise 00:30 UTC+02:00 highlights yesterday.
  const selectedDay = value ? toZonedTime(value, timeZone) : undefined

  // Two separate matchers rather than one interval: `DateInterval` requires
  // both ends, and retention routinely bounds only one — a topic that has
  // never expired a segment has no lower bound to offer.
  const matcher = React.useMemo(() => {
    const rules: Matcher[] = []
    if (disabled?.before)
      rules.push({ before: toZonedTime(disabled.before, timeZone) })
    if (disabled?.after)
      rules.push({ after: toZonedTime(disabled.after, timeZone) })
    return rules.length ? rules : undefined
  }, [disabled?.before, disabled?.after, timeZone])

  return (
    <div className={cn("flex items-center gap-2", className)}>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            variant="outline"
            className="w-[168px] justify-start font-normal"
            aria-label="Date"
          >
            <CalendarIcon className="mr-2 size-4" />
            {value ? formatDate(value, timeZone, dateOrder) : "Pick a date"}
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-auto p-0" align="start">
          <Calendar
            mode="single"
            selected={selectedDay}
            onSelect={applyDay}
            timeZone={timeZone}
            disabled={matcher}
            captionLayout="dropdown"
            autoFocus
          />
          <div className="flex flex-wrap gap-1 border-t border-line p-2">
            {PRESETS.map((preset) => (
              <Button
                key={preset.label}
                variant="ghost"
                size="sm"
                className="h-7 text-xs"
                onClick={() => {
                  onChange(preset.at())
                  setOpen(false)
                }}
              >
                {preset.label}
              </Button>
            ))}
          </div>
        </PopoverContent>
      </Popover>
      <Input
        type="time"
        step="1"
        value={time}
        aria-label="Time"
        // No `onKeyDown` handling: intercepting keystrokes is what breaks
        // typing a time into a native time input.
        onChange={(event) => applyTime(event.target.value)}
        className="w-[118px] tabular-nums"
      />
    </div>
  )
}
