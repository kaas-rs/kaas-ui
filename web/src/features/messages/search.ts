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

import { z } from "zod";

import { SEEK_MODE_NAMES, type SeekMode } from "./seek-modes";

const modes = SEEK_MODE_NAMES as [SeekMode, ...SeekMode[]];

export const messageSearch = z
  .object({
    mode: z.enum(modes).default("live"),
    offset: z.coerce.number().int().nonnegative().optional(),
    /** Epoch milliseconds, which is what the wire and `ListOffsets` take. */
    timestamp: z.coerce.number().int().optional(),
    partitions: z.string().optional(),
    /** Whether records from aborted transactions are shown. */
    visibility: z.enum(["all", "committed"]).default("all"),
    filter: z.string().optional(),
    limit: z.coerce.number().int().positive().max(10_000).optional(),
    /** `{partition}-{offset}` — the same id everything else keys on. */
    selected: z.string().optional(),
  })
  // A seek mode without its parameter is not a view, and rendering one anyway
  // means the server rejects the stream and the page shows an error it could
  // have avoided. Falling back to a mode that needs nothing is friendlier than
  // a validation screen for a link someone was sent.
  .transform((search) => {
    const needsOffset = search.mode === "fromOffset" || search.mode === "toOffset";
    const needsTime = search.mode === "sinceTime" || search.mode === "toTime";
    if (needsOffset && search.offset === undefined) return { ...search, mode: "live" as const };
    if (needsTime && search.timestamp === undefined) return { ...search, mode: "live" as const };
    return search;
  });

export type MessageSearch = z.infer<typeof messageSearch>;

/** TanStack Router's `validateSearch`. */
export function messageSearchSchema(input: Record<string, unknown>): MessageSearch {
  return messageSearch.parse(input);
}
