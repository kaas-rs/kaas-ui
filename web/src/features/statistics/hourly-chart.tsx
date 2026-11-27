import type { AnalysisStats } from "@/api/types"
import { count } from "@/lib/format"

/**
 * Records per hour, as a single-series line on a **linear time axis**.
 *
 * The series is zero-filled: an hour nothing was written to is a point at
 * zero in its true position, because a line drawn only through the non-empty
 * hours would bridge a quiet night as if it never happened — the one lie a
 * write-rate chart must not tell. One hue from the design system's chart
 * ramp; a single series needs no legend, the section title names it. The
 * caption says which clock is being plotted, because `createTime` and
 * `logAppendTime` can disagree by however long a producer buffers.
 *
 * Hover is a column per point, wider than the line, with the hour and its
 * count — the mark itself is too thin to be a hit target.
 *
 * **The zero-fill is bounded by points, not by hours.** The backend caps the
 * hours it holds, but filling the gaps between the first and the last is a
 * function of their *distance*: one record stamped `0` beside a week of real
 * ones spans 56 years, and filling that hour by hour built half a million
 * SVG nodes and froze the tab. So a span wider than `MAX_POINTS` hours is
 * drawn with each point averaging a run of hours — still records per hour,
 * so the axis keeps its unit — and the caption says how many.
 */
export function HourlyChart({
  stats,
  timeZone,
  clock,
}: {
  stats: AnalysisStats
  timeZone: string
  clock: string | null
}) {
  const HOUR = 3_600_000
  // One point per unit of the viewBox's width: more would be sub-pixel.
  const MAX_POINTS = 800
  const WIDTH = 800
  const HEIGHT = 190
  const PLOT = { left: 46, right: 8, top: 8, bottom: 28 }
  const plotWidth = WIDTH - PLOT.left - PLOT.right
  const plotHeight = HEIGHT - PLOT.top - PLOT.bottom

  const hours = stats.hourlyMsgCounts
  if (hours.length === 0) return null
  const first = hours[0]?.hourStart ?? 0
  const last = hours[hours.length - 1]?.hourStart ?? first
  const spanHours = Math.max(1, Math.round((last - first) / HOUR) + 1)

  // Zero-filled, in time order, and never more than MAX_POINTS long: each
  // point covers `per` hours and plots their mean, so a quiet hour still
  // pulls its point towards zero rather than being bridged over.
  const per = Math.ceil(spanHours / MAX_POINTS)
  const width = per * HOUR
  const points = Math.ceil(spanHours / per)
  const sums = new Array<number>(points).fill(0)
  for (const hour of hours) {
    const index = Math.floor((hour.hourStart - first) / width)
    if (index >= 0 && index < points)
      sums[index] = (sums[index] ?? 0) + hour.count
  }
  const series = sums.map((sum, index) => ({
    hourStart: first + index * width,
    count: sum / per,
  }))
  const peak = Math.max(...series.map((point) => point.count))

  const step = plotWidth / points
  const x = (hourStart: number) =>
    PLOT.left + ((hourStart - first) / width) * step + step / 2
  const y = (value: number) => PLOT.top + plotHeight * (1 - value / peak)
  const path = series
    .map(
      (point, index) =>
        `${index === 0 ? "M" : "L"}${x(point.hourStart).toFixed(1)},${y(point.count).toFixed(1)}`
    )
    .join(" ")

  // Recessive gridlines; the labels wear ink, never the series colour.
  const ticks = [Math.round(peak), Math.round(peak / 2)].filter(
    (tick, index, all) => tick > 0 && all.indexOf(tick) === index
  )

  const hourLabel = (ms: number) =>
    new Intl.DateTimeFormat(undefined, {
      timeZone,
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    }).format(new Date(ms))

  return (
    <figure className="space-y-1">
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        className="h-auto w-full"
        role="img"
        aria-label="records per hour"
      >
        {ticks.map((tick) => (
          <g key={tick}>
            <line
              x1={PLOT.left}
              x2={WIDTH - PLOT.right}
              y1={y(tick)}
              y2={y(tick)}
              stroke="var(--line)"
              strokeWidth="1"
            />
            <text
              x={PLOT.left - 6}
              y={y(tick) + 3}
              textAnchor="end"
              fontSize="10"
              fill="var(--ink-muted)"
            >
              {tick >= 10_000 ? `${Math.round(tick / 1000)}k` : tick}
            </text>
          </g>
        ))}
        <line
          x1={PLOT.left}
          x2={WIDTH - PLOT.right}
          y1={PLOT.top + plotHeight}
          y2={PLOT.top + plotHeight}
          stroke="var(--line-strong)"
          strokeWidth="1"
        />
        <path
          d={path}
          fill="none"
          stroke="var(--chart-1)"
          strokeWidth="2"
          strokeLinejoin="round"
          strokeLinecap="round"
        />
        {/* A visible marker only where the series is sparse enough for one
            per hour to read as points rather than as a rope of beads. */}
        {series.length <= 60
          ? series.map((point) => (
              <circle
                key={point.hourStart}
                cx={x(point.hourStart)}
                cy={y(point.count)}
                r="2.5"
                fill="var(--chart-1)"
              />
            ))
          : null}
        {series.map((point) => (
          <rect
            key={point.hourStart}
            x={x(point.hourStart) - step / 2}
            y={PLOT.top}
            width={Math.max(step, 1)}
            height={plotHeight}
            fill="transparent"
          >
            <title>
              {per === 1
                ? `${hourLabel(point.hourStart)} — ${count(point.count)} record${point.count === 1 ? "" : "s"}`
                : `${hourLabel(point.hourStart)} → ${hourLabel(point.hourStart + width - 1)} — ${count(Math.round(point.count))} records per hour on average`}
            </title>
          </rect>
        ))}
        <text
          x={PLOT.left}
          y={HEIGHT - 8}
          fontSize="10"
          fill="var(--ink-muted)"
        >
          {hourLabel(first)}
        </text>
        <text
          x={WIDTH - PLOT.right}
          y={HEIGHT - 8}
          textAnchor="end"
          fontSize="10"
          fill="var(--ink-muted)"
        >
          {hourLabel(last + HOUR - 1)}
        </text>
      </svg>
      <figcaption className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-ink-faint">
        <span>
          plotted by {clock ?? "record timestamp"}
          {per > 1 ? ` · each point averages ${count(per)} hours` : ""}
          {stats.missingTimestamps > 0
            ? ` · ${count(stats.missingTimestamps)} record(s) with no timestamp are not plotted`
            : ""}
        </span>
        {stats.hourlyTruncated ? (
          <span className="text-warn-ink">
            the hour map hit its ceiling — this chart is a view, not the whole
            story
          </span>
        ) : null}
      </figcaption>
    </figure>
  )
}
