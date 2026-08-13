// Who you are, which roles you resolved to, and what those roles actually
// reach.
//
// The third part is the reason this page exists. A role list is a set of names
// — `admin`, `prod-oncall` — and names do not answer the question people
// actually arrive with, which is "why can I not see that cluster". So the last
// section is not the policy as written but the policy as *applied*: one row per
// cluster this caller can see, and what they may do on it.
//
// It is built from the cluster cards rather than from a new endpoint, because
// the cards already carry the effective answer — the same `grants` the sidebar
// hides items with. Reading it here means the page cannot disagree with the
// navigation, which a second source would eventually do.

import { ShieldCheck } from "lucide-react"
import type { ReactNode } from "react"

import { useFleet, useIdentity } from "@/api/client"
import type { Action, Resource } from "@/api/types"
import {
  ClusterChip,
  Empty,
  HintHead,
  Section,
  Spinner,
} from "@/components/domain"
import { Badge } from "@/components/ui/badge"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { PageTitle } from "@/components/page-title"

/** The resources a row reports on, in the order the sidebar uses them. */
const RESOURCES: { resource: Resource; label: string; what: string }[] = [
  {
    resource: "cluster_config",
    label: "cluster",
    what: "brokers, configs, capabilities",
  },
  {
    resource: "topic",
    label: "topics",
    what: "the topic list, and each topic's detail",
  },
  {
    resource: "consumer",
    label: "groups",
    what: "consumer groups, offsets and lag",
  },
]

/**
 * One cell of the access table.
 *
 * Three states, not two. "May view" and "may also read payloads" are the
 * boundary the whole permission model turns on — browsing a topic's
 * configuration is not the same act as reading customer data out of it — so
 * they must not render as the same tick.
 */
function Grant({ actions }: { actions: Action[] | undefined }) {
  if (!actions || actions.length === 0) {
    return <span className="text-ink-faint">—</span>
  }
  const payloads = actions.includes("messages_read")
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className="text-ok text-[13px] font-medium">view</span>
      {payloads ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <Badge variant="outline" className="cursor-default text-[11px]">
              payloads
            </Badge>
          </TooltipTrigger>
          <TooltipContent>
            this role may read message bodies here, not only metadata
          </TooltipContent>
        </Tooltip>
      ) : null}
    </span>
  )
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="border-line flex items-baseline justify-between gap-6 border-b py-2.5 last:border-0">
      <span className="text-ink-muted shrink-0 text-[13px]">{label}</span>
      <span className="min-w-0 text-right text-[13px]">{children}</span>
    </div>
  )
}

export function AccountPage() {
  const identity = useIdentity()
  // The fleet, not one environment's clusters: this page answers "what do I
  // reach", and that question does not sit inside an environment.
  const fleet = useFleet()
  const cards = fleet.data?.items.flatMap((section) => section.clusters) ?? []

  if (identity.isLoading) return <Spinner label="reading your identity" />
  const me = identity.data
  if (!me) return <Empty>the server did not say who you are</Empty>

  return (
    <div className="max-w-3xl">
      <PageTitle
        title="Account"
        subtitle="Who this session is, and what it reaches."
      />

      <Section title="Identity">
        <div className="rounded-md border px-4 py-1">
          <Row label="name">
            {me.authenticated ? (
              <span className="font-medium">{me.displayName}</span>
            ) : (
              <span className="text-ink-muted">anonymous</span>
            )}
          </Row>
          <Row label="subject">
            {/* The claim a role's `subjects` is matched against — so this is
                the string to quote when asking someone for access. */}
            <span className="font-mono text-[12px] break-all">
              {me.subject}
            </span>
          </Row>
          <Row label="signed in">
            {me.authenticated ? (
              <span className="text-ok font-medium">yes</span>
            ) : (
              <span className="text-ink-muted">
                no
                {me.loginAvailable
                  ? ""
                  : " — this deployment has no identity provider"}
              </span>
            )}
          </Row>
          <Row label="roles enforced">
            {me.enforcing ? (
              "yes"
            ) : (
              <span className="text-ink-muted">
                no — every caller is an administrator here
              </span>
            )}
          </Row>
        </div>
      </Section>

      <Section title="Roles">
        {me.roles.length > 0 ? (
          <div className="flex flex-wrap gap-2">
            {me.roles.map((role) => (
              <Badge key={role} variant="secondary" className="gap-1.5 py-1">
                <ShieldCheck aria-hidden className="size-3.5" />
                {role}
              </Badge>
            ))}
          </div>
        ) : me.enforcing ? (
          <Empty>
            No role covers this caller, so no cluster is visible. A role is
            matched on the subject, login or email above.
          </Empty>
        ) : (
          <Empty>
            No roles are configured, so nothing is restricted — everything is
            visible to everyone who can reach this server.
          </Empty>
        )}
      </Section>

      <Section title="Access">
        {fleet.isLoading ? (
          <Spinner label="reading the fleet" />
        ) : cards.length === 0 ? (
          <Empty>
            No cluster is visible to this caller. A cluster nobody may see is a{" "}
            <code className="font-mono">404</code>, not a{" "}
            <code className="font-mono">403</code>, so this list being empty is
            the same answer as the fleet being empty.
          </Empty>
        ) : (
          <>
            <p className="text-ink-muted mb-3 text-[13px]">
              What these roles reach, per cluster. This is the applied answer —
              the same grants the sidebar hides items with — not the policy as
              written.
            </p>
            <div className="overflow-x-auto rounded-md border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <HintHead
                      label="cluster"
                      hint="every cluster you can see, in every environment — a cluster you cannot is not listed here either"
                    />
                    {RESOURCES.map((entry) => (
                      <HintHead
                        key={entry.resource}
                        label={entry.label}
                        hint={entry.what}
                      />
                    ))}
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {cards.map((card) => (
                    <TableRow key={card.id}>
                      <TableCell>
                        <ClusterChip id={card.id} labels={card.labels} />
                      </TableCell>
                      {RESOURCES.map((entry) => (
                        <TableCell key={entry.resource}>
                          <Grant actions={card.grants[entry.resource]} />
                        </TableCell>
                      ))}
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          </>
        )}
      </Section>
    </div>
  )
}
