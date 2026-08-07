// Is this the newest kaas-ui, or is there a version nobody has deployed yet?
//
// The whole check runs in the browser, against GitHub's public API. That is a
// deliberate choice over having the server do it, and it costs one thing and
// buys three:
//
//   * The server gains no outbound dependency. kaas-ui dials Kafka brokers and
//     an identity provider and nothing else, and a deployment behind a strict
//     egress policy should not start failing because github.com is
//     unreachable.
//   * There is no new endpoint, so nothing is added to the read-only API
//     surface for a cosmetic badge.
//   * It degrades to silence. Every failure path here — offline, rate limited,
//     blocked by a proxy, a repo with no tags — resolves to `undefined`, and
//     the badge renders as though the check had never been asked for.
//
// What it costs is that each viewer's browser contacts GitHub rather than the
// server doing it once. That is why the answer is cached in `localStorage`:
// unauthenticated GitHub allows 60 requests an hour *per IP*, and a team
// behind one NAT reloading a dashboard would exhaust it between them and see
// the badge quietly stop working.

const REPO = "kaas-rs/kaas-ui"

/**
 * Tags, not releases.
 *
 * `/releases/latest` is the obvious endpoint and it **404s for this repo**:
 * pushing a `v*` tag does not create a GitHub Release, and this project has
 * never created one — 23 tags, zero releases. Building on it would have been
 * undetectable, because a failed check and an up-to-date deployment render
 * identically by design. Tags are also the honest source: the release process
 * here is tag-driven, `release.yml` fires on `v*`, and a Release object would
 * be a second thing to remember.
 *
 * `per_page=100` because the default is 30 and this list only grows.
 */
const TAGS_URL = `https://api.github.com/repos/${REPO}/tags?per_page=100`

/** Where a human goes to read about one version. Renders for a bare tag. */
export function releaseUrl(version: string): string {
  return `https://github.com/${REPO}/releases/tag/v${version}`
}

const CACHE_KEY = "kaas-ui-latest-version"

/**
 * How long a cached answer is trusted.
 *
 * Six hours, which is far longer than a release cadence needs and far shorter
 * than a stale badge stays interesting. The point is the rate limit, not
 * freshness: a browser that reloads all day makes four requests.
 */
const CACHE_TTL = 6 * 60 * 60 * 1000

interface Cached {
  version: string
  checkedAt: number
}

function readCache(): Cached | undefined {
  try {
    const raw = localStorage.getItem(CACHE_KEY)
    if (!raw) return undefined
    const parsed: unknown = JSON.parse(raw)
    if (
      typeof parsed === "object" &&
      parsed !== null &&
      typeof (parsed as Cached).version === "string" &&
      typeof (parsed as Cached).checkedAt === "number"
    ) {
      return parsed as Cached
    }
  } catch {
    // Unparseable or unavailable storage is not worth a broken header.
  }
  return undefined
}

function writeCache(version: string): void {
  try {
    localStorage.setItem(
      CACHE_KEY,
      JSON.stringify({ version, checkedAt: Date.now() } satisfies Cached)
    )
  } catch {
    // Private browsing, or a full quota. The check still worked this time.
  }
}

/** `v1.2.3` and `1.2` yes, `nightly` and `v2-beta` no. */
const VERSION_TAG = /^v?\d+(\.\d+)*$/

/**
 * The highest published version, or `undefined` if that cannot be established.
 *
 * Never throws and never rejects. A caller cannot distinguish "up to date"
 * from "could not tell" by catching, which is intentional — the two render
 * identically, and an error state for a badge nobody asked for would be worse
 * than no badge.
 */
export async function fetchLatestVersion(): Promise<string | undefined> {
  const cached = readCache()
  if (cached && Date.now() - cached.checkedAt < CACHE_TTL) {
    return cached.version
  }

  try {
    const response = await fetch(TAGS_URL, {
      headers: { accept: "application/vnd.github+json" },
    })
    // 403 is the rate limit and 404 is a repo that moved or went private.
    // Both are ordinary answers to this question, not faults to report.
    if (!response.ok) return cached?.version
    const body: unknown = await response.json()
    if (!Array.isArray(body)) return cached?.version

    // The maximum, **not** the first element. GitHub returns tags in roughly
    // reverse-alphabetical order, which coincides with newest-first only
    // while every segment is one digit: `v0.8.9` sorts above `v0.8.10`.
    const version = body
      .map((tag: unknown) => (tag as { name?: unknown }).name)
      .filter((name): name is string => typeof name === "string")
      .filter((name) => VERSION_TAG.test(name))
      .map((name) => name.replace(/^v/, ""))
      .reduce<string | undefined>(
        (best, candidate) =>
          best === undefined || compareVersions(candidate, best) > 0
            ? candidate
            : best,
        undefined
      )

    if (version === undefined) return cached?.version
    writeCache(version)
    return version
  } catch {
    // Offline, DNS-blocked, or refused by something in the middle.
    return cached?.version
  }
}

/**
 * Compare two dotted numeric versions.
 *
 * Returns a negative number when `a` precedes `b`. Segments are compared as
 * integers so `0.8.10` sorts after `0.8.9`, which a string comparison gets
 * backwards, and a shorter version is padded rather than treated as smaller.
 *
 * Any pre-release suffix is dropped before comparing. That is a simplification
 * — semver orders `1.0.0-rc.1` *before* `1.0.0` — and it is the right one
 * here: this only decides whether to draw a dot, and treating a release
 * candidate as its release is a far smaller error than telling someone running
 * `1.0.0` that `1.0.0-rc.1` is newer.
 */
export function compareVersions(a: string, b: string): number {
  const parse = (value: string) =>
    value
      .split("-")[0]!
      .split(".")
      .map((part) => Number.parseInt(part, 10))

  const left = parse(a)
  const right = parse(b)
  const length = Math.max(left.length, right.length)

  for (let index = 0; index < length; index += 1) {
    const l = left[index] ?? 0
    const r = right[index] ?? 0
    if (Number.isNaN(l) || Number.isNaN(r)) return 0
    if (l !== r) return l - r
  }
  return 0
}

/**
 * Whether a newer version exists.
 *
 * Strictly newer. Running something *ahead* of the newest tag is normal — the
 * `main` image is built from every push, and the version in `Cargo.toml` is
 * bumped in the release commit — so "not equal" is the wrong test. It would
 * light the badge on every development deployment.
 */
export function isOutdated(
  current: string | undefined,
  latest: string | undefined
): boolean {
  if (!current || !latest) return false
  return compareVersions(current, latest) < 0
}
