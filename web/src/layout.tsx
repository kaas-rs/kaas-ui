// The frame every page is rendered inside: sidebar, top bar, content, footer.
//
// This is the root route's component, which is why the sign-in gate lives here
// — it is the one place that can decide there is nothing behind it to draw.
// Everything else it does is composition, in the shape shadcn's dashboard puts
// in the page rather than in the sidebar: a provider, an `AppSidebar`, and an
// inset holding the bar and the outlet.

import { Outlet, useRouterState } from "@tanstack/react-router"

import { useIdentity } from "@/api/client"
import { SignIn } from "@/pages/sign-in"
import { AppSidebar } from "@/components/app-sidebar"
import { Breadcrumbs } from "@/components/breadcrumbs"
import { VersionBadge } from "@/components/version-badge"
import { Separator } from "@/components/ui/separator"
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar"

export function AppLayout() {
  const identity = useIdentity()
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  })

  // Signed out, on a deployment that enforces roles: there is nothing behind
  // this to render. An open deployment never reaches here, and neither does
  // one whose provider is configured but whose roles are not — that caller
  // already sees everything, so demanding a login first would be theatre.
  const me = identity.data
  if (me && me.loginAvailable && me.enforcing && !me.authenticated) {
    return <SignIn enforcing={me.enforcing} connectors={me.connectors} />
  }

  return (
    <SidebarProvider>
      <AppSidebar />

      <SidebarInset className="min-w-0">
        <header className="flex h-12 shrink-0 items-center gap-2 border-b px-4">
          <SidebarTrigger className="-ml-1" />
          <Separator orientation="vertical" className="mr-1 h-4" />
          <Breadcrumbs pathname={pathname} />
          {/* The corner used to hold a link to openapi.json. That is a
              reference document — something you go and find once — and it has
              moved to Settings, where the other things you look up live. What
              belongs in a frame drawn on every page is the answer to "what am
              I looking at", which is the build. */}
          <VersionBadge />
        </header>

        <main className="min-w-0 flex-1 p-6">
          <div className="mx-auto w-full max-w-[1400px]">
            <Outlet />
          </div>
        </main>

        <footer className="border-t px-6 py-3 text-[12px] text-ink-faint">
          read-only by construction — kaas-ui has no mutating endpoint
        </footer>
      </SidebarInset>
    </SidebarProvider>
  )
}
