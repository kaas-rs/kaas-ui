// The sidebar, composed.
//
// shadcn's app-sidebar block with its `data` object deleted rather than
// filled in: the block hands sample data down as props, and here every part
// fetches what it renders — `NavMain` reads the fleet, `NavUser` reads the
// identity — because the alternative is a component whose only job is to hold
// three queries and pass them one level down.
//
// What is left is the composition, which is the part of the block worth
// keeping: header, content, footer, rail, and a `collapsible="icon"` sidebar
// that forwards its props so a caller can still say where it goes.

import type { ComponentProps } from "react"
import { Link } from "@tanstack/react-router"

import { useIdentity } from "@/api/client"
import { NavMain } from "@/components/nav-main"
import { NavUser } from "@/components/nav-user"
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
} from "@/components/ui/sidebar"

/**
 * The dark nav band is the strongest visual anchor and it is dark in both
 * modes — same as the book's sidebar. It is also where cluster identity lives,
 * because with a dozen clusters in one UI "which cluster am I looking at" must
 * be answerable without reading the URL.
 */
export function AppSidebar({ ...props }: ComponentProps<typeof Sidebar>) {
  const identity = useIdentity()

  return (
    <Sidebar collapsible="icon" {...props}>
      <SidebarHeader>
        {/* The mark is the way back to the fleet, which is the one view where
            every environment is on screen at once. Where the block puts a team
            switcher, this application puts the way out of the environment it is
            currently in. */}
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton asChild size="lg">
              <Link to="/">
                <div
                  className="flex aspect-square size-8 items-center justify-center rounded-md font-mono text-[15px] font-semibold"
                  style={{ background: "var(--rust)", color: "#3B2E2A" }}
                >
                  k
                </div>
                <div className="grid flex-1 text-left leading-tight">
                  <span className="truncate font-semibold">kaas-ui</span>
                  <span className="truncate text-[11px] opacity-70">
                    read-only
                  </span>
                </div>
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent>
        <NavMain />
      </SidebarContent>

      <SidebarFooter>
        {/* Rendered whenever `/api/me` has answered, signed in or not:
            anonymous is a real state here and hiding it makes "who am I"
            unanswerable on exactly the deployments where it is least obvious. */}
        {identity.data ? <NavUser identity={identity.data} /> : null}
        {/* The theme used to be a second row here, cycling through three states
            on click. It is a setting now, on the settings page with the others
            and reachable from the menu above — a footer row that changes what
            the whole application looks like was a lot of consequence for
            something sitting on the way to "sign out", and a cycle cannot say
            which of the three states it is heading for. */}
      </SidebarFooter>

      <SidebarRail />
    </Sidebar>
  )
}
