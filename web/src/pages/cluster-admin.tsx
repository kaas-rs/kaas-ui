// The read-only admin surface: five screens over five describes.
//
// One page with five tabs rather than five sidebar items, because they share a
// shape — a cluster-wide administrative fact, in a table, that most clusters
// answer for and some do not — and five rows under every cluster in the nav
// would crowd out the four things people open every day.
//
// **The tab set is a conformance report.** Which of the five a cluster offers
// is which api keys it advertises, so the same page against two clusters says
// what they differ by without anybody comparing version tables. That is the
// Phase 1 capability projection being paid off rather than a new mechanism.

import { useNavigate, useParams, useSearch } from "@tanstack/react-router"

import {
  useAcls,
  useCapabilities,
  useQuotas,
  useReassignments,
  useScramUsers,
  useTransactions,
} from "@/api/client"
import type { Feature } from "@/api/types"
import {
  CapabilityTab,
  Empty,
  ErrorChips,
  Spinner,
  capabilityGate,
  capabilityState,
} from "@/components/domain"
import { PageTitle } from "@/components/page-title"
import { Tabs, TabsContent, TabsList } from "@/components/ui/tabs"
import { AclTable } from "@/features/admin/acl-table"
import { QuotaTable } from "@/features/admin/quota-table"
import { ReassignmentTable } from "@/features/admin/reassignment-table"
import { ScramTable } from "@/features/admin/scram-table"
import { TransactionTable } from "@/features/admin/transaction-table"

/** The five, in the order they are worth looking at. */
export const ADMIN_TABS = [
  "acls",
  "quotas",
  "scram",
  "reassignments",
  "transactions",
] as const

export type AdminTab = (typeof ADMIN_TABS)[number]

/** Which feature each tab needs, which is what decides whether it exists. */
const FEATURE: Record<AdminTab, Feature> = {
  acls: "acls",
  quotas: "quotas",
  scram: "scramUsers",
  reassignments: "reassignments",
  transactions: "transactions",
}

export function ClusterAdminPage({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  const navigate = useNavigate()
  const search = useSearch({
    from: "/environments/$envId/clusters/$clusterId/admin",
  })
  const params = useParams({
    from: "/environments/$envId/clusters/$clusterId",
  })
  const capabilities = useCapabilities(envId, clusterId)

  const tab = search.screen
  const setTab = (next: string) =>
    navigate({
      to: "/environments/$envId/clusters/$clusterId/admin",
      params,
      search: { screen: next as AdminTab },
      replace: true,
    })

  // Every one unsupported is a page with nothing on it and no explanation,
  // because each explanation is behind a tab that is not rendered. Said once,
  // up front, on the cluster that has none of it.
  const none =
    capabilities.data !== undefined &&
    ADMIN_TABS.every((entry) => {
      const state = capabilityState(capabilities.data, FEATURE[entry])
      return state !== undefined && state.state !== "available"
    })

  return (
    <>
      <PageTitle
        title="Admin"
        subtitle="Read only, all of it: describes with no altering counterpart in this application."
      />

      {capabilities.isLoading ? (
        <Spinner label="reading what this cluster can be asked" />
      ) : none ? (
        <Empty>
          this cluster implements none of the admin apis — no ACLs, quotas,
          SCRAM users, reassignments or transactions to describe
        </Empty>
      ) : (
        <Tabs value={tab} onValueChange={setTab}>
          <TabsList>
            <CapabilityTab
              value="acls"
              label="ACLs"
              capabilities={capabilities.data}
              feature="acls"
            />
            <CapabilityTab
              value="quotas"
              label="quotas"
              capabilities={capabilities.data}
              feature="quotas"
            />
            <CapabilityTab
              value="scram"
              label="SCRAM users"
              capabilities={capabilities.data}
              feature="scramUsers"
            />
            <CapabilityTab
              value="reassignments"
              label="reassignments"
              capabilities={capabilities.data}
              feature="reassignments"
            />
            <CapabilityTab
              value="transactions"
              label="transactions"
              capabilities={capabilities.data}
              feature="transactions"
            />
          </TabsList>

          <TabsContent value="acls" className="mt-4">
            <AclsScreen envId={envId} clusterId={clusterId} />
          </TabsContent>
          <TabsContent value="quotas" className="mt-4">
            <QuotasScreen envId={envId} clusterId={clusterId} />
          </TabsContent>
          <TabsContent value="scram" className="mt-4">
            <ScramScreen envId={envId} clusterId={clusterId} />
          </TabsContent>
          <TabsContent value="reassignments" className="mt-4">
            <ReassignmentsScreen envId={envId} clusterId={clusterId} />
          </TabsContent>
          <TabsContent value="transactions" className="mt-4">
            <TransactionsScreen envId={envId} clusterId={clusterId} />
          </TabsContent>
        </Tabs>
      )}
    </>
  )
}

/**
 * Each screen gates itself.
 *
 * The tab is not rendered when the api is missing, but the URL still is —
 * `?screen=transactions` in a link somebody sent — and a routed-to screen that
 * renders an empty table would be the worst of the three answers. The gate
 * returns the panel naming both version ranges, and `null` when the feature is
 * there.
 */
function AclsScreen({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  const capabilities = useCapabilities(envId, clusterId)
  const acls = useAcls(envId, clusterId)
  const gate = capabilityGate(capabilities.data, "acls", "the ACL viewer")
  if (gate) return gate

  return (
    <>
      <ErrorChips errors={acls.data?.errors ?? []} />
      {acls.isLoading ? (
        <Spinner />
      ) : acls.error ? (
        <Empty>{String(acls.error)}</Empty>
      ) : (acls.data?.items.length ?? 0) === 0 ? (
        // An empty list and an absent authorizer are different facts, and the
        // second arrives as an error rather than as an empty list — a cluster
        // with no authorizer configured answers `SECURITY_DISABLED`, which is
        // the chip above and not this line.
        <Empty>
          this cluster has an authorizer and no bindings in it — everything is
          governed by its default
        </Empty>
      ) : (
        <AclTable acls={acls.data?.items ?? []} />
      )}
    </>
  )
}

function QuotasScreen({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  const capabilities = useCapabilities(envId, clusterId)
  const quotas = useQuotas(envId, clusterId)
  const gate = capabilityGate(capabilities.data, "quotas", "client quotas")
  if (gate) return gate

  return (
    <>
      <ErrorChips errors={quotas.data?.errors ?? []} />
      {quotas.isLoading ? (
        <Spinner />
      ) : (quotas.data?.items.length ?? 0) === 0 ? (
        <Empty>no client quotas are configured — nothing is throttled</Empty>
      ) : (
        <QuotaTable quotas={quotas.data?.items ?? []} />
      )}
    </>
  )
}

function ScramScreen({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  const capabilities = useCapabilities(envId, clusterId)
  const users = useScramUsers(envId, clusterId)
  const gate = capabilityGate(
    capabilities.data,
    "scramUsers",
    "the SCRAM user list"
  )
  if (gate) return gate

  return (
    <>
      <p className="mb-3 text-[13px] text-ink-muted">
        Who can authenticate, not how. The broker stores a salted hash and has
        no api that returns one.
      </p>
      <ErrorChips errors={users.data?.errors ?? []} />
      {users.isLoading ? (
        <Spinner />
      ) : (users.data?.items.length ?? 0) === 0 ? (
        <Empty>no SCRAM credentials are stored on this cluster</Empty>
      ) : (
        <ScramTable users={users.data?.items ?? []} />
      )}
    </>
  )
}

function ReassignmentsScreen({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  const capabilities = useCapabilities(envId, clusterId)
  const moves = useReassignments(envId, clusterId)
  const gate = capabilityGate(
    capabilities.data,
    "reassignments",
    "the reassignment view"
  )
  if (gate) return gate

  return (
    <>
      <ErrorChips errors={moves.data?.errors ?? []} />
      {moves.isLoading ? (
        <Spinner />
      ) : (moves.data?.items.length ?? 0) === 0 ? (
        // Nothing moving is the healthy answer and reads as one: this is not
        // an empty screen, it is a cluster at rest.
        <Empty>
          nothing is moving — no partition reassignment is in flight
        </Empty>
      ) : (
        <ReassignmentTable
          envId={envId}
          clusterId={clusterId}
          moves={moves.data?.items ?? []}
        />
      )}
    </>
  )
}

function TransactionsScreen({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  const capabilities = useCapabilities(envId, clusterId)
  const transactions = useTransactions(envId, clusterId)
  const gate = capabilityGate(
    capabilities.data,
    "transactions",
    "the transaction inspector"
  )
  if (gate) return gate

  return (
    <>
      <ErrorChips errors={transactions.data?.errors ?? []} />
      {transactions.isLoading ? (
        <Spinner />
      ) : (transactions.data?.items.length ?? 0) === 0 ? (
        <Empty>no transactional producer has an id on this cluster</Empty>
      ) : (
        <TransactionTable
          envId={envId}
          clusterId={clusterId}
          transactions={transactions.data?.items ?? []}
        />
      )}
    </>
  )
}
