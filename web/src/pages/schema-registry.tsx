// The schema registry browser.
//
// A view of a **registry**, reached from a cluster. Two clusters that
// reference `dev` show the same subject list, so every screen here names the
// registry that is answering rather than implying the subjects belong to the
// cluster whose nav you arrived through.
//
// Read-only, like everything else: no registering, no compatibility changes,
// no deleting a version. What kafbat spends its detail page's buttons on is
// the half of that page kaas-ui does not have.

import { useMemo, useState } from "react"
import { Link } from "@tanstack/react-router"
import { AlertTriangle, ArrowLeft, FileWarning } from "lucide-react"

import {
  useFleet,
  useSubjectDetails,
  useSubjects,
  useSubjectVersions,
  useTopics,
} from "@/api/client"
import type { RegistryCard, SubjectRow, SubjectSchema } from "@/api/types"
import { Empty, ErrorChips, Mono, Section, Spinner } from "@/components/domain"
import { PageTitle } from "@/components/page-title"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
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
        subtitle={`schema registry — ${total} subject${total === 1 ? "" : "s"}`}
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

/**
 * The topic a subject was registered for, under `TopicNameStrategy`.
 *
 * Only that strategy is inferable: `RecordNameStrategy` names the Avro record
 * and `TopicRecordNameStrategy` glues both together, and neither can be undone
 * without guessing. Returning `null` there is the honest answer — the button
 * is absent rather than pointing somewhere plausible and wrong.
 */
function topicOf(subject: string): string | null {
  for (const suffix of ["-value", "-key"]) {
    if (subject.endsWith(suffix) && subject.length > suffix.length) {
      return subject.slice(0, -suffix.length)
    }
  }
  return null
}

export function SchemaDetail({
  envId,
  registryId,
  subject,
}: {
  envId: string
  registryId: string
  subject: string
}) {
  const detail = useSubjectVersions(envId, registryId, subject)
  const [left, setLeft] = useState<number>()
  const [right, setRight] = useState<number>()

  const back = (
    <Button variant="ghost" size="sm" asChild>
      <Link
        to="/environments/$envId/schema-registries/$registryId"
        params={{ envId, registryId }}
      >
        <ArrowLeft aria-hidden />
        schema registry
      </Link>
    </Button>
  )

  if (detail.isLoading) return <Spinner label={`reading ${subject}`} />
  if (detail.error) {
    return (
      <>
        <PageTitle title={subject} actions={back} />
        <p className="text-xs text-danger">{(detail.error as Error).message}</p>
      </>
    )
  }

  const versions = detail.data?.versions ?? []
  const newest = versions[versions.length - 1]

  if (!newest) {
    // The response's own registry card is what tells "the subject holds
    // nothing" apart from "the registry could not answer" — the list page's
    // banner may still be showing a cached `ready` from before it went down.
    const fault =
      detail.data?.registry && detail.data.registry.status !== "ready"
        ? detail.data.registry
        : null
    return (
      <>
        <PageTitle
          title={<span className="font-mono">{subject}</span>}
          actions={back}
        />
        {detail.data?.errors.length ? (
          <ErrorChips errors={detail.data.errors} />
        ) : null}
        {fault ? (
          <p
            className={cn(
              "flex items-start gap-2 text-xs",
              fault.status === "misconfigured" ? "text-danger" : "text-warn-ink"
            )}
          >
            <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
            <span>
              The registry is {fault.status}
              {fault.error ? `: ${fault.error}` : "."} Whether{" "}
              <span className="font-mono">{subject}</span> holds versions is
              unknown until it answers.
            </span>
          </p>
        ) : (
          <Empty>
            The registry lists no versions for <Mono>{subject}</Mono>.
          </Empty>
        )}
      </>
    )
  }

  const previous = versions[versions.length - 2]
  const a = versions.find((v) => v.version === left) ?? previous ?? newest
  const b = versions.find((v) => v.version === right) ?? newest
  const older = versions.slice(0, -1).reverse()

  return (
    <>
      <PageTitle
        title={<span className="font-mono">{subject}</span>}
        subtitle={
          detail.data?.registry ? (
            <span className="flex flex-wrap items-center gap-3">
              <span>
                {versions.length} version{versions.length === 1 ? "" : "s"}
              </span>
              <span className="text-ink-faint">
                in {detail.data.registry.name}
              </span>
            </span>
          ) : undefined
        }
        actions={
          <span className="flex items-center gap-2">
            <GoToTopic envId={envId} subject={subject} />
            {back}
          </span>
        }
      />

      {detail.data?.errors.length ? (
        <ErrorChips errors={detail.data.errors} />
      ) : null}

      {/* The schema and the facts about it, side by side. The text is the
          reason anyone opened this page, so it gets the width; the facts are
          five short lines and read as a column. */}
      <div className="grid gap-4 lg:grid-cols-[2.4fr_minmax(220px,1fr)]">
        <Section title="Actual version">
          <SchemaText text={newest.schema} format={newest.format} />
          <References schema={newest} />
        </Section>

        <Card className="h-fit p-4">
          <dl className="space-y-3 text-xs">
            <Fact label="Latest version">{newest.version}</Fact>
            <Fact label="ID">#{newest.id}</Fact>
            <Fact label="Type">
              <Badge variant="outline">{newest.format}</Badge>
            </Fact>
            <Fact label="Subject">
              <span className="font-mono break-all">{subject}</span>
            </Fact>
            <Fact label="Compatibility">
              {detail.data?.compatibility ?? (
                <span className="text-ink-faint">—</span>
              )}
            </Fact>
          </dl>
        </Card>
      </div>

      {older.length ? (
        <Section title="Old versions">
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="text-right">version</TableHead>
                  <TableHead className="text-right">id</TableHead>
                  <TableHead>type</TableHead>
                  <TableHead />
                </TableRow>
              </TableHeader>
              <TableBody>
                {older.map((version) => (
                  <OldVersion key={version.version} schema={version} />
                ))}
              </TableBody>
            </Table>
          </div>
        </Section>
      ) : null}

      {versions.length > 1 ? (
        <Section title="Compare versions">
          <div className="mb-2 flex items-center gap-2 text-xs">
            <VersionSelect
              label="From"
              versions={versions}
              value={a?.version}
              onChange={setLeft}
            />
            <VersionSelect
              label="To"
              versions={versions}
              value={b?.version}
              onChange={setRight}
            />
          </div>
          {a && b ? <Diff before={a.schema} after={b.schema} /> : null}
        </Section>
      ) : null}
    </>
  )
}

function Fact({
  label,
  children,
}: {
  label: string
  children: React.ReactNode
}) {
  return (
    <div>
      <dt className="text-[11px] text-ink-faint">{label}</dt>
      <dd className="mt-0.5 font-mono">{children}</dd>
    </div>
  )
}

/**
 * A link to the topic this subject describes, when there is one to be sure of.
 *
 * The topic is looked up rather than assumed. `TopicNameStrategy` says
 * `orders-value` describes `orders`, but a subject can outlive its topic, and
 * a button that opens "the cluster did not describe this topic" is worse than
 * no button. The lookup is served from the metadata snapshot, so it costs no
 * broker round trip.
 */
function GoToTopic({ envId, subject }: { envId: string; subject: string }) {
  const name = topicOf(subject)
  // Which cluster? A registry serves the environment, so the subject names no
  // cluster and cannot. Every cluster here that decodes against a registry is
  // a candidate; the first one that actually holds the topic is the answer,
  // and if none does there is no button.
  const fleet = useFleet()
  const candidates =
    fleet.data?.items
      .find((section) => section.id === envId)
      ?.clusters.filter((card) => card.schemaRegistry !== null) ?? []
  const clusterId = candidates[0]?.id ?? ""
  const topics = useTopics(envId, clusterId, {
    search: name ?? "",
    limit: PAGE,
  })
  if (!name || !clusterId) return null
  const exists = topics.data?.items.some((topic) => topic.name === name)
  if (!exists) return null

  return (
    <Button variant="outline" size="sm" asChild>
      <Link
        to="/environments/$envId/clusters/$clusterId/topics/$topic"
        params={{ envId, clusterId, topic: name }}
      >
        go to topic <span className="font-mono">{name}</span>
      </Link>
    </Button>
  )
}

/**
 * One superseded version, collapsed.
 *
 * Expanded on demand rather than rendered: the text is already in the response
 * — the server fetched every version to build it — so this is a disclosure and
 * not a fetch, and thirty versions of a hundred-line schema is a page nobody
 * can scroll.
 */
function OldVersion({ schema }: { schema: SubjectSchema }) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <TableRow>
        <TableCell className="text-right font-mono">{schema.version}</TableCell>
        <TableCell className="text-right font-mono">#{schema.id}</TableCell>
        <TableCell>
          <Badge variant="outline">{schema.format}</Badge>
        </TableCell>
        <TableCell className="text-right">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setOpen(!open)}
            aria-expanded={open}
          >
            {open ? "hide" : "show"}
          </Button>
        </TableCell>
      </TableRow>
      {open ? (
        <TableRow>
          <TableCell colSpan={4} className="p-2">
            <SchemaText text={schema.schema} format={schema.format} />
            <References schema={schema} />
          </TableCell>
        </TableRow>
      ) : null}
    </>
  )
}

function VersionSelect({
  label,
  versions,
  value,
  onChange,
}: {
  label: string
  versions: SubjectSchema[]
  value?: number
  onChange(version: number): void
}) {
  return (
    <Label className="gap-1 text-xs font-normal text-ink-faint">
      {label}
      <Select
        value={value !== undefined ? String(value) : undefined}
        onValueChange={(next) => onChange(Number(next))}
      >
        <SelectTrigger className="w-[110px]" aria-label={`${label} version`}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {versions.map((version) => (
            <SelectItem key={version.version} value={String(version.version)}>
              v{version.version} (#{version.id})
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Label>
  )
}

function References({ schema }: { schema: SubjectSchema }) {
  if (!schema.references.length) return null
  return (
    <div className="mt-2 space-y-1 text-[11px]">
      <p className="text-ink-faint">
        References — stored separately, and followed when decoding:
      </p>
      <ul className="space-y-0.5">
        {schema.references.map((reference) => (
          <li key={`${reference.subject}-${reference.version}`}>
            <Mono>{reference.name}</Mono>
            {" → "}
            <Mono>
              {reference.subject} v{reference.version}
            </Mono>
          </li>
        ))}
      </ul>
    </div>
  )
}

/**
 * The schema text.
 *
 * JSON — which Avro and JSON Schema both are — is pretty-printed so the
 * registry's own whitespace does not decide readability. Protobuf is `.proto`
 * source and is shown as it was registered.
 */
function SchemaText({ text, format }: { text: string; format: string }) {
  const shown = format === "protobuf" ? text : prettyJson(text)
  return (
    <pre className="max-h-[45vh] overflow-auto rounded-md border border-line bg-surface-sunken p-3 font-mono text-[11px] leading-relaxed whitespace-pre">
      {shown}
    </pre>
  )
}

function prettyJson(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2)
  } catch {
    // Not JSON after all. Showing it verbatim beats showing an error about
    // formatting something nobody asked to have formatted.
    return text
  }
}

/**
 * A line diff between two registered versions.
 *
 * Both sides are normalised first — parsed and re-printed where they are JSON
 * — so a version that differs only in the registry's whitespace shows as
 * unchanged rather than as every line rewritten.
 */
function Diff({ before, after }: { before: string; after: string }) {
  const left = prettyJson(before).split("\n")
  const right = prettyJson(after).split("\n")
  const lines = diffLines(left, right)

  if (!lines.some((line) => line.kind !== "same")) {
    return (
      <p className="flex items-center gap-2 text-xs text-ink-muted">
        <FileWarning className="size-3.5" aria-hidden />
        These two versions are identical once formatting is ignored.
      </p>
    )
  }

  return (
    <pre className="max-h-[45vh] overflow-auto rounded-md border border-line bg-surface-sunken p-3 font-mono text-[11px] leading-relaxed whitespace-pre">
      {lines.map((line, index) => (
        <div
          key={index}
          className={cn(
            line.kind === "added" && "bg-ok/15 text-ok-ink",
            line.kind === "removed" && "bg-danger/15 text-danger"
          )}
        >
          {line.kind === "added" ? "+" : line.kind === "removed" ? "-" : " "}
          {line.text}
        </div>
      ))}
    </pre>
  )
}

interface DiffLine {
  kind: "same" | "added" | "removed"
  text: string
}

/**
 * The classic longest-common-subsequence diff, on lines.
 *
 * Quadratic, and that is fine here: a schema is tens of lines, not thousands,
 * and the alternative is a diff library in the bundle for one screen.
 */
function diffLines(left: string[], right: string[]): DiffLine[] {
  const rows = left.length
  const columns = right.length
  const table: number[][] = Array.from({ length: rows + 1 }, () =>
    new Array<number>(columns + 1).fill(0)
  )
  for (let i = rows - 1; i >= 0; i -= 1) {
    for (let j = columns - 1; j >= 0; j -= 1) {
      const row = table[i]
      const next = table[i + 1]
      if (!row || !next) continue
      row[j] =
        left[i] === right[j]
          ? (next[j + 1] ?? 0) + 1
          : Math.max(next[j] ?? 0, row[j + 1] ?? 0)
    }
  }

  const out: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < rows && j < columns) {
    if (left[i] === right[j]) {
      out.push({ kind: "same", text: left[i] ?? "" })
      i += 1
      j += 1
      continue
    }
    const down = table[i + 1]?.[j] ?? 0
    const across = table[i]?.[j + 1] ?? 0
    if (down >= across) {
      out.push({ kind: "removed", text: left[i] ?? "" })
      i += 1
    } else {
      out.push({ kind: "added", text: right[j] ?? "" })
      j += 1
    }
  }
  while (i < rows) {
    out.push({ kind: "removed", text: left[i] ?? "" })
    i += 1
  }
  while (j < columns) {
    out.push({ kind: "added", text: right[j] ?? "" })
    j += 1
  }
  return out
}
