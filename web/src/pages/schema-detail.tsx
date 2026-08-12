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

import { Link } from "@tanstack/react-router"
import { AlertTriangle, ArrowLeft } from "lucide-react"

import { useSubjectVersions } from "@/api/client"
import { Empty, ErrorChips, Mono, Spinner } from "@/components/domain"
import { PageTitle } from "@/components/page-title"
import { Button } from "@/components/ui/button"
import { AvailableOn } from "@/features/schemas/available-on"
import { CompareVersions } from "@/features/schemas/compare-versions"
import { SubjectOverview } from "@/features/schemas/subject-overview"
import { cn } from "@/lib/utils"

export function SchemaDetailPage({
  envId,
  registryId,
  subject,
}: {
  envId: string
  registryId: string
  subject: string
}) {
  const detail = useSubjectVersions(envId, registryId, subject)

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
      <SubjectOverview
        newest={newest}
        versions={versions}
        compatibility={data.compatibility}
      />

      <AvailableOn
        envId={envId}
        registryId={registryId}
        subject={subject}
        naming={data.naming}
      />

      <CompareVersions versions={versions} newest={newest} />
    </>
  )
}
