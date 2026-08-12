import type { ReactNode } from "react"

export function Section({
  title,
  actions,
  children,
}: {
  title: string
  actions?: ReactNode
  children: ReactNode
}) {
  return (
    <section className="mb-8">
      <div className="mb-3 flex items-baseline justify-between gap-4">
        <h2 className="text-[15px] font-semibold tracking-[-0.01em]">
          {title}
        </h2>
        {actions}
      </div>
      {children}
    </section>
  )
}

/**
 * Everything a broker said verbatim is mono; everything kaas-ui wrote is sans.
 *
 * That split does real work: it tells the reader at a glance which strings they
 * can paste into `kafka-configs.sh`.
 */
export function Mono({ children }: { children: ReactNode }) {
  return (
    <span className="font-mono text-[13px] text-ink-muted">{children}</span>
  )
}

export function Empty({ children }: { children: ReactNode }) {
  return (
    <div className="rounded-md border border-dashed py-8 text-center text-[13px] text-ink-muted">
      {children}
    </div>
  )
}

export function Spinner({ label = "loading" }: { label?: string }) {
  return <div className="py-8 text-[13px] text-ink-faint">{label}…</div>
}
