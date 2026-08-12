import type { ReactNode } from "react"

import type { SubjectSchema } from "@/api/types"
import { Section } from "@/components/domain"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"

export function SubjectOverview({
  newest,
  versions,
  compatibility,
}: {
  newest: SubjectSchema
  versions: SubjectSchema[]
  compatibility: string | null
}) {
  // Only a count now that the old-versions table is gone — the compare
  // control reaches every version, so a second list of them was two ways to
  // open the same text.
  const superseded = versions.length - 1

  return (
    <Section title="Overview">
      <Card className="px-5 py-4">
        {/* Two columns on a phone, five on a desktop, separated by space
            rather than rules. A wrapped grid has no row a separator could
            belong to, so drawing one means nth-child arithmetic per
            breakpoint — three chances to leave a stray line down the middle,
            for a divider that five short facts do not need. */}
        <dl className="grid grid-cols-2 gap-x-8 gap-y-4 sm:grid-cols-3 lg:grid-cols-5">
          <Fact label="Latest version">v{newest.version}</Fact>
          <Fact label="Schema id" hint="The number the wire format carries.">
            #{newest.id}
          </Fact>
          <Fact label="Type">
            <Badge variant="outline">{newest.format}</Badge>
          </Fact>
          <Fact label="Versions">
            {versions.length}
            {superseded > 0 ? (
              <span className="text-ink-faint text-[11px]">
                {" "}
                ({superseded} superseded)
              </span>
            ) : null}
          </Fact>
          <Fact
            label="Compatibility"
            hint="What the registry will accept as the next version."
          >
            {compatibility ?? <span className="text-ink-faint">—</span>}
          </Fact>
        </dl>
      </Card>
    </Section>
  )
}

/**
 * One cell of the overview.
 *
 * `Subject` is not among them any more: the page title is the subject, in
 * mono, two inches above — a strip whose first job is to repeat the heading
 * has spent a fifth of itself saying nothing. `Versions` took the slot, which
 * is the one fact the page held and never stated.
 */
function Fact({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: ReactNode
}) {
  return (
    <div className="min-w-0">
      <dt
        className="text-ink-faint text-[11px] tracking-wide uppercase"
        title={hint}
        style={hint ? { cursor: "help" } : undefined}
      >
        {label}
      </dt>
      <dd className="mt-1 truncate font-mono text-[15px]">{children}</dd>
    </div>
  )
}
