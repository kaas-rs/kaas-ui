// The sizing advisor, on the back of the scan that measured it.
//
// It lives here and not on a tab of its own for one reason: a recommendation
// about partitions or segments needs a *write rate*, and the only thing in
// this product that measures one is the analysis above. Metadata knows what a
// topic is configured as; only the fold knows what it carries. So the advice
// arrives attached to the analysis result — `TopicAnalysis.sizing` — and
// everything below is laid out, never derived: the arithmetic is in
// `kaas-ui-core/src/sizing/`, because a threshold that exists in Rust and
// again in TypeScript drifts.
//
// Seven profiles, all computed server-side in one pass, so switching between
// them costs no request. The card opens on the one the signals point at and
// says why it did — the alternative is a card that arrives pre-set to a claim
// about the topic that nothing in the data supports.

import type {
  Change,
  Diagnostic,
  Profile,
  ProfileAdvice,
  Recommendation,
  SizingAdvice as Advice,
} from "@/api/types"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
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
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Empty, HintHead, Section, Stat } from "@/components/domain"
import { bytes, count } from "@/lib/format"

import { Ticked } from "./ticked"

export function SizingAdviceSections({
  advice,
  profile,
  onProfile,
}: {
  advice: Advice
  /** From the URL, so a recommendation can be pasted into a ticket. */
  profile: Profile | undefined
  onProfile(profile: Profile): void
}) {
  const chosen =
    advice.profiles.find((entry) => entry.profile === profile) ??
    advice.profiles.find((entry) => entry.profile === advice.suggested) ??
    advice.profiles[0]

  return (
    <>
      {/* No heading: the card opens the sub-page and its own first line
          already says these are measured rates. A word above it saying so
          is the heading restating the card. */}
      <section className="mb-8">
        <MeasuredCard advice={advice} />
      </section>

      {/* One card, not four. Picking a profile, reading what it optimises
          for, reading the rows it produced and checking the assumptions
          underneath them are one act — split across cards, the reader has to
          remember which profile the table two cards down belongs to. */}
      <Section title="Sizing advice">
        <Card>
          <CardContent className="space-y-5">
            <ProfileSelect
              profiles={advice.profiles}
              chosen={chosen?.profile}
              suggested={advice.suggested}
              onProfile={onProfile}
            />
            {chosen ? (
              <>
                <Chosen advice={advice} chosen={chosen} />
                <Rows rows={chosen.rows} />
              </>
            ) : (
              <Empty>the server offered no profiles</Empty>
            )}
            <Assumptions advice={advice} />
          </CardContent>
        </Card>
      </Section>

      <Section title="Configuration findings">
        <Findings diagnostics={advice.diagnostics} />
      </Section>
    </>
  )
}

/**
 * The measurement every recommendation below is built on.
 *
 * Above the advice rather than below it, because the first question a
 * recommendation raises is "measured on what" — and because the peak is an
 * *hourly* figure, which is the one caveat that changes how the numbers
 * should be read.
 */
function MeasuredCard({ advice }: { advice: Advice }) {
  const m = advice.measured
  const perSec = (value: number | null) =>
    value === null ? "—" : `${bytes(value)}/s`
  const times = (value: number | null) =>
    value === null ? "—" : `${value.toFixed(1)}×`
  const percent = (value: number | null) =>
    value === null ? "—" : `${Math.round(value * 100)}%`

  return (
    <Card>
      <CardContent className="space-y-4">
        <dl className="grid grid-cols-2 gap-x-6 gap-y-3 text-[13px] sm:grid-cols-4">
          <Stat
            label="peak write rate"
            value={perSec(m.peakBytesPerSec)}
            note="busiest hour"
            hint="the busiest hourly bucket, averaged across that hour — a burst inside one hour is invisible at this resolution"
          />
          <Stat
            label="mean write rate"
            value={perSec(m.meanBytesPerSec)}
            hint="across the whole window the scanned records span"
          />
          <Stat
            label="peak / mean"
            value={times(m.peakToMean)}
            tone={
              m.peakToMean !== null &&
              m.peakToMean >= advice.assumptions.burstRatio
                ? "warn"
                : undefined
            }
            hint="above the burst ratio, one number cannot size the topic: partitions want the peak and segments want the mean"
          />
          <Stat
            label="bytes per record"
            value={m.bytesPerRecord === null ? "—" : bytes(m.bytesPerRecord)}
            hint={m.bytesPerRecordSource}
          />
          <Stat
            label="partition skew"
            value={times(m.partitionSkew)}
            hint="the busiest partition's records over the mean partition's — more partitions never fixes this, the key does"
          />
          <Stat
            label="keyed records"
            value={percent(m.keyedFraction)}
            hint="records carrying a key; these are the ones a partition-count change reorders"
          />
          <Stat
            label="distinct keys"
            value={percent(m.distinctKeyFraction)}
            note="estimate"
            hint="distinct keys over records, from the same sketch the totals use — compaction headroom"
          />
          <Stat
            label="compression"
            value={times(m.compressionRatio)}
            hint="payload produced over bytes on disk; near 1× means little is being compressed"
          />
        </dl>
        {!m.complete ? (
          <p className="text-warn-ink text-[12px]">
            <strong>Measured on a sample.</strong> The scan stopped early, so
            every rate above describes the {count(m.records)} records it read
            from the beginning of the topic rather than the whole retained log.
            Run it uncapped before acting on a partition count.
          </p>
        ) : null}
        {m.clock === "createTime" || m.clock === "mixed" ? (
          <p className="text-warn-ink text-[12px]">
            <strong>Rates are only as good as the clock.</strong> These
            timestamps are <span className="font-mono">{m.clock}</span>, set by
            the producer — skew between producers moves records between hourly
            buckets and distorts the peak in ways that still look plausible. A
            topic on <span className="font-mono">logAppendTime</span> has no
            such gap.
          </p>
        ) : null}
      </CardContent>
    </Card>
  )
}

/**
 * The profile picker.
 *
 * A select rather than seven chips: these are alternatives to be read one at
 * a time, not filters to be toggled, and a row of seven long labels is most
 * of a card spent on options the reader is not taking. The one the topic's
 * own signals point at is marked in the list, so choosing differently is a
 * decision made against something rather than in the dark.
 */
function ProfileSelect({
  profiles,
  chosen,
  suggested,
  onProfile,
}: {
  profiles: ProfileAdvice[]
  chosen: Profile | undefined
  suggested: Profile
  onProfile(profile: Profile): void
}) {
  return (
    <Label className="text-ink-faint gap-2 text-xs font-normal">
      optimise for
      <Select
        value={chosen}
        onValueChange={(next) => onProfile(next as Profile)}
      >
        <SelectTrigger
          className="w-full sm:w-[17rem]"
          aria-label="sizing profile"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {profiles.map((entry) => (
            <SelectItem key={entry.profile} value={entry.profile}>
              {entry.label}
              {entry.profile === suggested ? (
                <span className="text-ink-faint">· signal</span>
              ) : null}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Label>
  )
}

function Chosen({ advice, chosen }: { advice: Advice; chosen: ProfileAdvice }) {
  return (
    <div className="space-y-2 text-[12px] leading-relaxed">
      <p className="text-ink-muted">
        <Ticked text={chosen.optimises} />
      </p>
      <p className="text-ink-faint">
        <span className="text-ink-muted font-medium">signal: </span>
        <Ticked text={chosen.signal} />
      </p>
      {chosen.profile === advice.suggested ? (
        <p className="text-ink-faint">
          <span className="text-ink-muted font-medium">opened here: </span>
          <Ticked text={advice.suggestedBecause} />
        </p>
      ) : null}
    </div>
  )
}

/**
 * The recommendations.
 *
 * `why` is the column that makes this a tool rather than a number generator:
 * a reader cannot tell whether an assumption matches their topic unless they
 * can see which one was used.
 */
function Rows({ rows }: { rows: Recommendation[] }) {
  return (
    // Inset rather than outlined. A bordered table on a raised card is
    // raised-on-raised — the same value either side of a hairline, which is
    // the least contrast the palette can produce. Sunken puts it a step
    // *below* the card it sits in, which is what it is, and the row hover
    // then has somewhere to go: up, to the card's own value.
    //
    // Darkening the ground costs the text contrast it gains on the edges,
    // which is why `why` is full ink here rather than muted — on light,
    // muted ink on the sunken value is the weakest pairing on the page.
    <div className="border-line-strong bg-surface-sunken overflow-hidden rounded-md border">
      <Table>
        <TableHeader className="[&_tr]:border-line-strong">
          <TableRow className="hover:bg-transparent">
            <HintHead
              label="setting"
              hint="spelled as the broker spells it, except the replication factor, which is not a topic config at all"
            />
            <HintHead
              label="current"
              hint="what is in force, with (default) where nobody set it"
            />
            <HintHead
              label="recommended"
              hint="what this profile would set; blank where the measurement did not support a number"
            />
            <HintHead
              label="why"
              hint="the derivation, naming every input it used and where each came from"
            />
          </TableRow>
        </TableHeader>
        <TableBody className="[&_tr]:border-line-strong">
          {rows.map((row) => (
            <TableRow key={row.setting} className="hover:bg-surface-raised">
              <TableCell className="font-mono align-top whitespace-nowrap">
                {row.setting}
              </TableCell>
              <TableCell className="font-mono align-top whitespace-nowrap">
                {row.current ?? "—"}
              </TableCell>
              <TableCell className="align-top whitespace-nowrap">
                <span className="font-mono">{row.recommended ?? "—"}</span>
                <ChangeBadge change={row.change} />
              </TableCell>
              <TableCell className="max-w-prose align-top text-[12px] leading-relaxed">
                <Ticked text={row.why} />
                {row.caution ? (
                  <p className="text-warn-ink mt-1.5">
                    <strong>One-way: </strong>
                    <Ticked text={row.caution} />
                  </p>
                ) : null}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

function ChangeBadge({ change }: { change: Change }) {
  if (change === "keep") return null
  const label = { increase: "raise", decrease: "lower", review: "look" }[change]
  // Outlined, not filled: `secondary` is the sunken surface, which is the
  // ground this table now sits on — a filled badge there is a badge nobody
  // can see. An edge reads against any of the three surfaces.
  return (
    <Badge variant="outline" className="border-line-strong ml-2">
      {label}
    </Badge>
  )
}

/**
 * The inputs that are not measurements.
 *
 * Rendered beside the advice rather than hidden behind it: a sizing tool that
 * does not show its assumptions is worse than no tool, and the per-partition
 * capacities in particular are conservative guesses that span an order of
 * magnitude in the published figures.
 */
function Assumptions({ advice }: { advice: Advice }) {
  const a = advice.assumptions
  const items: Array<[string, string]> = [
    [
      "produce capacity",
      `${a.produceMibPerSecPerPartition} MiB/s per partition`,
    ],
    ["consume capacity", `${a.consumeMibPerSecPerConsumer} MiB/s per consumer`],
    ["headroom", `${a.headroomPercent}% of the measured peak`],
    ["segments per window", `${a.segmentsPerRetention}`],
    ["compacted roll", `${a.compactedRollMinutes} min`],
    ["retention overshoot", `${a.overshootPercent}%`],
    ["smallest segment", `${a.minSegmentMib} MiB`],
    ["target replicas", `${a.targetReplication}`],
    ["precision window", `${a.precisionWindowHours} h`],
  ]
  return (
    <div className="space-y-3 border-t pt-4">
      <p className="text-ink-muted text-[12px] leading-relaxed">
        <strong>Assumed, not measured.</strong> Nothing in a log says what one
        partition can absorb or what one consumer keeps up with, so these are
        defaults — deliberately conservative, which makes every partition
        recommendation above err high rather than low. The cluster holds{" "}
        {advice.topology.brokers} broker(s), which is what bounds the replica
        count.
      </p>
      <dl className="grid grid-cols-2 gap-x-6 gap-y-1.5 text-[12px] sm:grid-cols-4">
        {items.map(([label, value]) => (
          <div key={label}>
            <dt className="text-ink-faint">{label}</dt>
            <dd className="font-mono">{value}</dd>
          </div>
        ))}
      </dl>
    </div>
  )
}

/**
 * The configuration lint: profile-independent, and needs no scan at all.
 *
 * Severity here is confidence rather than importance — a warning means the
 * partitions show the configuration's implication actually happening.
 */
function Findings({ diagnostics }: { diagnostics: Diagnostic[] }) {
  if (diagnostics.length === 0) {
    return (
      <Empty>
        nothing to flag: retention, the segment roll and the offset index agree
        with each other here
      </Empty>
    )
  }
  return (
    <div className="space-y-3">
      {diagnostics.map((diagnostic) => {
        const warns = diagnostic.severity === "warn"
        return (
          <Card
            key={diagnostic.code}
            className={warns ? "border-warn" : undefined}
          >
            <CardContent className="space-y-2">
              <div className="flex flex-wrap items-baseline gap-2">
                <Badge variant={warns ? "destructive" : "secondary"}>
                  {warns ? "warning" : "note"}
                </Badge>
                <span className="text-[13px] font-semibold">
                  <Ticked text={diagnostic.summary} />
                </span>
              </div>
              <p className="text-ink-muted text-[12px] leading-relaxed">
                <Ticked text={diagnostic.detail} />
              </p>
              {diagnostic.partitions && diagnostic.partitions.length > 0 ? (
                <p className="text-ink-faint text-[11px]">
                  partitions:{" "}
                  <span className="font-mono">
                    {diagnostic.partitions.join(", ")}
                  </span>
                </p>
              ) : null}
            </CardContent>
          </Card>
        )
      })}
    </div>
  )
}
