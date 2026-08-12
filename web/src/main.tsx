// The entry point: providers, global settings, and the mount — the route tree
// itself lives in `router.tsx`.

import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider } from "@tanstack/react-router"

import "./styles.css"
import { TooltipProvider } from "@/components/ui/tooltip"
import { installSettings } from "@/lib/settings"
import { router } from "@/router"

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // A cluster being unreachable is a fact to render, not a request to
      // retry three times before saying so.
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
})

// Keep the document's theme in step with the stored choice, from here on. The
// inline script in `index.html` already resolved it before first paint; this
// picks the same key up and keeps following the OS — outside React, because a
// listener that lives in a component stops at the first navigation away from it.
installSettings()

const container = document.getElementById("root")
if (container) {
  createRoot(container).render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={200}>
          <RouterProvider router={router} />
        </TooltipProvider>
      </QueryClientProvider>
    </StrictMode>
  )
}
