import type { SubjectNaming } from "@/api/types"
import { Mono } from "@/components/domain"

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
export function NamingNote({
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
