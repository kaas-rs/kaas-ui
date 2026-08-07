// Where this page is actually being served from.
//
// Normally `/`, and then everything below is a no-op. It stops being `/` when
// a reverse proxy mounts kaas-ui under a path — code-server's `/proxy/8099/`,
// or an ingress hosting it at `/kafka/`.
//
// **Read at runtime, not compiled in.** The server rewrites `index.html` as it
// serves it, adding a `<base>` element naming the prefix; this reads it back.
// The alternative — Vite's `base` option, baked in at build time — produces a
// binary that is *compiled for* one deployment and 404s its own assets
// anywhere else, which is a trap that costs a confusing debugging session
// every time someone forgets. See `crates/kaas-ui-server/src/assets.rs`.
//
// The prefix cannot be detected: a stripping proxy forwards no record of what
// it removed. code-server sends no `X-Forwarded-Prefix` and rewrites `Host` to
// its own, so the request that arrives at kaas-ui is indistinguishable from
// one made at the root. It is configuration — `server.base_path`.
//
// This value drives the router's `basepath` and every URL built in
// JavaScript: `fetch`, and the `EventSource` the message stream opens. The
// asset URLs in `index.html` are rewritten by the same server pass, so they do
// not go through here.

function detect(): string {
  // The element the server injected. Absolute, so it survives a deep link:
  // resolving it against the origin gives the prefix whatever page we are on.
  const injected = document.querySelector("base")?.getAttribute("href")
  if (injected) {
    try {
      return new URL(injected, window.location.origin).pathname
    } catch {
      // A malformed href is not worth breaking the application over; fall
      // through to the build-time value, which is `/` in every normal build.
    }
  }
  // No server in front: `npm run dev`, where Vite's own `base` is the answer.
  return import.meta.env.BASE_URL
}

/** The prefix, without its trailing slash. `""` when hosted at the root. */
export const BASE_PATH = detect().replace(/\/+$/, "")

/**
 * Put an absolute application path under the prefix.
 *
 * Takes paths that start with `/` and returns them unchanged at the root,
 * which is why adding it to an existing call site is safe.
 */
export function withBase(path: string): string {
  return `${BASE_PATH}${path}`
}
