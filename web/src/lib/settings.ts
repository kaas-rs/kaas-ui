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

/** Shared with the inline script in `index.html`. Changing it changes both. */
const THEME_KEY = "kaas-ui-theme"

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
export function installTheme() {
  paint(current)

  // Following the OS is only meaningful if it keeps following it.
  window.matchMedia(DARK).addEventListener("change", () => {
    if (current === "system") paint(current)
  })

  // Another tab is the same browser, so it is the same setting. Without this
  // the two tabs disagree until one of them is reloaded.
  window.addEventListener("storage", (event) => {
    if (event.key !== null && event.key !== THEME_KEY) return
    current = readStored()
    paint(current)
    for (const listener of listeners) listener()
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
