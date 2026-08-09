// One subject: its newest schema, the facts about it, and what came before.
//
// Split from `schema-registry.tsx`, which is the table of subject *names*.
// They share a route prefix and nothing else — this page is text, versions and
// a diff, and keeping both in one file meant eight hundred lines in which the
// diff machinery sat below a paging control it has no relationship to.
//
// Read-only: no editing a schema, no deleting a version, no changing
// compatibility. That is most of what kafbat's equivalent page spends its
// buttons on, and it is the half kaas-ui does not have.

import { useState } from "react"
import { Link } from "@tanstack/react-router"
import { AlertTriangle, ArrowLeft, FileWarning, RotateCcw } from "lucide-react"

import { useEnvironment, useSubjectVersions, useTopics } from "@/api/client"
import type { SubjectNaming, SubjectSchema } from "@/api/types"
import { Empty, ErrorChips, Mono, Section, Spinner } from "@/components/domain"
import { PageTitle } from "@/components/page-title"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
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
import { highlight } from "@/lib/highlight"
import { cn } from "@/lib/utils"

/** How many topics to search when resolving a subject's topic. */
const PAGE = 50

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
  // Off by default: a schema is tens of lines, and on the common case — two
  // versions differing by a field — the whole file *is* the context. It earns
  // its keep on the schema with ninety fields and one changed default, which
  // is exactly where scrolling a full diff stops being reading.
  const [compact, setCompact] = useState(false)

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

  const data = detail.data
  const versions = data?.versions ?? []
  const newest = versions[versions.length - 1]

  // `!data` is what `!newest` already implied — the versions came out of it —
  // and stating it is what lets the rest of the page read the response without
  // a `?.` in front of every field.
  if (!data || !newest) {
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

  // Both ends default to the newest, so the first thing the page shows is the
  // schema as it stands rather than a diff nobody asked for. It used to open
  // on previous-vs-newest, which answered a question one version late.
  const a = versions.find((version) => version.version === left) ?? newest
  const b = versions.find((version) => version.version === right) ?? newest
  const atDefault = a.version === newest.version && b.version === newest.version
  // Textually the same once both are normalised, which is exactly the
  // condition under which the diff would have nothing to draw. Two *different*
  // versions can satisfy it — a registration that only reordered keys.
  const identical = prettyJson(a.schema) === prettyJson(b.schema)
  // Only a count now that the old-versions table is gone — the compare
  // control reaches every version, so a second list of them was two ways to
  // open the same text.
  const superseded = versions.length - 1

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
        actions={<span className="flex items-center gap-2">{back}</span>}
      />

      {detail.data?.errors.length ? (
        <ErrorChips errors={detail.data.errors} />
      ) : null}

      {/* What this subject *is*, then where it applies, then what it says.
          Both of the first two used to sit after or beside the schema text,
          which put the longest thing on the page in front of the two shortest
          — and the schema is the one part you scroll rather than read, so
          anything below it is behind a scroll. Full width, and the text gets
          the whole page once you have the frame for it. */}
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
              {detail.data?.compatibility ?? (
                <span className="text-ink-faint">—</span>
              )}
            </Fact>
          </dl>
        </Card>
      </Section>

      <AvailableOn
        envId={envId}
        registryId={registryId}
        subject={subject}
        naming={data.naming}
      />

      {/* One control, and the same one however many versions there are.
          At its default — newest against newest — there is nothing to diff, so
          it renders the schema whole. That is what the "actual version" tab
          was: this control in its default state, kept as a second place
          rendering the same text from the same data. Move either end and it
          becomes a diff.

          A single-version subject goes through it too, rather than getting a
          different heading and no controls. Both ends are v1, so it shows the
          schema — which is exactly what the special case showed — and the page
          no longer changes shape the moment somebody registers a second
          version. */}
      <Section title="Compare versions">
        <div className="mb-2 flex flex-wrap items-center gap-4 text-xs">
          <VersionSelect
            label="From"
            versions={versions}
            value={a.version}
            onChange={setLeft}
          />
          <VersionSelect
            label="To"
            versions={versions}
            value={b.version}
            onChange={setRight}
          />
          {/* Beside the selects it undoes, rather than off in the section
                header: a control that acts on two other controls belongs with
                them, and the negative margin closes the row gap so it reads as
                theirs instead of as a third peer.

                Icon only, because the two selects beside it already say which
                versions are loaded and the button's whole meaning is "not
                those". The word is not lost — it is the accessible name and
                the tooltip, both of which name the version so the destination
                is never a guess.

                Absent at the default rather than disabled: reserving the space
                would leave a hole in the row, and appearing is what makes it
                noticeable at the moment it becomes useful. */}
          {atDefault ? null : (
            <Button
              variant="ghost"
              size="icon-sm"
              className="-ml-2 self-end"
              onClick={() => {
                setLeft(undefined)
                setRight(undefined)
              }}
              aria-label={`Reset to v${newest.version}`}
              title={`Reset to the newest version, v${newest.version}`}
            >
              <RotateCcw aria-hidden />
            </Button>
          )}
          {/* Nothing to collapse when nothing changed, and a checkbox that
                would silently do nothing is worse than one that says it
                cannot. */}
          <Label
            className={cn(
              "gap-1.5 self-end pb-1 text-[12px] font-normal",
              identical ? "text-ink-faint" : "text-ink-muted"
            )}
          >
            <input
              type="checkbox"
              checked={compact && !identical}
              disabled={identical}
              onChange={(event) => setCompact(event.target.checked)}
            />
            compact
            <span
              className="text-ink-faint"
              title={
                identical
                  ? "These two are the same schema, so there are no unchanged lines to drop"
                  : "Drop the unchanged lines, keeping three either side of every change"
              }
            >
              {identical ? "(no changes)" : "(changes only)"}
            </span>
          </Label>
        </div>

        {identical ? (
          <>
            <SchemaText text={b.schema} format={b.format} />
            <References schema={b} />
            {/* Only when it is surprising. Two ends on the same version being
                identical is arithmetic; two *different* versions being
                identical is a fact about the registry worth stating. */}
            {a.version !== b.version ? (
              <p className="text-ink-muted mt-2 flex items-center gap-2 text-xs">
                <FileWarning className="size-3.5" aria-hidden />v{a.version} and
                v{b.version} are the same schema once formatting is ignored.
              </p>
            ) : null}
          </>
        ) : (
          <Diff before={a.schema} after={b.schema} compact={compact} />
        )}
      </Section>
    </>
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
  children: React.ReactNode
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

/**
 * Every cluster this schema resolves on, and where its topic actually is.
 *
 * The question the page could not answer before. A registry serves an
 * *environment*, so a subject is not a fact about one cluster — every cluster
 * that decodes against this registry resolves schema id 1 to this schema, from
 * the same handle and the same cache. The old button picked the first such
 * cluster and linked to it, which was a guess dressed as an answer: on a
 * two-cluster environment it silently hid one of them.
 *
 * Two different claims, kept apart because they can disagree:
 *
 * * **the schema resolves here** — true of every cluster in `usedBy`, by
 *   configuration, whether or not anything has ever produced against it;
 * * **the topic is here** — under the two strategies whose subject contains a
 *   topic, and only where the cluster actually holds it. A subject outlives its
 *   topic, and a link to a topic that is not there is worse than no link.
 *
 * Which strategy that is comes from the server, which has the schema and can
 * therefore take a declared record name off the end of a subject exactly. The
 * page used to strip `-value` and give up on anything else, so every
 * `TopicRecordNameStrategy` subject read as unlinkable when its topic was
 * sitting in the name.
 */
function AvailableOn({
  envId,
  registryId,
  subject,
  naming,
}: {
  envId: string
  registryId: string
  subject: string
  naming: SubjectNaming
}) {
  const environment = useEnvironment(envId)
  const usedBy =
    environment.data?.items[0]?.schemaRegistries.find(
      (entry) => entry.registry.id === registryId
    )?.usedBy ?? []
  const topic = naming.topic

  if (usedBy.length === 0) {
    return (
      <Section title="Available on">
        <Empty>
          No cluster in this environment decodes against this registry, so
          nothing resolves <Mono>{subject}</Mono> today. The subject is
          registered all the same.
        </Empty>
      </Section>
    )
  }

  return (
    <Section title="Available on">
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>cluster</TableHead>
              <TableHead>schema resolves</TableHead>
              {/* The column exists only when the subject holds a topic. A
                  `RecordNameStrategy` subject would otherwise repeat one
                  sentence down every row, saying the same thing about the
                  subject each time as though it were a fact about the cluster
                  — and it would cost a topic listing per cluster to say it. */}
              {topic === null ? null : (
                <TableHead>
                  topic <span className="font-mono">{topic}</span>
                </TableHead>
              )}
            </TableRow>
          </TableHeader>
          <TableBody>
            {usedBy.map((clusterId) =>
              topic === null ? (
                <TableRow key={clusterId}>
                  <TableCell>
                    <ClusterLink envId={envId} clusterId={clusterId} />
                  </TableCell>
                  <TableCell className="text-ok-ink text-[12px]">yes</TableCell>
                </TableRow>
              ) : (
                <ClusterRow
                  key={clusterId}
                  envId={envId}
                  clusterId={clusterId}
                  topic={topic}
                />
              )
            )}
          </TableBody>
        </Table>
      </div>
      <NamingNote naming={naming} subject={subject} />
      <p className="mt-2 text-[11px] text-ink-faint">
        Every cluster here holds the same <Mono>Arc&lt;RegistryHandle&gt;</Mono>
        , so schema id {""}
        <Mono>1</Mono> is genuinely the same schema on all of them — one set of
        decoders, one id→schema cache.
      </p>
    </Section>
  )
}

/**
 * How the subject name was read, in one line, always.
 *
 * All four cases say something, including the two that yield no topic: "there
 * is no topic in this name" and "this name follows no strategy I know" are
 * different facts about the deployment, and a reader shown a missing column
 * learns neither. Stated for the two that *do* yield one as well — under
 * `TopicRecordNameStrategy` the seam is only obvious once you are told where
 * it was found, and a link whose derivation is unexplained is a link you
 * check by hand.
 */
function NamingNote({
  naming,
  subject,
}: {
  naming: SubjectNaming
  subject: string
}) {
  const record = naming.recordName

  const body = (() => {
    switch (naming.strategy) {
      case "topicName":
        return (
          <>
            <Mono>TopicNameStrategy</Mono> — the <Mono>-value</Mono> /{" "}
            <Mono>-key</Mono> suffix is what names the topic.
          </>
        )
      case "topicRecordName":
        return (
          <>
            <Mono>TopicRecordNameStrategy</Mono> — the schema declares{" "}
            <Mono>{record}</Mono>, and what precedes it is the topic. Nothing in
            the name says where that seam is; the schema does.
          </>
        )
      case "recordName":
        return (
          <>
            <Mono>RecordNameStrategy</Mono> — this names the record, not a
            topic. The same record goes to whatever topics carry it, and which
            those are lives in the records rather than in the registry, so there
            is no topic to link to.
          </>
        )
      case "unrecognized":
        return record ? (
          <>
            No naming strategy fits: <Mono>{subject}</Mono> is neither{" "}
            <Mono>{record}</Mono>, <Mono>{`<topic>-${record}`}</Mono> nor{" "}
            <Mono>{"<topic>-value"}</Mono>, so no topic can be read out of it.
          </>
        ) : (
          <>
            The newest schema declares no name, so only a <Mono>-value</Mono> or{" "}
            <Mono>-key</Mono> suffix could have named a topic — and{" "}
            <Mono>{subject}</Mono> has neither.
          </>
        )
    }
  })()

  return <p className="mt-2 text-[11px] text-ink-faint">{body}</p>
}

/** The cluster's own page. Both row shapes need it, only one has a topic. */
function ClusterLink({
  envId,
  clusterId,
}: {
  envId: string
  clusterId: string
}) {
  return (
    <Link
      to="/environments/$envId/clusters/$clusterId"
      params={{ envId, clusterId }}
      className="font-mono hover:underline"
      style={{ color: "var(--rust-ink)" }}
    >
      {clusterId}
    </Link>
  )
}

/**
 * One cluster's row, for a subject that names a topic.
 *
 * A hook per row rather than one lookup for all of them, because `useTopics`
 * is per cluster. It costs nothing at the broker — the topic list is served
 * from the metadata snapshot — so the honest answer is worth the extra query.
 * Only rendered where there is a topic to look for, so the query is never the
 * empty search that used to fetch fifty topics to display nothing.
 */
function ClusterRow({
  envId,
  clusterId,
  topic,
}: {
  envId: string
  clusterId: string
  topic: string
}) {
  const topics = useTopics(envId, clusterId, { search: topic, limit: PAGE })
  const exists = topics.data?.items.some((entry) => entry.name === topic)

  return (
    <TableRow>
      <TableCell>
        <ClusterLink envId={envId} clusterId={clusterId} />
      </TableCell>
      <TableCell className="text-ok-ink text-[12px]">yes</TableCell>
      <TableCell className="text-[12px]">
        {topics.isLoading ? (
          <span className="text-ink-faint">·</span>
        ) : exists ? (
          <Link
            to="/environments/$envId/clusters/$clusterId/topics/$topic"
            params={{ envId, clusterId, topic }}
            className="font-mono hover:underline"
            style={{ color: "var(--rust-ink)" }}
          >
            {topic}
          </Link>
        ) : (
          <span className="text-ink-faint">absent</span>
        )}
      </TableCell>
    </TableRow>
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
  // Oldest first, so the last one is the version in force. Derived rather than
  // passed: one list, one notion of which end of it is current.
  const current = versions[versions.length - 1]?.version

  return (
    <Label className="gap-1 text-xs font-normal text-ink-faint">
      {label}
      <Select
        value={value !== undefined ? String(value) : undefined}
        onValueChange={(next) => onChange(Number(next))}
      >
        <SelectTrigger className="w-[130px]" aria-label={`${label} version`}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {versions.map((version) => (
            <SelectItem key={version.version} value={String(version.version)}>
              {/* The version, and whether it is the one in force. The schema
                  id used to ride along here and does not belong: it is a
                  registry-wide counter, so `v1 (#2)` invites reading 2 as
                  something about this subject when it is a position in a
                  sequence shared with every other subject. It is on the
                  overview, once, where it is the only number of its kind. */}
              v{version.version}
              {version.version === current ? (
                <span className="text-ink-faint">(current)</span>
              ) : null}
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
  const proto = format === "protobuf"
  const shown = proto ? text : prettyJson(text)
  return (
    <pre className="max-h-[45vh] overflow-auto rounded-md border border-line bg-surface-sunken p-3 font-mono text-[11px] leading-relaxed whitespace-pre">
      {/* Coloured by the declared format rather than by sniffing the text: the
          registry says which of the three this is, and a JSON schema that
          happens to start with a brace is not a reason to guess. */}
      {highlight(shown, proto ? "proto" : "json")}
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
function Diff({
  before,
  after,
  compact,
}: {
  before: string
  after: string
  compact: boolean
}) {
  const left = prettyJson(before).split("\n")
  const right = prettyJson(after).split("\n")
  const lines = diffLines(left, right)

  const shown: DiffRow[] = compact
    ? collapse(lines)
    : lines.map((line) => ({ line }))

  return (
    <pre className="max-h-[45vh] overflow-auto rounded-md border border-line bg-surface-sunken p-3 font-mono text-[11px] leading-relaxed whitespace-pre">
      {shown.map((row, index) =>
        row.elided !== undefined ? (
          // The count, not a bare `…`. "Twelve lines you are not being shown"
          // is a different claim from "something is hidden here", and only the
          // first tells you whether compact is costing you anything.
          <div
            key={index}
            className="text-ink-faint my-1 border-y border-dashed border-line/60 py-0.5 select-none"
          >
            {"  ⋯ "}
            {row.elided} unchanged line{row.elided === 1 ? "" : "s"}
          </div>
        ) : (
          <div
            key={index}
            className={cn(
              row.line?.kind === "added" && "bg-ok/15 text-ok-ink",
              row.line?.kind === "removed" && "bg-danger/15 text-danger"
            )}
          >
            {row.line?.kind === "added"
              ? "+"
              : row.line?.kind === "removed"
                ? "-"
                : " "}
            {row.line?.text}
          </div>
        )
      )}
    </pre>
  )
}

/** How many unchanged lines to keep either side of a change. */
const CONTEXT = 3

/** A rendered line, or a stand-in for the ones that were dropped. */
interface DiffRow {
  line?: DiffLine
  elided?: number
}

/**
 * Drop the unchanged lines, keeping a few either side of every change.
 *
 * A run is only worth eliding when it is taller than the marker replacing it:
 * collapsing two lines into "⋯ 2 unchanged lines" saves no height and costs
 * the reader the two lines. So a short gap between two changes is kept whole,
 * which also stops a dense diff turning into a ladder of markers.
 */
function collapse(lines: DiffLine[]): DiffRow[] {
  const keep = new Array<boolean>(lines.length).fill(false)
  lines.forEach((line, index) => {
    if (line.kind === "same") return
    const from = Math.max(0, index - CONTEXT)
    const to = Math.min(lines.length - 1, index + CONTEXT)
    for (let near = from; near <= to; near += 1) keep[near] = true
  })

  const out: DiffRow[] = []
  // Where the current run of dropped lines began, or -1 for "not in one".
  let start = -1
  const flush = (end: number) => {
    if (start < 0) return
    const run = end - start
    if (run <= 2) {
      for (let index = start; index < end; index += 1) {
        const line = lines[index]
        if (line) out.push({ line })
      }
    } else {
      out.push({ elided: run })
    }
    start = -1
  }

  lines.forEach((line, index) => {
    if (keep[index]) {
      flush(index)
      out.push({ line })
    } else if (start < 0) {
      start = index
    }
  })
  flush(lines.length)
  return out
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
