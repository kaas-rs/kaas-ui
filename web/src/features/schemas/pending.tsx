import type { ReactNode } from "react"

/**
 * A cell whose value is still on its way, or never coming.
 *
 * Blank and `—` are different answers: the first means the registry has not
 * been asked yet, the second that it was and had nothing to say. Collapsing
 * them makes a slow registry indistinguishable from a broken one.
 */
export function Pending<T>({
  value,
  fetching,
  children,
}: {
  value: T | null
  fetching: boolean
  children: (value: T) => ReactNode
}) {
  if (value !== null && value !== undefined) return <>{children(value)}</>
  return (
    <span
      className="text-ink-faint"
      title={fetching ? "still asking" : undefined}
    >
      {fetching ? "·" : "—"}
    </span>
  )
}
