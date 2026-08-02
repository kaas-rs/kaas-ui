import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The dev server proxies the API to a locally running `kaas-ui` so the
// frontend can be developed without rebuilding the Rust binary on every
// change. In production there is no proxy: the binary serves both.
export default defineConfig({
  // Always `/`. Serving under a path prefix is a *runtime* concern: the binary
  // rewrites index.html as it serves it, driven by `server.base_path`, so one
  // build works at any prefix. Baking a base in here would produce a bundle
  // that 404s its own assets anywhere else.
  //
  // VITE_BASE still exists for the dev server, which has no Rust binary in
  // front of it to do the rewriting — see the `dev:proxy` script.
  base: process.env.VITE_BASE || "/",
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  server: {
    proxy: {
      "/api": "http://127.0.0.1:8099",
      "/health": "http://127.0.0.1:8099",
    },
  },
  build: {
    // Everything under assets/ is fingerprinted, which is what lets the server
    // mark it immutable and index.html no-cache.
    assetsDir: "assets",
    sourcemap: false,
  },
});
