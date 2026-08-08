// The schema browser.
//
// A view of a **registry**, reached from a cluster. Two clusters that
// reference `dev` show the same subject list, so every screen here names the
// registry that is answering rather than implying the subjects belong to the
// cluster whose nav you arrived through.
//
// Read-only, like everything else: no registering, no compatibility changes.

import { useState } from "react"
import { AlertTriangle, FileWarning, Search } from "lucide-react"

import { useSubjects, useSubjectVersions } from "@/api/client"
import type { RegistryCard, SubjectSchema } from "@/api/types"
import { Empty, ErrorChips, Mono, Section, Spinner } from "@/components/domain"
import { PageTitle } from "@/components/page-title"
import { Badge } from "@/components/ui/badge"
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
import { cn } from "@/lib/utils"

export function Schemas({ clusterId }: { clusterId: string }) {
  const subjects = useSubjects(clusterId)
  const [selected, setSelected] = useState<string>()
  const [query, setQuery] = useState("")

  if (subjects.isLoading) return <Spinner label="reading the registry" />

  // A failed request is not an absent registry. Rendering the "add
  // `schema_registry:` to your config" panel over a transient 5xx would send
  // an operator to edit configuration that is already correct.
  if (subjects.error) {
    return (
      <>
        <PageTitle
          title="Schemas"
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
          title="Schemas"
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

  const all = subjects.data?.subjects ?? []
  const shown = query.trim()
    ? all.filter((subject) =>
        subject.toLowerCase().includes(query.trim().toLowerCase())
      )
    : all

  return (
    <>
      <PageTitle
        title="Schemas"
        subtitle={`${all.length} subject${all.length === 1 ? "" : "s"} in ${registry.name}`}
      />

      <RegistryBanner registry={registry} />

      <div className="grid gap-4 lg:grid-cols-[minmax(220px,1fr)_2.4fr]">
        <Card className="flex min-h-0 flex-col p-0">
          <div className="border-b border-line p-2">
            <div className="relative">
              <Search
                className="absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-ink-faint"
                aria-hidden
              />
              <Input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="Filter subjects…"
                aria-label="Filter subjects"
                className="pl-7"
              />
            </div>
          </div>
          {shown.length ? (
            <ul className="max-h-[60vh] overflow-auto p-1">
              {shown.map((subject) => (
                <li key={subject}>
                  <button
                    type="button"
                    onClick={() => setSelected(subject)}
                    className={cn(
                      "w-full truncate rounded px-2 py-1.5 text-left font-mono text-xs hover:bg-surface-raised",
                      selected === subject && "bg-rust/25"
                    )}
                    title={subject}
                  >
                    {subject}
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <div className="p-4">
              <Empty>
                {all.length
                  ? "No subject matches that."
                  : "This registry holds no subjects."}
              </Empty>
            </div>
          )}
        </Card>

        <div className="min-w-0">
          {selected ? (
            <SubjectPanel clusterId={clusterId} subject={selected} />
          ) : (
            <Empty>
              Pick a subject. A subject on two clusters is one subject —{" "}
              <Mono>TopicNameStrategy</Mono> turns topic <Mono>orders</Mono>{" "}
              into <Mono>orders-value</Mono> whichever cluster in the
              environment produced it.
            </Empty>
          )}
        </div>
      </div>
    </>
  )
}

/**
 * Which registry is answering, and whether it is.
 *
 * Always rendered, even when everything is fine: two clusters showing the same
 * subjects is the registry doing its job, and a reader has to be able to see
 * that rather than wonder about it.
 */
function RegistryBanner({ registry }: { registry: RegistryCard }) {
  const broken =
    registry.status === "unreachable" || registry.status === "misconfigured"
  return (
    <Card
      className={cn(
        "mb-4 p-3",
        broken && registry.status === "misconfigured" && "border-danger/50",
        broken && registry.status === "unreachable" && "border-warn-ink/50"
      )}
    >
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <Badge variant="outline">{registry.id}</Badge>
        <span className="font-medium">{registry.name}</span>
        <Mono>{registry.url}</Mono>
        <span className="flex-1" />
        <Badge
          variant="outline"
          className={cn(
            registry.status === "ready" && "border-ok/50 text-ok-ink",
            registry.status === "unreachable" &&
              "border-warn-ink/50 text-warn-ink",
            registry.status === "misconfigured" &&
              "border-danger/50 text-danger"
          )}
        >
          {registry.status}
        </Badge>
      </div>
      {registry.error ? (
        <p
          className={cn(
            "mt-2 flex items-start gap-2 text-[11px]",
            registry.status === "misconfigured"
              ? "text-danger"
              : "text-warn-ink"
          )}
        >
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          {registry.error}
        </p>
      ) : null}
      <p className="mt-2 text-[11px] text-ink-faint">
        A registry serves an environment. Every cluster that names{" "}
        <Mono>{registry.id}</Mono> sees exactly these subjects, resolves schema
        ids against this registry, and shares one cache of them.
      </p>
    </Card>
  )
}

function SubjectPanel({
  clusterId,
  subject,
}: {
  clusterId: string
  subject: string
}) {
  const detail = useSubjectVersions(clusterId, subject)
  const [left, setLeft] = useState<number>()
  const [right, setRight] = useState<number>()

  if (detail.isLoading) return <Spinner label="reading the subject" />
  if (detail.error)
    return (
      <p className="text-xs text-danger">{(detail.error as Error).message}</p>
    )

  const versions = detail.data?.versions ?? []
  if (!versions.length) {
    // The response's own registry card is what tells "the subject holds
    // nothing" apart from "the registry could not answer" — the page-level
    // banner may still be showing a cached `ready` from before it went down.
    const fault =
      detail.data?.registry && detail.data.registry.status !== "ready"
        ? detail.data.registry
        : null
    return (
      <div className="space-y-3">
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
      </div>
    )
  }

  const newest = versions[versions.length - 1]
  const previous = versions[versions.length - 2]
  const a = versions.find((v) => v.version === left) ?? previous ?? newest
  const b = versions.find((v) => v.version === right) ?? newest

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <h2 className="font-mono text-sm font-medium">{subject}</h2>
        <Badge variant="outline">
          {versions.length} version{versions.length === 1 ? "" : "s"}
        </Badge>
        {newest ? <Badge variant="outline">{newest.format}</Badge> : null}
        {detail.data?.compatibility ? (
          <Badge
            variant="outline"
            title="Compatibility mode, as the registry reports it"
          >
            {detail.data.compatibility}
          </Badge>
        ) : null}
      </div>

      {detail.data?.errors.length ? (
        <ErrorChips errors={detail.data.errors} />
      ) : null}

      {newest ? (
        <Section
          title={`Version ${newest.version} — schema id ${newest.id}`}
          actions={<Badge variant="outline">#{newest.id}</Badge>}
        >
          <SchemaText text={newest.schema} format={newest.format} />
          <References schema={newest} />
        </Section>
      ) : null}

      {versions.length > 1 ? (
        <Section title="Diff">
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
    </div>
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
