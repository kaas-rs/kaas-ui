// Who you are, at the foot of the sidebar.
//
// Built on shadcn's nav-user block, with its fields answered from what an
// OIDC login actually gives us rather than from the block's sample data.
// Three of those answers are different, and each difference is a fact about
// this application rather than a shortcut:
//
//   * **No avatar image.** `/api/me` carries no picture and Dex is not asked
//     for one, so the avatar is initials and there is no `AvatarImage` to be
//     permanently empty. That is also why no `avatar.tsx` primitive is added:
//     a Radix Avatar exists to swap an image for a fallback, and there is no
//     image.
//   * **Roles, where the block puts an email.** An email is not why anyone
//     opens this menu; "which roles do I have" is, because it is the answer
//     to why a cluster is missing from the fleet.
//   * **It renders when nobody is signed in.** Anonymous is a real answer
//     here — it is what every development deployment runs as — and a footer
//     that disappears leaves "who am I" unanswerable exactly when it is
//     least obvious.

import {
  BadgeCheck,
  ChevronsUpDown,
  LogIn,
  LogOut,
  Settings,
} from "lucide-react"
import { useRef } from "react"
import { Link } from "@tanstack/react-router"

import { withBase } from "@/api/base"
import type { Identity } from "@/api/types"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from "@/components/ui/sidebar"

/** Up to two letters, from a display name that may be one word or three. */
function initials(name: string): string {
  const parts = name.split(/[\s._-]+/).filter(Boolean)
  const letters = parts.slice(0, 2).map((part) => part[0] ?? "")
  return (letters.join("") || name.slice(0, 2) || "?").toUpperCase()
}

/**
 * The line under the name.
 *
 * Never empty, and never a guess: each branch is a distinguishable state of
 * the auth model, and collapsing them would make "signed in with nothing
 * granted" look like "signed in".
 */
function subtitle(me: Identity): string {
  if (!me.authenticated)
    return me.loginAvailable ? "not signed in" : "no identity provider"
  if (me.roles.length > 0) return me.roles.join(", ")
  return me.enforcing
    ? "no roles — nothing is visible"
    : "roles are not enforced"
}

function Face({ name }: { name: string }) {
  return (
    <div
      className="bg-surface-sunken text-ink border-line flex size-8 shrink-0 items-center justify-center rounded-lg border text-[11px] font-semibold"
      aria-hidden
    >
      {initials(name)}
    </div>
  )
}

export function NavUser({ identity }: { identity: Identity }) {
  const { isMobile } = useSidebar()
  // Logout is a POST so that a page elsewhere cannot sign somebody out by
  // being loaded. A menu item is a button, not a form, so the form lives
  // beside the menu and the item submits it — the property survives the
  // change of clothes.
  const signOut = useRef<HTMLFormElement>(null)

  const name = identity.authenticated ? identity.displayName : "anonymous"
  const under = subtitle(identity)

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        {/* Kept on one line: `cargo xtask ci` proves logout is a POST by
            grepping for `action` and `method="post"` together, and Prettier
            would otherwise wrap this at 80 columns and defeat the check. */}
        {/* prettier-ignore */}
        <form ref={signOut} method="post" action={withBase("/auth/logout")} hidden />

        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton
              size="lg"
              tooltip={`${name} — ${under}`}
              className="data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground"
            >
              <Face name={name} />
              <div className="grid flex-1 text-left text-sm leading-tight">
                <span className="truncate font-medium">{name}</span>
                <span className="text-ink-muted truncate text-xs">{under}</span>
              </div>
              <ChevronsUpDown className="ml-auto size-4" />
            </SidebarMenuButton>
          </DropdownMenuTrigger>

          <DropdownMenuContent
            className="w-(--radix-dropdown-menu-trigger-width) min-w-56 rounded-lg"
            side={isMobile ? "bottom" : "right"}
            align="end"
            sideOffset={4}
          >
            <DropdownMenuLabel className="p-0 font-normal">
              <div className="flex items-center gap-2 px-1 py-1.5 text-left text-sm">
                <Face name={name} />
                <div className="grid flex-1 text-left text-sm leading-tight">
                  <span className="truncate font-medium">{name}</span>
                  <span className="text-ink-muted truncate text-xs">
                    {under}
                  </span>
                </div>
              </div>
            </DropdownMenuLabel>

            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              {/* Offered signed out too: "what am I allowed to see" is a fair
                  question for an anonymous caller, and on an open deployment
                  the answer — everything — is worth being able to check. */}
              <DropdownMenuItem asChild>
                <Link to="/account">
                  <BadgeCheck />
                  Account
                </Link>
              </DropdownMenuItem>
              {/* Next to Account because the pair is one question asked twice
                  — what is true of *me* here. Account answers the half that
                  follows the login to any machine; Settings answers the half
                  that stays in this browser. */}
              <DropdownMenuItem asChild>
                <Link to="/settings">
                  <Settings />
                  Settings
                </Link>
              </DropdownMenuItem>
            </DropdownMenuGroup>

            <DropdownMenuSeparator />
            {identity.authenticated ? (
              <DropdownMenuItem onSelect={() => signOut.current?.submit()}>
                <LogOut />
                Sign out
              </DropdownMenuItem>
            ) : identity.loginAvailable ? (
              <DropdownMenuItem asChild>
                {/* A plain navigation: the browser has to leave for the
                    provider and come back with a cookie. */}
                <a href={withBase("/auth/login")}>
                  <LogIn />
                  Sign in with GitHub
                </a>
              </DropdownMenuItem>
            ) : (
              <DropdownMenuItem
                onSelect={(event) => event.preventDefault()}
                className="focus:bg-transparent"
              >
                <span className="text-ink-muted text-[12px]">
                  This deployment has no identity provider, so every caller is
                  this one.
                </span>
              </DropdownMenuItem>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  )
}
