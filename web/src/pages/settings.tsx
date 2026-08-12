// How this browser displays kaas-ui.
//
// The sibling of the Account page, and the split between them is the useful
// part: Account answers "who is this session and what does it reach", which is
// the same answer from every machine you sign in from. Settings answers the
// questions that are *not* — they live in this browser's `localStorage`, they
// are not sent anywhere, and signing in somewhere else does not bring them
// along. The page says so, because a settings page that looks account-shaped
// invites the assumption that it syncs.
//
// Timezone and notation are shown rather than chosen: they are the browser's,
// and naming them beats an inert dropdown with one option in it. Two rows
// rather than one because they are two questions — a laptop in Amsterdam set
// to `en-US` is `Europe/Amsterdam` at `8/9/2026, 11:33:41`.
//
// Date order is the exception, and it is chosen, because it is the one part of
// a notation that changes what a date *means* rather than how it looks. It
// still defaults to the browser, so this page is the only place the difference
// between "the browser says so" and "I said so" is visible.

import { useEffect, useState, type ReactNode } from "react"
import { Clock, ExternalLinkIcon, Monitor, Moon, Sun } from "lucide-react"
import { formatInTimeZone } from "date-fns-tz"

import { withBase } from "@/api/base"
import { Empty, Mono, Section } from "@/components/domain"
import { Button } from "@/components/ui/button"
import { Item, ItemActions, ItemContent, ItemTitle } from "@/components/ui/item"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  displayLocale,
  displayTimeZone,
  formatClock,
  formatDate,
  localeDateOrder,
  resolveTheme,
  useDateOrder,
  useResolvedDateOrder,
  useTheme,
  type DateOrder,
  type ResolvedDateOrder,
  type Theme,
} from "@/lib/settings"
import { cn } from "@/lib/utils"
import { PageTitle } from "@/components/page-title"

/** One row of a settings block: what it is, what it says, why it matters. */
function Setting({
  label,
  note,
  children,
}: {
  label: string
  note?: string
  children: ReactNode
}) {
  return (
    <div className="border-line flex flex-wrap items-center justify-between gap-x-6 gap-y-3 border-b py-4 last:border-0">
      <div className="min-w-0">
        <div className="text-[13px] font-medium">{label}</div>
        {note ? (
          <div className="text-ink-muted mt-0.5 text-[12px]">{note}</div>
        ) : null}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  )
}

const THEMES: { value: Theme; label: string; icon: typeof Sun }[] = [
  { value: "system", label: "system", icon: Monitor },
  { value: "light", label: "light", icon: Sun },
  { value: "dark", label: "dark", icon: Moon },
]

/**
 * Three states, not a toggle.
 *
 * "Follow the OS" is a different thing from "light", and a two-position switch
 * cannot express it — it would silently pin whichever mode the machine was in
 * when someone first touched it, and stop following at dusk.
 */
function ThemePicker() {
  const [theme, setTheme] = useTheme()

  return (
    <div
      role="radiogroup"
      aria-label="Theme"
      className="border-line bg-surface-sunken inline-flex gap-0.5 rounded-md border p-0.5"
    >
      {THEMES.map((option) => {
        const active = option.value === theme
        return (
          <Button
            key={option.value}
            role="radio"
            aria-checked={active}
            variant="ghost"
            size="sm"
            onClick={() => setTheme(option.value)}
            className={cn(
              "px-3",
              active && "bg-surface-raised text-ink shadow-xs",
              !active && "text-ink-muted"
            )}
          >
            <option.icon aria-hidden />
            {option.label}
          </Button>
        )
      })}
    </div>
  )
}

const ORDERS: { value: ResolvedDateOrder; label: string }[] = [
  { value: "dmy", label: "day first" },
  { value: "ymd", label: "year first" },
  { value: "mdy", label: "month first" },
]

/**
 * Which field leads, and the sample that proves it.
 *
 * Four options, not one per notation: a locale settles the separators and the
 * digits too, and none of that is what makes a date misread. `9/8/2026` is
 * either the ninth of August or the eighth of September, and that is the whole
 * question this answers.
 *
 * Each option carries today's date written its way, because the labels alone
 * are the wrong end of it — "day first" is a description, `9/8/2026` is the
 * thing you will actually be looking at.
 */
function DateOrderPicker({ timeZone }: { timeZone: string }) {
  const [order, setOrder] = useDateOrder()
  const sample = new Date()

  return (
    <Select value={order} onValueChange={(next) => setOrder(next as DateOrder)}>
      <SelectTrigger className="w-[210px]" aria-label="Date order">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="system">
          from browser
          <span className="text-ink-faint">
            {formatDate(sample, timeZone, localeDateOrder())}
          </span>
        </SelectItem>
        {ORDERS.map((option) => (
          <SelectItem key={option.value} value={option.value}>
            {option.label}
            <span className="text-ink-faint">
              {formatDate(sample, timeZone, option.value)}
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

/**
 * A clock in the zone and notation above, so both can be checked against a
 * wrist.
 *
 * The same two settings a record's timestamp is written with, so this is also
 * the sample — the field order and the separators here are what a message list
 * will show. To the second and not beyond: a record
 * carries milliseconds because they are what tells two records apart, and a
 * clock ticking once a second would only show three digits that look stuck.
 */
function Now({ timeZone }: { timeZone: string }) {
  const order = useResolvedDateOrder()
  const [now, setNow] = useState(() => new Date())

  useEffect(() => {
    const timer = setInterval(() => setNow(new Date()), 1000)
    return () => clearInterval(timer)
  }, [])

  return (
    <span className="inline-flex items-center gap-2 font-mono text-[13px]">
      <Clock aria-hidden className="text-ink-faint size-3.5" />
      {formatClock(now, timeZone, order)}
      <span className="text-ink-faint">
        UTC{formatInTimeZone(now, timeZone, "xxx")}
      </span>
    </span>
  )
}

export function SettingsPage() {
  const [theme] = useTheme()
  const timeZone = displayTimeZone()
  const locale = displayLocale()

  return (
    <div className="max-w-3xl">
      <PageTitle
        title="Settings"
        subtitle="How this browser displays kaas-ui, and where to find what it is built on."
      />

      <Section title="Appearance">
        <div className="rounded-md border px-4 py-1">
          <Setting
            label="Theme"
            note={
              theme === "system"
                ? `following the operating system, which is ${resolveTheme("system")} right now`
                : `pinned to ${theme}, whatever the operating system does`
            }
          >
            <ThemePicker />
          </Setting>
        </div>
      </Section>

      <Section title="Time">
        <div className="rounded-md border px-4 py-1">
          <Setting
            label="Timezone"
            note="from this browser — every timestamp in kaas-ui is rendered in it, and the message browser's seek picker reads times in it"
          >
            <Mono>{timeZone}</Mono>
          </Setting>
          {/* A separate question from the zone, and not a redundant one: a
              laptop in Amsterdam set to `en-US` is `Europe/Amsterdam` at
              `8/9/2026, 09:05:03`. Shown rather than chosen, like the zone. */}
          <Setting
            label="Notation"
            note="from this browser — it decides the separators, the digits and the decimal mark. The clock is not its call: 24 hours everywhere"
          >
            <Mono>{locale}</Mono>
          </Setting>
          <Setting
            label="Date order"
            note="which of the three fields leads — the one thing a notation decides that changes what a date means"
          >
            <DateOrderPicker timeZone={timeZone} />
          </Setting>
          <Setting
            label="Local time"
            note="right now, in the zone and notation above — a record's timestamp is written the same way, and carries milliseconds as well"
          >
            <Now timeZone={timeZone} />
          </Setting>
        </div>
        <p className="text-ink-muted mt-3 text-[12px]">
          Brokers deal in epoch milliseconds and so does the wire format, so the
          zone is applied once, on the way to the screen. It is not a filter on
          what a cluster returns.
        </p>
      </Section>

      {/* Not a setting, and the heading says so rather than pretending
          otherwise. It used to sit in the top bar of every page, which gave a
          document you read once the same weight as the navigation. */}
      <Section title="Reference">
        {/* `asChild`, not `render` — the Item in this registry is the Radix
            build, so the anchor is passed as the child and `Slot.Root` merges
            the props onto it. No wrapping bordered div either: `outline` is
            the border, and keeping both draws two. */}
        <Item variant="outline" asChild>
          <a
            href={withBase("/api/openapi.json")}
            target="_blank"
            rel="noopener noreferrer"
          >
            <ItemContent>
              <ItemTitle>OpenAPI</ItemTitle>
            </ItemContent>
            <ItemActions>
              <ExternalLinkIcon className="size-4" />
            </ItemActions>
          </a>
        </Item>
      </Section>

      {/* Named "your settings" rather than "these" because Reference now sits
          directly above it: "these" would sweep in the API description, which
          is served by the binary and would make "not on the server" read as a
          contradiction. */}
      <Section title="Where your settings live">
        <Empty>
          Your theme choice lives in this browser's local storage — not in your
          account, and not on the server. Another browser, or this one after its
          site data is cleared, starts from the defaults again.
        </Empty>
      </Section>
    </div>
  )
}
