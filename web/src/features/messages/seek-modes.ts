// The seven ways to ask for a window of a topic.
//
// One table drives the store, the virtualizer and the toolbar. The moment a
// component asks `mode === "live"` directly, a new mode arrives and that
// component is the one place nobody remembers to update — usually showing up
// as a list that sorts the wrong way for exactly one mode, or a viewport that
// drags on every arriving record.
//
// Mirrors `crates/kaas-ui-api/src/routes/messages/seek.rs`. The two must agree
// about which modes walk backwards, because the server sorts and the client
// scrolls on the same answer.

export type SeekMode =
  | "live"
  | "newest"
  | "oldest"
  | "fromOffset"
  | "toOffset"
  | "sinceTime"
  | "toTime"

export type SeekGroup = "Streaming" | "Snapshot" | "Seek"

export interface SeekModeConfig {
  label: string
  group: SeekGroup
  /** Whether the stream stays open. Only `live` does. */
  live: boolean
  /** Which end new rows arrive at. `desc` is newest-first. */
  sort: "asc" | "desc"
  /** The extra control the toolbar shows beside the selector. */
  input: "none" | "offset" | "datetime"
  hint: string
}

export const SEEK_GROUPS: readonly SeekGroup[] = [
  "Streaming",
  "Snapshot",
  "Seek",
]

export const SEEK_MODES: Record<SeekMode, SeekModeConfig> = {
  live: {
    label: "Live",
    group: "Streaming",
    live: true,
    sort: "desc",
    input: "none",
    hint: "Tails the topic as messages arrive",
  },
  newest: {
    label: "Newest",
    group: "Snapshot",
    live: false,
    sort: "desc",
    input: "none",
    hint: "Most recent messages, then stops",
  },
  oldest: {
    label: "Oldest",
    group: "Snapshot",
    live: false,
    sort: "asc",
    input: "none",
    hint: "From the start of retention",
  },
  fromOffset: {
    label: "From offset",
    group: "Seek",
    live: false,
    sort: "asc",
    input: "offset",
    hint: "Reads forward from this offset",
  },
  toOffset: {
    label: "To offset",
    group: "Seek",
    live: false,
    sort: "desc",
    input: "offset",
    hint: "Reads backward from this offset, which is included",
  },
  sinceTime: {
    label: "Since time",
    group: "Seek",
    live: false,
    sort: "asc",
    input: "datetime",
    hint: "Reads forward from this moment",
  },
  toTime: {
    label: "To time",
    group: "Seek",
    live: false,
    sort: "desc",
    input: "datetime",
    hint: "Reads backward from this moment, which is included",
  },
}

export const SEEK_MODE_NAMES = Object.keys(SEEK_MODES) as SeekMode[]

/**
 * What a URL that does not name a mode gets.
 *
 * `newest` rather than `live`, and the reason is not only that the newest
 * records are what someone opening a topic wants to see. A live tail is the
 * one mode that never ends, and kaas-lib keeps **one connection per broker**
 * shared by every caller — so defaulting to it meant that merely opening a
 * topic page started a long poll that sits at the head of that queue and slows
 * every other view of the cluster. See ask 11 in
 * `docs/reference/upstream-asks.md`, where that cost is measured.
 *
 * Live is still one click away, and now it is a thing someone asks for rather
 * than a thing they get by arriving.
 *
 * Mirrored by `Plan::build` in `routes/messages/seek.rs`, which answers the
 * same question for a caller that is not this app.
 */
export const DEFAULT_SEEK_MODE: SeekMode = "newest"

/**
 * Whether arriving rows go on the top of the list rather than the bottom.
 *
 * The one derived fact the scroll compensation is allowed to depend on.
 * Deriving it here rather than comparing mode names in a component is what
 * makes the rule survive a new mode: an appending mode that gets the
 * prepending correction visibly drags the list under the reader's cursor.
 */
export function insertsAtTop(mode: SeekMode): boolean {
  const config = SEEK_MODES[mode]
  return config.sort === "desc" && config.live
}

/** Whether this mode needs a companion parameter to be a valid query. */
export function needs(mode: SeekMode): "offset" | "timestamp" | null {
  switch (SEEK_MODES[mode].input) {
    case "offset":
      return "offset"
    case "datetime":
      return "timestamp"
    default:
      return null
  }
}
