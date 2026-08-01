import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The dev server proxies the API to a locally running `kaas-ui` so the
// frontend can be developed without rebuilding the Rust binary on every
// change. In production there is no proxy: the binary serves both.
export default defineConfig({
  plugins: [react(), tailwindcss()],
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
