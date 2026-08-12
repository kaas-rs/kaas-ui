import { Link } from "@tanstack/react-router"
import { ArrowRight } from "lucide-react"

import { useSubjectDetails } from "@/api/client"
import type { NamingStrategy } from "@/api/types"
import { Section, Stat } from "@/components/domain"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"

/**
 * The schema this topic's records are written against, where there is one.
 *
 * Absent by default, and that is the common case: most topics carry no schema,
 * most environments reference no registry, and a card that says "no schema"
 * on every topic in the fleet is a card nobody reads. It appears only when a
 * subject genuinely names *this* topic.
 *
 * Which subjects those are comes from the server, per row, and it has to:
 * matching `orders-` here would claim `orders-eu-value` for the topic
 * `orders`, and under `TopicRecordNameStrategy` the seam between topic and
 * record is in the *schema* rather than in the name. The registry search is a
 * substring — it casts wide — and `naming.topic` is what narrows it to an
 * answer. See `SubjectNaming`.
 *
 * Both sides are listed when both exist. A key schema and a value schema are
 * two subjects and two schemas, and picking one to show would be picking the
 * one that happened to sort first.
 */
export function TopicSchemas({
  envId,
  registryId,
  topic,
}: {
  envId: string
  registryId: string | null
  topic: string
}) {
  // `details` because the id, the format and the version are the card, and
  // because naming can only be read exactly once the newest schema is in
  // hand. The search is the topic name, so the page described is the handful
  // of subjects that mention it rather than the registry.
  const subjects = useSubjectDetails(envId, registryId ?? "", {
    search: topic,
    limit: SUBJECT_SEARCH,
  })

  if (!registryId) return null

  const rows = (subjects.data?.subjects ?? [])
    .filter((row) => row.naming.topic === topic)
    // `-value` first: it is the one people mean by "the schema of this topic",
    // and a key schema is the exception that should read as an addition.
    .sort((a, b) => rank(a.subject) - rank(b.subject))

  if (!rows.length) return null

  return (
    <Section title={rows.length === 1 ? "Schema" : "Schemas"}>
      <div className="space-y-3">
        {rows.map((row) => (
          <Card key={row.subject}>
            <CardContent>
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-mono text-[13px]">{row.subject}</span>
                    {row.format ? (
                      <Badge variant="outline">{row.format}</Badge>
                    ) : null}
                    <Badge variant="outline" className="text-ink-muted">
                      {side(row.subject, row.naming.strategy)}
                    </Badge>
                  </div>
                  <dl className="mt-3 grid grid-cols-2 gap-x-6 gap-y-3 text-[13px] sm:grid-cols-4">
                    <Stat
                      label="schema id"
                      value={row.id === null ? "—" : `#${row.id}`}
                      note="what the wire format carries"
                    />
                    <Stat
                      label="version"
                      value={row.version === null ? "—" : `v${row.version}`}
                    />
                    <Stat
                      label="compatibility"
                      value={row.compatibility ?? "—"}
                      note={
                        row.compatibilityInherited
                          ? "the registry's default"
                          : undefined
                      }
                    />
                    {row.naming.recordName ? (
                      <Stat label="record" value={row.naming.recordName} />
                    ) : null}
                  </dl>
                </div>

                {/* To the subject, not to the registry listing: the reader is
                    already on the thing the subject is about, and the page
                    they want is the schema text. */}
                <Button variant="outline" size="sm" asChild>
                  <Link
                    to="/environments/$envId/schema-registries/$registryId/subjects/$subject"
                    params={{ envId, registryId, subject: row.subject }}
                  >
                    open schema
                    <ArrowRight aria-hidden />
                  </Link>
                </Button>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>
    </Section>
  )
}

/** How many subjects mentioning the topic to describe. */
const SUBJECT_SEARCH = 50

/** `-value` before `-key` before anything a record strategy named. */
function rank(subject: string): number {
  if (subject.endsWith("-value")) return 0
  if (subject.endsWith("-key")) return 1
  return 2
}

/**
 * Which half of the record this subject decodes.
 *
 * Only `TopicNameStrategy` says: its whole suffix is the answer. Under
 * `{topic}-{record}` the subject names a record and nothing about the name
 * says which side carries it, so the badge says that instead of guessing.
 */
function side(subject: string, strategy: NamingStrategy): string {
  if (strategy !== "topicName") return "record"
  return subject.endsWith("-key") ? "key" : "value"
}
