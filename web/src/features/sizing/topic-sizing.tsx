// The sizing tab: what this topic's configuration implies about its segments.
//
// Its own tab rather than a card on the statistics one, and the reason is a
// grant. Statistics reads every payload, so that tab is gated on
// `messages_read`; this reads two describes and no records, so it is gated
// like the configs tab. Folded into statistics it would be invisible to
// exactly the operator it is for — someone who may see a topic and its
// configuration but is not trusted with what is inside it.
//
// Everything rendered here was derived server-side, in
// `kaas-ui-core/src/sizing.rs`. That is on purpose: a threshold that exists in
// TypeScript and in Rust drifts, and the rules are the feature. The browser's
// job is to lay the findings out and to be honest about what they were read
// from — which is why the settings that fed them are on the page rather than
// one tab away.

import { useTopicSizing } from "@/api/client"
import type { Diagnostic, SettingValue, SizingReport } from "@/api/types"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  Empty,
  ErrorChips,
  HintHead,
  Section,
  Spinner,
  Stat,
} from "@/components/domain"
import { bytes, count } from "@/lib/format"

import { Ticked } from "./ticked"

export function TopicSizing({
  envId,
  clusterId,
  topic,
}: {
  envId: string
  clusterId: string
  topic: string
}) {
  const sizing = useTopicSizing(envId, clusterId, topic)
  const report = sizing.data?.items[0]

  if (sizing.isLoading) return <Spinner label={`sizing ${topic}`} />

  return (
    <div className="space-y-6">
      <ErrorChips errors={sizing.data?.errors ?? []} />
      {report ? (
        <>
          <Evidence report={report} />
          <Findings diagnostics={report.diagnostics} />
          <Inputs settings={report.settings} />
        </>
      ) : (
        <Empty>the cluster did not answer for this topic</Empty>
      )}
    </div>
  )
}

/**
 * What the findings were read from, before any of them are stated.
 *
 * Above the findings rather than below, because the first question a
 * recommendation raises is "measured on what" — and because the coverage line
 * is what makes a short fan-out visible instead of silently narrowing every
 * number under it.
 */
function Evidence({ report }: { report: SizingReport }) {
  const sizes = report.sizes
  const partial =
    sizes !== undefined && sizes.partitionsMeasured < report.partitions

  return (
    <Card>
      <CardContent className="space-y-4">
        <p className="text-[12px] text-ink-muted">
          Read from this topic's configuration and from what its partitions hold
          on disk. <strong>No records are read</strong>, so nothing here costs a
          scan — and nothing here can see inside a payload.
        </p>
        <dl className="grid grid-cols-2 gap-x-6 gap-y-3 text-[13px] sm:grid-cols-4">
          <Stat
            label="partitions"
            value={count(report.partitions)}
            hint="from cluster metadata, which is the whole list — the sizes below may cover fewer"
          />
          <Stat
            label="on disk"
            value={sizes ? bytes(sizes.logicalBytes) : "—"}
            note={sizes ? "leader copies" : "not read"}
            hint="the leader's copy of each measured partition, summed — the figure segment.bytes is comparable with, unlike the replicated total on the overview"
          />
          <Stat
            label="largest partition"
            value={sizes ? bytes(sizes.largestPartitionBytes) : "—"}
            hint="the leader's copy of the biggest measured partition"
          />
          <Stat
            label="smallest partition"
            value={sizes ? bytes(sizes.smallestPartitionBytes) : "—"}
            hint="the leader's copy of the smallest — a partition well under one segment is one whose segments are not rolling on size"
          />
        </dl>
        {sizes === undefined ? (
          <p className="text-warn-ink text-[12px]">
            <strong>No partition sizes.</strong> Log directories were not asked,
            or did not answer — the errors above say which. Every finding below
            is what the configuration allows rather than what the partitions
            show, which is why none of them is a warning.
          </p>
        ) : null}
        {partial ? (
          <p className="text-warn-ink text-[12px]">
            <strong>Partial coverage.</strong> {sizes.partitionsMeasured} of{" "}
            {report.partitions} partitions were measured — a broker in the
            fan-out did not answer. The findings below describe the ones that
            did.
          </p>
        ) : null}
      </CardContent>
    </Card>
  )
}

function Findings({ diagnostics }: { diagnostics: Diagnostic[] }) {
  return (
    <Section title="Findings">
      {diagnostics.length === 0 ? (
        <Empty>
          nothing to flag: retention, the segment roll and the offset index
          agree with each other here
        </Empty>
      ) : (
        <div className="space-y-3">
          {diagnostics.map((diagnostic) => (
            <Finding key={diagnostic.code} diagnostic={diagnostic} />
          ))}
        </div>
      )}
    </Section>
  )
}

/**
 * One finding, with its derivation next to it rather than behind a disclosure.
 *
 * A number without its derivation is not actionable — the reader cannot tell
 * whether the assumption behind it matches their topic — and a detail nobody
 * expands is a detail nobody reads.
 */
function Finding({ diagnostic }: { diagnostic: Diagnostic }) {
  const warns = diagnostic.severity === "warn"
  return (
    <Card className={warns ? "border-warn" : undefined}>
      <CardContent className="space-y-2">
        <div className="flex flex-wrap items-baseline gap-2">
          <Badge variant={warns ? "destructive" : "secondary"}>
            {warns ? "warning" : "note"}
          </Badge>
          <span className="text-[13px] font-semibold">
            <Ticked text={diagnostic.summary} />
          </span>
        </div>
        <p className="text-[12px] leading-relaxed text-ink-muted">
          <Ticked text={diagnostic.detail} />
        </p>
        {diagnostic.partitions && diagnostic.partitions.length > 0 ? (
          <p className="text-[11px] text-ink-faint">
            partitions:{" "}
            <span className="font-mono">
              {diagnostic.partitions.join(", ")}
            </span>
          </p>
        ) : null}
      </CardContent>
    </Card>
  )
}

/**
 * The settings the findings reasoned from.
 *
 * `set` versus `default` is the column that matters: a value nobody chose is
 * a value nobody has checked against this topic, and most of the findings
 * above are about exactly that kind of inheritance.
 */
function Inputs({ settings }: { settings: SettingValue[] }) {
  if (settings.length === 0) return null
  return (
    <Section title="What this was read from">
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <HintHead
                label="key"
                hint="the settings these findings depend on — the configs tab has the rest"
              />
              <HintHead
                label="value"
                hint="what is in force here, effective rather than as written"
              />
              <HintHead
                label="source"
                hint="whether someone set it on this topic, or it was inherited"
              />
            </TableRow>
          </TableHeader>
          <TableBody>
            {settings.map((setting) => (
              <TableRow key={setting.name}>
                <TableCell className="font-mono">{setting.name}</TableCell>
                <TableCell className="font-mono">
                  {setting.value ?? "—"}
                </TableCell>
                <TableCell>
                  {setting.isExplicit ? (
                    <Badge variant="outline">set here</Badge>
                  ) : (
                    <span className="text-[12px] text-ink-faint">default</span>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </Section>
  )
}
