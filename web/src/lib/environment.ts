// Which environment the sidebar is showing.
//
// A module-level store rather than a context, for the same reason the theme in
// `settings.ts` is one: two components in different parts of the tree need the
// answer — the sidebar menu, which shows one environment's clusters, and the
// breadcrumb, whose environment crumb switches it — and threading a provider
// between them would be a wrapper that exists only to carry one string.
//
// Nothing sets it by hand any more. Opening a cluster sets it, because the nav
// has to be showing the environment you are in; the breadcrumb sets it when
// you pick a different one. There is no control whose only job is this.
//
// It is remembered per browser. Which environment you work in is a fact about
// you and not about the deployment, and re-picking it after every reload is
// the kind of small tax that makes a tool feel like it is not paying attention.

import { useSyncExternalStore } from "react"

const KEY = "kaas-ui-environment"

let current: string | null = localStorage.getItem(KEY)
const listeners = new Set<() => void>()

/** Remember an environment as the one being looked at. */
export function chooseEnvironment(id: string) {
  if (current === id) return
  current = id
  localStorage.setItem(KEY, id)
  for (const listener of listeners) listener()
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

/** The remembered id, which may name an environment that no longer exists. */
export function useChosenEnvironment(): string | null {
  return useSyncExternalStore(
    subscribe,
    () => current,
    () => current
  )
}

/**
 * The environment to show, given what is remembered.
 *
 * Falls through to the first section, so a first visit, a cleared browser and
 * a renamed environment all land somewhere real rather than on an empty nav.
 */
export function pickEnvironment<T extends { id: string }>(
  sections: T[],
  chosen: string | null
): T | undefined {
  return sections.find((section) => section.id === chosen) ?? sections[0]
}
