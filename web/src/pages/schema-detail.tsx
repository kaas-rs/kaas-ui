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
import { AlertTriangle, ArrowLeft, FileWarning } from "lucide-react"

import { useEnvironment, useSubjectVersions, useTopics } from "@/api/client"
import type { SubjectSchema } from "@/api/types"
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
import { cn } from "@/lib/utils"

/** How many topics to search when resolving a subject's topic. */
const PAGE = 50

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
        actions={<span className="flex items-center gap-2">{back}</span>}
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

      <AvailableOn envId={envId} registryId={registryId} subject={subject} />

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
 * * **the topic is here** — only under `TopicNameStrategy`, and only where
 *   the cluster actually holds it. A subject outlives its topic, and a link to
 *   a topic that is not there is worse than no link.
 */
function AvailableOn({
  envId,
  registryId,
  subject,
}: {
  envId: string
  registryId: string
  subject: string
}) {
  const environment = useEnvironment(envId)
  const usedBy =
    environment.data?.items[0]?.schemaRegistries.find(
      (entry) => entry.registry.id === registryId
    )?.usedBy ?? []
  const topic = topicOf(subject)

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
              <TableHead>
                {topic ? (
                  <>
                    topic <span className="font-mono">{topic}</span>
                  </>
                ) : (
                  "topic"
                )}
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {usedBy.map((clusterId) => (
              <ClusterRow
                key={clusterId}
                envId={envId}
                clusterId={clusterId}
                topic={topic}
              />
            ))}
          </TableBody>
        </Table>
      </div>
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
 * One cluster's row: the schema, and the topic if this is that kind of subject.
 *
 * A hook per row rather than one lookup for all of them, because `useTopics`
 * is per cluster. It costs nothing at the broker — the topic list is served
 * from the metadata snapshot — so the honest answer is worth the extra query.
 */
function ClusterRow({
  envId,
  clusterId,
  topic,
}: {
  envId: string
  clusterId: string
  topic: string | null
}) {
  const topics = useTopics(envId, clusterId, {
    search: topic ?? "",
    limit: PAGE,
  })
  const exists = topic
    ? topics.data?.items.some((entry) => entry.name === topic)
    : undefined

  return (
    <TableRow>
      <TableCell>
        <Link
          to="/environments/$envId/clusters/$clusterId"
          params={{ envId, clusterId }}
          className="font-mono hover:underline"
          style={{ color: "var(--rust-ink)" }}
        >
          {clusterId}
        </Link>
      </TableCell>
      <TableCell className="text-ok-ink text-[12px]">yes</TableCell>
      <TableCell className="text-[12px]">
        {topic === null ? (
          <span
            className="text-ink-faint"
            title="Only TopicNameStrategy names a topic a subject can be undone into"
          >
            not derivable from this subject
          </span>
        ) : topics.isLoading ? (
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
