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
// Timezone is shown rather than chosen. It is the browser's, it is what every
// timestamp in the app is rendered in, and until there is a reason to override
// it the honest thing is to name it and say where it came from — an inert
// dropdown with one option in it would be worse.

import { useEffect, useState, type ReactNode } from "react"
import { Clock, Monitor, Moon, Sun } from "lucide-react"
import { formatInTimeZone } from "date-fns-tz"

import { Empty, Mono, Section } from "@/components/domain"
import { Button } from "@/components/ui/button"
import {
  displayTimeZone,
  resolveTheme,
  useTheme,
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

/** A clock in the zone above, so the answer can be checked against a wrist. */
function Now({ timeZone }: { timeZone: string }) {
  const [now, setNow] = useState(() => new Date())

  useEffect(() => {
    const timer = setInterval(() => setNow(new Date()), 1000)
    return () => clearInterval(timer)
  }, [])

  return (
    <span className="inline-flex items-center gap-2 font-mono text-[13px]">
      <Clock aria-hidden className="text-ink-faint size-3.5" />
      {formatInTimeZone(now, timeZone, "HH:mm:ss")}
      <span className="text-ink-faint">
        UTC{formatInTimeZone(now, timeZone, "xxx")}
      </span>
    </span>
  )
}

export function Settings() {
  const [theme] = useTheme()
  const timeZone = displayTimeZone()

  return (
    <div className="max-w-3xl">
      <PageTitle
        title="Settings"
        subtitle="How this browser displays kaas-ui. Nothing here is sent to the server."
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
          <Setting label="Local time" note="right now, in the zone above">
            <Now timeZone={timeZone} />
          </Setting>
        </div>
        <p className="text-ink-muted mt-3 text-[12px]">
          Brokers deal in epoch milliseconds and so does the wire format, so the
          zone is applied once, on the way to the screen. It is not a filter on
          what a cluster returns.
        </p>
      </Section>

      <Section title="Where these live">
        <Empty>
          In this browser's local storage — not in your account, and not on the
          server. Another browser, or this one after its site data is cleared,
          starts from the defaults again.
        </Empty>
      </Section>
    </div>
  )
}
