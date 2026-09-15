// The brand mark: the kaas logo, bundled.
//
// **An `<img>`, not inlined JSX**, and that is about the artwork rather than
// taste. It is a vector export whose seven gradients are declared under ids of
// the form `_Linear1`. SVG ids are document-global, so two inlined copies —
// the sidebar and a second mark, or this beside any other export from the same
// tool — collide, the last definition wins, and the earlier copy's fills go
// flat. Each `<img>` gets its own document and the question cannot arise.
//
// The URL is a two-part answer, because the two halves are decided at
// different times:
//
// * Vite fingerprints the file into `assets/` and bakes that URL in **at build
//   time**, prefixed with whatever `base` the build ran under;
// * a reverse proxy mounting kaas-ui under a path is a **runtime** fact the
//   build cannot know, and `withBase` is how everything else here answers it.
//
// So strip the first before applying the second. Both are usually empty — a
// production build runs at `/` and `BASE_PATH` is `""` — and neither is empty
// under `npm run dev` inside code-server, where skipping the strip would
// prefix the path twice and 404 the mark in the one environment it is looked
// at most. See `web/src/api/base.ts`.

import logoUrl from "@/assets/kaas-logo.svg"
import { withBase } from "@/api/base"
import { cn } from "@/lib/utils"

const BUILT_UNDER = import.meta.env.BASE_URL.replace(/\/+$/, "")

const SRC = withBase(
  BUILT_UNDER && logoUrl.startsWith(`${BUILT_UNDER}/`)
    ? logoUrl.slice(BUILT_UNDER.length)
    : logoUrl
)

/**
 * The mark, sized by its caller.
 *
 * `aria-hidden` and an empty `alt`, always: every place this renders sits
 * beside the word "kaas-ui" already, and a screen reader announcing the logo
 * as well reads the name twice.
 */
export function KaasLogo({ className }: { className?: string }) {
  return (
    <img
      src={SRC}
      alt=""
      aria-hidden
      draggable={false}
      className={cn("shrink-0 select-none", className)}
    />
  )
}
