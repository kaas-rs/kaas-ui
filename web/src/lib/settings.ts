// The settings that belong to this browser rather than to the account.
//
// Nothing here goes to the server. The theme is a `localStorage` key and the
// timezone is whatever the machine says it is, which is exactly why they are
// not on the Account page: that page answers "who is this session and what
// does it reach", and the answer is the same on every machine you sign in
// from. These are the answers that are not.
//
// The theme lives in a module-level store rather than in a component, because
// it is a *document* attribute and the document already has it before React
// exists — the inline script in `index.html` resolves it before first paint so
// a dark-mode reader never sees a white flash. This module is the continuation
// of that script, not a second opinion about it: same key, same two attributes,
// so there is no state where the two disagree.

import { useSyncExternalStore } from "react"

/** What the reader chose. `system` is a choice too — "keep following the OS". */
export type Theme = "light" | "dark" | "system"

/**
 * Which of the three date fields leads.
 *
 * `system` is the fourth choice and the default: follow the browser, the way
 * the theme follows the OS. It is not the same as storing whatever the browser
 * says today — someone who has never touched this setting should keep tracking
 * their machine, and someone who has should be immune to it.
 *
 * Three, and not one per notation. A locale decides the separators, the digits
 * and the calendar as well, and none of that is in dispute when someone says a
 * date read wrong: `9/8/2026` is either the ninth of August or
 * the eighth of September, and that is the whole question. `ydm` exists in
 * five of ICU's locales and is not offered — it resolves to `ymd`, which
 * leads with the same field.
 */
export type DateOrder = "system" | "dmy" | "ymd" | "mdy"

/** The three a formatter can actually be given. */
export type ResolvedDateOrder = Exclude<DateOrder, "system">

/** Shared with the inline script in `index.html`. Changing it changes both. */
const THEME_KEY = "kaas-ui-theme"

/** No inline script for this one: nothing paints before React reads it. */
const ORDER_KEY = "kaas-ui-date-order"

const DARK = "(prefers-color-scheme: dark)"

/** Absence means `system`, which is why choosing it *removes* the key. */
function readStored(): Theme {
  const value = localStorage.getItem(THEME_KEY)
  return value === "dark" || value === "light" ? value : "system"
}

let current: Theme = readStored()
const listeners = new Set<() => void>()

/** What `system` currently means, so the UI can say so instead of guessing. */
export function resolveTheme(theme: Theme): "light" | "dark" {
  if (theme !== "system") return theme
  return window.matchMedia(DARK).matches ? "dark" : "light"
}

/** Both attributes, always — `data-theme` for our CSS, `.dark` for shadcn's. */
function paint(theme: Theme) {
  const resolved = resolveTheme(theme)
  document.documentElement.setAttribute("data-theme", resolved)
  document.documentElement.classList.toggle("dark", resolved === "dark")
}

function setTheme(theme: Theme) {
  current = theme
  if (theme === "system") {
    localStorage.removeItem(THEME_KEY)
  } else {
    localStorage.setItem(THEME_KEY, theme)
  }
  paint(theme)
  for (const listener of listeners) listener()
}

/**
 * Start keeping the document in step with the choice.
 *
 * Called once from `main.tsx`, before the app mounts. It is not a hook and not
 * an effect: following the OS has to keep working while nobody is looking at a
 * settings page, and a listener that only exists while some component happens
 * to be mounted would stop at the first navigation.
 */
export function installSettings() {
  paint(current)

  // Following the OS is only meaningful if it keeps following it.
  window.matchMedia(DARK).addEventListener("change", () => {
    if (current === "system") paint(current)
  })

  // Another tab is the same browser, so it is the same setting. Without this
  // the two tabs disagree until one of them is reloaded. A `null` key is the
  // whole store being cleared, which is both of them.
  window.addEventListener("storage", (event) => {
    if (event.key === null || event.key === THEME_KEY) {
      current = readStored()
      paint(current)
      for (const listener of listeners) listener()
    }
    if (event.key === null || event.key === ORDER_KEY) {
      currentOrder = readStoredOrder()
      for (const listener of orderListeners) listener()
    }
  })
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function useTheme(): [Theme, (theme: Theme) => void] {
  const theme = useSyncExternalStore(subscribe, () => current)
  return [theme, setTheme]
}

/** Absence means `system`, which is why choosing it *removes* the key. */
function readStoredOrder(): DateOrder {
  const value = localStorage.getItem(ORDER_KEY)
  return value === "dmy" || value === "ymd" || value === "mdy"
    ? value
    : "system"
}

let currentOrder: DateOrder = readStoredOrder()
const orderListeners = new Set<() => void>()

function setDateOrder(order: DateOrder) {
  currentOrder = order
  if (order === "system") {
    localStorage.removeItem(ORDER_KEY)
  } else {
    localStorage.setItem(ORDER_KEY, order)
  }
  for (const listener of orderListeners) listener()
}

function subscribeOrder(listener: () => void) {
  orderListeners.add(listener)
  return () => {
    orderListeners.delete(listener)
  }
}

/**
 * What the reader picked, which may be "follow the browser".
 *
 * The pair to `useTheme`, and the settings page is the only thing that wants
 * this shape — everywhere else the question is what to *format* with, which is
 * `useResolvedDateOrder`.
 */
export function useDateOrder(): [DateOrder, (order: DateOrder) => void] {
  const order = useSyncExternalStore(subscribeOrder, () => currentOrder)
  return [order, setDateOrder]
}

/** The order to format with, resolved, and re-read when the reader changes it. */
export function useResolvedDateOrder(): ResolvedDateOrder {
  return resolveDateOrder(
    useSyncExternalStore(subscribeOrder, () => currentOrder)
  )
}

/**
 * The zone times are shown in.
 *
 * One place, so the message list, the seek picker and the settings page cannot
 * disagree about what "14:30" meant. Today it is the browser's, read fresh
 * rather than captured, because a laptop that crosses a timezone changes this
 * answer without reloading the tab.
 */
export function displayTimeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone
}

/**
 * The notation times are written in — `en-GB`, `nl-NL`, `en-US`.
 *
 * The zone's companion, and a separate question from it: a laptop in
 * Amsterdam set to `en-US` is `Europe/Amsterdam` at `8/9/2026, 11:33:41`.
 * Read fresh for the same reason, and named on the settings page for the same
 * reason — it is not a choice kaas-ui offers, so the honest thing is to say
 * where it came from.
 */
export function displayLocale(): string {
  return Intl.DateTimeFormat().resolvedOptions().locale
}

const DATE_FIELDS: Record<
  ResolvedDateOrder,
  readonly ["year" | "month" | "day", ...("year" | "month" | "day")[]]
> = {
  dmy: ["day", "month", "year"],
  ymd: ["year", "month", "day"],
  mdy: ["month", "day", "year"],
}

const IS_DATE_FIELD = new Set(["year", "month", "day"])

let ownOrder: ResolvedDateOrder | null = null

/**
 * The order this browser's own locale leads with.
 *
 * Cached, unlike the zone: a laptop crossing a timezone changes that answer
 * under a running tab, but nothing changes a browser's locale without a
 * reload.
 */
export function localeDateOrder(): ResolvedDateOrder {
  if (ownOrder) return ownOrder
  const parts = new Intl.DateTimeFormat(undefined, {
    year: "numeric",
    month: "numeric",
    day: "numeric",
  }).formatToParts(new Date(Date.UTC(2026, 7, 9)))
  const first = parts.find((part) => IS_DATE_FIELD.has(part.type))?.type
  ownOrder = first === "year" ? "ymd" : first === "month" ? "mdy" : "dmy"
  return ownOrder
}

/** What to format with, given what the reader picked. */
export function resolveDateOrder(order: DateOrder): ResolvedDateOrder {
  return order === "system" ? localeDateOrder() : order
}

/**
 * The locale's own rendering, with the three date fields put in another order.
 *
 * Reordered rather than formatted in a borrowed locale, which is the tempting
 * shortcut: asking `en-US` for a month-first date also imports its separators,
 * its decimal mark and its digits, so a Dutch reader who wanted the day moved
 * gets `8/9/2026, 09:05:03.639` — three changes they did not ask for. Here the
 * locale still decides everything except which field leads and the clock.
 *
 * The block is rewritten only when the three fields sit together with nothing
 * but literals between them, which is every locale ICU has for these options.
 * Anything else is left exactly as the locale wrote it: moving text this does
 * not understand is worse than honouring the setting.
 */
function rewrite(
  parts: Intl.DateTimeFormatPart[],
  order: ResolvedDateOrder,
  colonize: boolean
): string {
  const values = parts.map((part) => part.value)

  // The clock first, because it does not move anything: only the two literals
  // that sit between hour, minute and second are replaced.
  if (colonize) {
    parts.forEach((part, index) => {
      if (part.type !== "hour" && part.type !== "minute") return
      if (parts[index + 1]?.type === "literal") values[index + 1] = ":"
    })
  }

  const plain = () => values.join("")

  const slots = parts.flatMap((part, index) =>
    IS_DATE_FIELD.has(part.type) ? [{ index, type: part.type }] : []
  )
  const first = slots[0]
  const middle = slots[1]
  const last = slots[2]
  if (slots.length !== 3 || !first || !middle || !last) return plain()

  const between = parts.slice(first.index + 1, last.index)
  if (
    between.some(
      (part) => !IS_DATE_FIELD.has(part.type) && part.type !== "literal"
    )
  ) {
    return plain()
  }

  // Both gaps have to be the same string. Where they are not, the locale is
  // not *separating* its fields but labelling them — `2026年8月9日` says which
  // number is the year, which the month and which the day — and sliding the
  // numbers between the labels would produce a date that states something
  // false. Measured across ICU's 603 locales: exactly one, `zh-SG`, does this
  // with numeric fields, and it keeps its own order rather than a wrong one.
  const separator = values[first.index + 1] ?? ""
  if (values[middle.index + 1] !== separator) return plain()

  const value = new Map(
    slots.map((slot) => [slot.type, values[slot.index] ?? ""])
  )
  const block = DATE_FIELDS[order]
    .map((field) => value.get(field) ?? "")
    .join(separator)

  return [
    ...values.slice(0, first.index),
    block,
    ...values.slice(last.index + 1),
  ].join("")
}

/**
 * The fields. Numeric components rather than `dateStyle`/`timeStyle`, which
 * cannot say `fractionalSecondDigits` at all — and whose `short` renders a
 * two-digit year, so a record from 2026 and one from 1926 read alike.
 *
 * The clock is **ours**, and it is the one part of a rendering that is: 24
 * hours, zero-padded, `09:05:03`. A meridiem costs three characters and a
 * glance in a column whose whole job is to be scanned, and `9:05:03 PM`
 * aligns like nothing at all. Measured over ICU's 603 locales, `h23` leaves
 * none of them showing a day period.
 *
 * `hourCycle` rather than `hour12: false`, which is not the same thing: the
 * latter selects `h24` in some locales and renders midnight as `24:00:00`.
 * None of the 603 do that here.
 */
const FIELDS = {
  year: "numeric",
  month: "numeric",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hourCycle: "h23",
} as const

/**
 * Both formatters, kept until the zone changes under them.
 *
 * Building one costs about fifteen times formatting with one — 28µs against
 * 1.9µs, measured — and the millisecond one runs once per visible row of a
 * list that republishes several times a second, so a formatter built per call
 * is the most expensive thing on the row.
 */
let stamps: {
  zone: string
  toMillisecond: Intl.DateTimeFormat
  toSecond: Intl.DateTimeFormat
  dayOnly: Intl.DateTimeFormat
  /** Whether this locale separates its time fields with something else. */
  colonize: boolean
} | null = null

/**
 * Whether this locale needs its time separator replaced with a colon.
 *
 * Nineteen of ICU's 603 write `09.05.03` — Danish, Finnish, Indonesian — and
 * beside a `9.8.2026` date that is five dot-separated numbers in a row. The
 * same guard the date reorder uses applies: only when both gaps are the same
 * string, so `fr-CA`'s `09 h 05 min 03,639 s` is left alone rather than
 * half-converted into `09:05:03,639 s`.
 */
function needsColons(format: Intl.DateTimeFormat): boolean {
  const parts = format.formatToParts(new Date(Date.UTC(2026, 7, 9, 9, 5, 3)))
  const hour = parts.findIndex((part) => part.type === "hour")
  const minute = parts.findIndex((part) => part.type === "minute")
  if (hour < 0 || minute < 0) return false
  const first = parts[hour + 1]
  const second = parts[minute + 1]
  if (first?.type !== "literal" || second?.type !== "literal") return false
  return first.value === second.value && first.value !== ":"
}

function formats(timeZone: string) {
  if (stamps?.zone !== timeZone) {
    stamps = {
      zone: timeZone,
      toMillisecond: new Intl.DateTimeFormat(undefined, {
        ...FIELDS,
        fractionalSecondDigits: 3,
        timeZone,
      }),
      toSecond: new Intl.DateTimeFormat(undefined, { ...FIELDS, timeZone }),
      dayOnly: new Intl.DateTimeFormat(undefined, {
        year: "numeric",
        month: "numeric",
        day: "numeric",
        timeZone,
      }),
      colonize: false,
    }
    stamps.colonize = needsColons(stamps.toSecond)
  }
  return stamps
}

/**
 * Format, walking the parts only when something actually has to change —
 * `formatToParts` costs about twice `format`, and the common case is a locale
 * that already writes colons and a setting nobody has touched.
 */
function write(
  format: Intl.DateTimeFormat,
  value: number | Date,
  order: ResolvedDateOrder,
  colonize: boolean
): string {
  return order === localeDateOrder() && !colonize
    ? format.format(value)
    : rewrite(format.formatToParts(value), order, colonize)
}

/**
 * A record's moment, to the millisecond, in whatever notation this browser
 * asks for.
 *
 * `8/9/2026, 09:05:03.639` where that is the notation and
 * `9-8-2026, 09:05:03,639` where it is that one — the locale still decides the
 * date separators, the digits and the decimal mark. What it does not decide is
 * the clock, which is 24 hours everywhere, or the field order, which is a
 * setting. The one thing this insists on is the millisecond: two records
 * a few hundred microseconds apart are a normal thing to be looking at in a
 * Kafka UI, and a timestamp that renders them identically cannot answer which
 * came first.
 *
 * The zone is a parameter rather than read here because reading it is itself a
 * formatter construction: callers read it once per render and hand it down.
 */
export function formatTimestamp(
  value: number | Date,
  timeZone: string,
  order: ResolvedDateOrder
): string {
  const { toMillisecond, colonize } = formats(timeZone)
  return write(toMillisecond, value, order, colonize)
}

/**
 * The same notation, to the second — for a clock rather than for a record.
 *
 * A millisecond is what tells two records apart. On something that ticks it is
 * three digits of noise, and on something that ticks once a second it is three
 * digits that look stuck.
 */
export function formatClock(
  value: number | Date,
  timeZone: string,
  order: ResolvedDateOrder
): string {
  const { toSecond, colonize } = formats(timeZone)
  return write(toSecond, value, order, colonize)
}

/**
 * A day on its own, for somewhere a time would be noise.
 *
 * The seek picker's button is the one such place: it names the day being
 * chosen, next to its own time input. It used to spell the month, to be
 * unambiguous about a date nobody had told the app how to read — which is
 * exactly the question the date-order setting now answers, so it is numeric
 * like every other date here. It also has to be: a spelled month puts a
 * per-field marker in 158 of ICU's locales rather than one separator, and
 * those are the ones the reorder above declines to touch.
 */
export function formatDate(
  value: number | Date,
  timeZone: string,
  order: ResolvedDateOrder
): string {
  // No time fields, so nothing to colonise.
  return write(formats(timeZone).dayOnly, value, order, false)
}
