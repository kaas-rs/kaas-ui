import { useState } from "react"
import { FileWarning, RotateCcw } from "lucide-react"

import type { SubjectSchema } from "@/api/types"
import { Section } from "@/components/domain"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import { cn } from "@/lib/utils"

import { Diff } from "./diff"
import { prettyJson } from "./pretty-json"
import { References } from "./references"
import { SchemaText } from "./schema-text"
import { VersionSelect } from "./version-select"

/* One control, and the same one however many versions there are.
   At its default — newest against newest — there is nothing to diff, so
   it renders the schema whole. That is what the "actual version" tab
   was: this control in its default state, kept as a second place
   rendering the same text from the same data. Move either end and it
   becomes a diff.

   A single-version subject goes through it too, rather than getting a
   different heading and no controls. Both ends are v1, so it shows the
   schema — which is exactly what the special case showed — and the page
   no longer changes shape the moment somebody registers a second
   version. */
export function CompareVersions({
  versions,
  newest,
}: {
  versions: SubjectSchema[]
  newest: SubjectSchema
}) {
  const [left, setLeft] = useState<number>()
  const [right, setRight] = useState<number>()
  // Off by default: a schema is tens of lines, and on the common case — two
  // versions differing by a field — the whole file *is* the context. It earns
  // its keep on the schema with ninety fields and one changed default, which
  // is exactly where scrolling a full diff stops being reading.
  const [compact, setCompact] = useState(false)

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

  return (
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
              <FileWarning className="size-3.5" aria-hidden />v{a.version} and v
              {b.version} are the same schema once formatting is ignored.
            </p>
          ) : null}
        </>
      ) : (
        <Diff before={a.schema} after={b.schema} compact={compact} />
      )}
    </Section>
  )
}
