// The URL is the state.
//
// A view seeked to a timestamp, filtered, with a message selected is the thing
// people actually send each other from a Kafka UI. It has to survive a copied
// link and a reload, which means the router owns it and nothing else does —
// no `useState` mirroring a search param, because the two then disagree the
// first time someone uses the back button.
//
// Validated with zod so a hand-edited URL produces a usable page rather than
// a component reading `undefined.toString()`.
//
// The message browser lives in a tab on the topic page, so the *topic* route
// is what validates these — plus which tab is open, for the same reason. A
// link to a seeked view that lands on the partitions table is not a link to
// that view.

import type { SearchSchemaInput } from "@tanstack/react-router"
import { z } from "zod"

import { DEFAULT_SEEK_MODE, SEEK_MODE_NAMES, type SeekMode } from "./seek-modes"

const modes = SEEK_MODE_NAMES as [SeekMode, ...SeekMode[]]

/**
 * The longest needle the server will accept — `MAX_FILTER_CHARS` in
 * `kaas-ui-core`, kept in step here so a pasted essay is trimmed rather than
 * sent and refused. An over-long filter on the stream would be a `400` that
 * `EventSource` reports as a dropped connection and retries forever.
 */
export const MAX_FILTER_CHARS = 256

const fields = z.object({
  mode: z.enum(modes).default(DEFAULT_SEEK_MODE),
  offset: z.coerce.number().int().nonnegative().optional(),
  /** Epoch milliseconds, which is what the wire and `ListOffsets` take. */
  timestamp: z.coerce.number().int().optional(),
  partitions: z.string().optional(),
  /** Whether records from aborted transactions are shown. */
  visibility: z.enum(["all", "committed"]).default("all"),
  /**
   * A literal substring of the decoded value.
   *
   * Matched by the server after the payload is decoded, so it searches what
   * the row shows rather than the bytes underneath it. Capped there at 256
   * characters — see `MAX_FILTER_CHARS` — and the input says so too, because a
   * pasted essay should not become a 400.
   */
  filter: z.string().optional(),
  limit: z.coerce.number().int().positive().max(10_000).optional(),
  /**
   * How to read keys and values, overriding the per-topic configuration.
   *
   * No control sets these any more — the toolbar's two codec selects are
   * gone — but the URL still carries them and the server still honours them,
   * so a link shared while the selects existed opens on the view it named.
   *
   * Only the four that need no schema are accepted. `avro` here would be a URL
   * asking the server to invent a schema id, which it refuses — better to keep
   * it unrepresentable in the link than to explain the refusal.
   */
  keyCodec: z.enum(["auto", "string", "hex", "json"]).optional(),
  valueCodec: z.enum(["auto", "string", "hex", "json"]).optional(),
  /** `{partition}-{offset}` — the same id everything else keys on. */
  selected: z.string().optional(),
})

type Fields = z.infer<typeof fields>

/**
 * A seek mode without its parameter is not a view, and rendering one anyway
 * means the server rejects the stream and the page shows an error it could
 * have avoided. Falling back to a mode that needs nothing is friendlier than
 * a validation screen for a link someone was sent. An over-long filter is
 * trimmed here for the same reason.
 */
function usable(search: Fields): Fields {
  // By code point rather than by `.length`, which counts UTF-16 units and
  // would cut an astral character in half — and half a character is not a
  // substring of anything.
  const needle = search.filter ? [...search.filter] : []
  const filter =
    needle.length > MAX_FILTER_CHARS
      ? needle.slice(0, MAX_FILTER_CHARS).join("")
      : search.filter

  const needsOffset = search.mode === "fromOffset" || search.mode === "toOffset"
  const needsTime = search.mode === "sinceTime" || search.mode === "toTime"
  if (
    (needsOffset && search.offset === undefined) ||
    (needsTime && search.timestamp === undefined)
  ) {
    return { ...search, filter, mode: DEFAULT_SEEK_MODE }
  }
  return { ...search, filter }
}

export const messageSearch = fields.transform(usable)

export type MessageSearch = Fields

/**
 * The topic page's tabs. In the URL, so a shared link opens the right one.
 *
 * Two names have been retired here, `placement` and then `partitions`, as the
 * grid moved into the partition table and the table gained a card above it.
 * Neither is listed and neither needs to be: both fall through the `.catch`
 * below onto `overview`, which is where that content now lives, so links
 * shared under either name still open on the view they were pointing at.
 */
export const TOPIC_TABS = ["overview", "configs", "messages"] as const

export type TopicTab = (typeof TOPIC_TABS)[number]

export const topicSearch = fields
  // `.catch` rather than `.default`: it covers a missing tab, a retired one
  // *and* a nonsense one. A tab is a thing people hand-edit, and `?tab=message`
  // landing on an error boundary rather than on the topic would be a poor
  // reward for it.
  .extend({ tab: z.enum(TOPIC_TABS).catch("overview") })
  .transform((search) => ({ ...usable(search), tab: search.tab }))

export type TopicSearch = Fields & { tab: TopicTab }

/**
 * What a `<Link>` to the topic page may pass.
 *
 * Every field optional, because a link to a topic is `/topics/orders` and
 * nothing more. The `SearchSchemaInput` brand is what makes the router believe
 * that: without it, `validateSearch` is read as taking what it returns, and
 * every link to a topic in the app is then made to state a seek mode.
 */
export type TopicSearchInput = Partial<TopicSearch> & SearchSchemaInput

/** TanStack Router's `validateSearch` for the topic route. */
export function topicSearchSchema(input: TopicSearchInput): TopicSearch {
  return topicSearch.parse(input)
}

/**
 * The same, for the standalone `…/messages` URL that now redirects into the
 * tab. Parsing it there rather than forwarding the raw query is what keeps a
 * link shared before the move — seeked, filtered, with a row selected —
 * arriving on exactly the view it named.
 */
export function messageSearchSchema(
  input: Partial<MessageSearch> & SearchSchemaInput
): MessageSearch {
  return messageSearch.parse(input)
}
