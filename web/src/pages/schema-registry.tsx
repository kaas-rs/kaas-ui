// The subject table.
//
// A view of a **registry**, which is a peer of a cluster inside its
// environment rather than a feature of one — every cluster here that names it
// reads these same subjects, from the same handle, through the same cache. So
// the page is named after the registry and never after a cluster.
//
// Read-only, like everything else: no registering, no compatibility changes.
// The subject *itself* is `schema-detail.tsx`; this file is only the list, and
// the two are separate because they share nothing but a route prefix — one is
// a paged table over names, the other is text, versions and a diff.

import { useMemo, useState } from "react"
import { Link } from "@tanstack/react-router"
import { AlertTriangle } from "lucide-react"

import { useEnvironment, useSubjectDetails, useSubjects } from "@/api/client"
import type { RegistryCard, SubjectRow } from "@/api/types"
import { Empty, Mono, Spinner } from "@/components/domain"
import { PageTitle } from "@/components/page-title"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { cn } from "@/lib/utils"

const PAGE = 50

export function SchemaRegistry({
  envId,
  registryId,
}: {
  envId: string
  registryId: string
}) {
  const [search, setSearch] = useState("")
  const [order, setOrder] = useState<"asc" | "desc">("asc")
  const [offset, setOffset] = useState(0)

  const query = { search, order, limit: PAGE, offset }

  // Two requests for one table, as on the topic list. The names are one cached
  // call; id, type, version and compatibility are two registry calls per row,
  // so they arrive into a table that is already on screen.
  const subjects = useSubjects(envId, registryId, query)
  const details = useSubjectDetails(envId, registryId, query)
  // Who reads these. A registry serves the *environment*, so every subject
  // below is resolvable on every cluster that decodes against it — which is a
  // fact about the whole table and belongs above it, not repeated per row.
  const environment = useEnvironment(envId)
  const usedBy =
    environment.data?.items[0]?.schemaRegistries.find(
      (entry) => entry.registry.id === registryId
    )?.usedBy ?? []

  const described = useMemo(() => {
    const map = new Map<string, SubjectRow>()
    for (const row of details.data?.subjects ?? []) map.set(row.subject, row)
    return map
  }, [details.data])

  if (subjects.isLoading) return <Spinner label="reading the registry" />

  // A failed request is not an absent registry. Rendering the "add
  // `schema_registry:` to your config" panel over a transient 5xx would send
  // an operator to edit configuration that is already correct.
  if (subjects.error) {
    return (
      <>
        <PageTitle
          title="Schema registry"
          subtitle="The subject list could not be read."
        />
        <p className="flex items-start gap-2 text-xs text-danger">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          {(subjects.error as Error).message}
        </p>
      </>
    )
  }

  const registry = subjects.data?.registry ?? null

  // Absence is a normal path, not a degraded one: a kaas instance with no
  // registry sits in the same environment as a Strimzi cluster with one.
  if (!registry) {
    return (
      <>
        <PageTitle
          title="Schema registry"
          subtitle="This cluster does not reference a schema registry."
        />
        <Empty>
          A registry serves an <em>environment</em>, and a cluster opts into one
          by name. Declare one under <Mono>schema_registries</Mono> and point
          this cluster at it with <Mono>schema_registry: &lt;id&gt;</Mono>.
          Records on this cluster are rendered as text or hex until then.
        </Empty>
      </>
    )
  }

  const total = subjects.data?.total ?? 0
  const rows = subjects.data?.subjects ?? []

  return (
    <>
      {/* Named after the registry, not after the page. You arrive here from a
          nav row that says "Apicurio (ccompat)", and a heading that answered
          with a category instead would leave you checking you landed right —
          on a fleet with two registries, twice. */}
      <PageTitle
        title={registry.name}
        subtitle={
          <span className="flex flex-wrap items-center gap-x-2">
            <span>
              schema registry — {total} subject{total === 1 ? "" : "s"}
            </span>
            {/* The plural is the point: these subjects are not one cluster's.
                Naming the readers here is what stops the table reading as a
                property of whichever cluster you arrived from. */}
            {usedBy.length > 0 ? (
              <span className="text-ink-faint">
                · resolved on{" "}
                {usedBy.map((clusterId, index) => (
                  <span key={clusterId}>
                    {index > 0 ? ", " : ""}
                    <Link
                      to="/environments/$envId/clusters/$clusterId"
                      params={{ envId, clusterId }}
                      className="font-mono hover:underline"
                    >
                      {clusterId}
                    </Link>
                  </span>
                ))}
              </span>
            ) : (
              <span className="text-ink-faint">
                · no cluster here decodes against it
              </span>
            )}
          </span>
        }
      />

      {/* Only when it is not answering. A registry that cannot be reached
          still returns an empty subject list, and without this line the table
          says "this registry holds no subjects" — which is a different claim,
          and the wrong one. Nothing is said while everything is fine: the
          endpoint and the id are on the nav row's tooltip. */}
      <RegistryFault registry={registry} />

      <div className="mb-4 flex flex-wrap items-center gap-4">
        <Input
          value={search}
          onChange={(event) => {
            setSearch(event.target.value)
            setOffset(0)
          }}
          placeholder="filter by subject"
          aria-label="Filter subjects"
          className="h-8 max-w-xs"
        />
      </div>

      {rows.length === 0 ? (
        <Empty>
          {search.trim()
            ? "No subject matches that."
            : "This registry holds no subjects."}
        </Empty>
      ) : (
        <>
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>
                    <button
                      type="button"
                      onClick={() => {
                        setOrder(order === "asc" ? "desc" : "asc")
                        setOffset(0)
                      }}
                      className="hover:underline"
                    >
                      subject{order === "asc" ? " ↑" : " ↓"}
                    </button>
                  </TableHead>
                  <TableHead className="text-right">id</TableHead>
                  <TableHead>type</TableHead>
                  <TableHead className="text-right">version</TableHead>
                  <TableHead>compatibility</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.map((row) => {
                  const full = described.get(row.subject) ?? row
                  return (
                    <TableRow key={row.subject}>
                      <TableCell>
                        <Link
                          to="/environments/$envId/schema-registries/$registryId/subjects/$subject"
                          params={{ envId, registryId, subject: row.subject }}
                          className="font-mono hover:underline"
                          style={{ color: "var(--rust-ink)" }}
                        >
                          {row.subject}
                        </Link>
                      </TableCell>
                      <TableCell className="text-right font-mono">
                        <Pending value={full.id} fetching={details.isFetching}>
                          {(id) => `#${id}`}
                        </Pending>
                      </TableCell>
                      <TableCell>
                        <Pending
                          value={full.format}
                          fetching={details.isFetching}
                        >
                          {(format) => (
                            <Badge variant="outline">{format}</Badge>
                          )}
                        </Pending>
                      </TableCell>
                      <TableCell className="text-right font-mono">
                        <Pending
                          value={full.version}
                          fetching={details.isFetching}
                        >
                          {(version) => String(version)}
                        </Pending>
                      </TableCell>
                      <TableCell>
                        <Pending
                          value={full.compatibility}
                          fetching={details.isFetching}
                        >
                          {(mode) => (
                            <Compatibility
                              mode={mode}
                              inherited={full.compatibilityInherited}
                            />
                          )}
                        </Pending>
                      </TableCell>
                    </TableRow>
                  )
                })}
              </TableBody>
            </Table>
          </div>

          {total > PAGE ? (
            <div className="mt-3 flex items-center gap-3 text-[12px]">
              <Button
                variant="outline"
                size="sm"
                disabled={offset === 0}
                onClick={() => setOffset(Math.max(0, offset - PAGE))}
              >
                previous
              </Button>
              <span className="text-ink-muted">
                {offset + 1}–{Math.min(offset + PAGE, total)} of {total}
              </span>
              <Button
                variant="outline"
                size="sm"
                disabled={offset + PAGE >= total}
                onClick={() => setOffset(offset + PAGE)}
              >
                next
              </Button>
            </div>
          ) : null}
        </>
      )}
    </>
  )
}

/**
 * A cell whose value is still on its way, or never coming.
 *
 * Blank and `—` are different answers: the first means the registry has not
 * been asked yet, the second that it was and had nothing to say. Collapsing
 * them makes a slow registry indistinguishable from a broken one.
 */
function Pending<T>({
  value,
  fetching,
  children,
}: {
  value: T | null
  fetching: boolean
  children: (value: T) => React.ReactNode
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

/**
 * A compatibility mode, and where it came from.
 *
 * `BACKWARD` set on this subject and `BACKWARD` inherited from the registry
 * are not the same fact — the second changes when somebody edits the registry
 * default, and the first does not.
 */
function Compatibility({
  mode,
  inherited,
}: {
  mode: string
  inherited: boolean
}) {
  return (
    <span className="flex items-center gap-1.5">
      <span className="font-mono text-[12px]">{mode}</span>
      {inherited ? (
        <span
          className="text-[11px] text-ink-faint"
          title="Inherited from the registry default — this subject sets no rule of its own"
        >
          inherited
        </span>
      ) : null}
    </span>
  )
}

/**
 * One line, and only when the registry is not answering.
 *
 * What is left of a banner that used to be here in every state. Saying "ready"
 * on a page that is visibly full of subjects was the page repeating itself,
 * and the id and endpoint it also carried are on the nav row's tooltip. The
 * failure is the part that was load-bearing: an unreachable registry returns
 * an empty list, and an empty list renders as "holds no subjects".
 */
function RegistryFault({ registry }: { registry: RegistryCard }) {
  if (registry.status === "ready") return null
  return (
    <p
      className={cn(
        "mb-4 flex items-start gap-2 text-xs",
        registry.status === "misconfigured" ? "text-danger" : "text-warn-ink"
      )}
    >
      <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
      <span>
        This registry is {registry.status}
        {registry.error ? `: ${registry.error}` : "."} What is listed below is
        what it last answered, which may be nothing at all.
      </span>
    </p>
  )
}

/* ------------------------------------------------------------------ detail */
