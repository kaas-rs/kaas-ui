import { fileURLToPath, URL } from "node:url"
import { defineConfig } from "vite"
import react from "@vitejs/plugin-react"
import tailwindcss from "@tailwindcss/vite"

// The dev server proxies the API to a locally running `kaas-ui` so the
// frontend can be developed without rebuilding the Rust binary on every
// change. In production there is no proxy: the binary serves both.

const PORT = 5173
const API = "http://127.0.0.1:8099"

// Development inside code-server, reached through its path proxy.
//
// **`/absproxy/`, not `/proxy/`,** and the difference is the whole reason this
// block exists. code-server offers both. `/proxy/<port>` *strips* the prefix
// before forwarding — that is why `config.dev.yaml` has to tell the Rust
// binary its `base_path`, and why the binary serves `/assets/…` while
// rewriting the HTML it hands out to say `/proxy/8099/assets/…`. A server can
// be built to work that way. Vite cannot: `base` makes it both *emit* and
// *expect* the prefix, so once code-server removes it every module 404s while
// index.html still loads — a page that appears to work and executes nothing.
//
// `/absproxy/<port>` sets `passthroughPath: true` and forwards the path whole,
// which is the shape `base` already assumes. code-server registers a
// `wsRouter` for it as well, so HMR's WebSocket survives the trip.
//
// The origin comes from code-server's own `VSCODE_PROXY_URI`, so no hostname
// is written down here and the base cannot drift from the socket.
const proxied =
  process.env.CODE_SERVER && process.env.VSCODE_PROXY_URI
    ? new URL(
        process.env.VSCODE_PROXY_URI.replace("{{port}}", String(PORT)).replace(
          "/proxy/",
          "/absproxy/"
        )
      )
    : undefined

// Always `/` for a build. Serving under a path prefix is a *runtime* concern:
// the binary rewrites index.html as it serves it, driven by `server.base_path`,
// so one build works at any prefix. Baking a base in here would produce a
// bundle that 404s its own assets anywhere else.
const base = process.env.VITE_BASE || proxied?.pathname || "/"
const prefix = base.replace(/\/$/, "")

/**
 * Route the API paths to the Rust binary.
 *
 * Under a base path the browser asks for `{base}/api/…`, because `withBase`
 * builds every URL from `import.meta.env.BASE_URL`. So the prefix has to be
 * part of the match here, and has to come back off before the request reaches
 * a server that knows nothing about it.
 */
const apiProxy = Object.fromEntries(
  ["/api", "/health"].map((path) => [
    `${prefix}${path}`,
    prefix
      ? { target: API, rewrite: (from: string) => from.slice(prefix.length) }
      : { target: API },
  ])
)

export default defineConfig({
  base,
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  server: {
    port: PORT,
    // Fail rather than silently move to 5174: the proxied path, the base and
    // the HMR socket all name this port, and a server that quietly picked
    // another one would serve a page whose every asset URL is wrong.
    strictPort: true,
    // All interfaces under code-server, and this is not optional: Vite's
    // default bind is loopback, which in this container resolves to `::1`
    // alone. code-server's proxy dials IPv4 and gets `ECONNREFUSED
    // 0.0.0.0:5173` — a message that reads like nothing is running, when what
    // is wrong is which address family it is running on.
    host: proxied ? true : undefined,
    proxy: apiProxy,
    hmr: proxied
      ? {
          protocol: proxied.protocol === "https:" ? "wss" : "ws",
          host: proxied.hostname,
          // The port the *browser* connects to, which is the edge's, not
          // ours. `new URL` leaves `port` empty on a default-port URL, so
          // the scheme supplies it.
          clientPort:
            Number(proxied.port) || (proxied.protocol === "https:" ? 443 : 80),
        }
      : true,
  },
  build: {
    // Everything under assets/ is fingerprinted, which is what lets the server
    // mark it immutable and index.html no-cache.
    assetsDir: "assets",
    sourcemap: false,
  },
})
