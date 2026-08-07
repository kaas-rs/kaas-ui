// What this deployment is running, in the corner of every page.
//
// The number comes from `/health`, which reports `CARGO_PKG_VERSION` — the
// version compiled into the binary, not the image tag it was pulled under. If
// someone deploys `:0.8.2` from an image built before the release commit, this
// says so and the tag does not.

import { useQuery } from "@tanstack/react-query"

import { useHealth } from "@/api/client"
import { fetchLatestVersion, isOutdated, releaseUrl } from "@/lib/upstream"
import { Button } from "@/components/ui/button"

/**
 * GitHub's mark, inline.
 *
 * Not from lucide: it removed its brand icons, and `lucide-react@1` exports no
 * `Github`. Inlining one path is cheaper than a second icon dependency for a
 * single glyph.
 */
function GithubMark({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 16 16"
      fill="currentColor"
      aria-hidden
      className={className}
    >
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  )
}

export function VersionBadge() {
  const health = useHealth()
  const current = health.data?.version

  // Deliberately not gated on `current`: the newest release is worth knowing
  // even when /health has not answered yet, and the query is cached across the
  // session either way. It resolves to `undefined` rather than failing.
  const latest = useQuery({
    queryKey: ["latest-release"],
    queryFn: fetchLatestVersion,
    staleTime: Infinity,
    retry: false,
  })

  const behind = isOutdated(current, latest.data)

  // Nothing to show until the running version is known. An empty slot is
  // better than a spinner in a header that is otherwise still.
  if (!current) return null

  return (
    <Button
      variant="ghost"
      size="sm"
      asChild
      className="relative ml-auto text-[12px]"
    >
      {/* Points at whichever version is worth reading about: the newer one
          when there is one, otherwise the one you are running. */}
      <a
        href={releaseUrl(behind && latest.data ? latest.data : current)}
        target="_blank"
        rel="noreferrer noopener"
      >
        <GithubMark className="size-3.5" />
        <span className="font-mono">{current}</span>
        {/* Presence is the signal, not hue: the dot either is or is not
            there, so this still reads without colour vision. With no tooltip
            left to explain it, the sr-only label below is the only wording
            there is — which is why it spells the state out rather than
            repeating the number. */}
        {behind ? (
          <span
            aria-hidden
            className="bg-warn absolute top-1 right-1 size-1.5 rounded-full"
          />
        ) : null}
        <span className="sr-only">
          {behind
            ? `version ${current}, and ${latest.data} has been released`
            : `version ${current}`}
        </span>
      </a>
    </Button>
  )
}
