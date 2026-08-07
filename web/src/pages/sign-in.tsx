// The one screen an unauthenticated caller sees, when this deployment has an
// identity provider and roles to enforce.
//
// A page with a button rather than an automatic redirect. A redirect is
// tempting — one fewer click — but it fires again the moment a session
// expires, which means it fires in the middle of reading a topic and returns
// you to the fleet with no idea why. A button is a place to stand.

import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { LogIn } from "lucide-react"

import { withBase } from "@/api/base"

export function SignIn({ enforcing }: { enforcing: boolean }) {
  return (
    <div className="bg-muted flex min-h-svh flex-col items-center justify-center gap-6 p-6 md:p-10">
      <div className="flex w-full max-w-sm flex-col gap-6">
        {/* The same mark the sidebar wears, at the size the shell uses for a
            cluster chip. Not a link: there is nowhere behind this to go, and a
            brand that navigates to the page you are on is a dead control. */}
        <div className="flex items-center gap-2 self-center font-medium">
          <div
            className="flex size-6 items-center justify-center rounded-md font-mono text-sm font-semibold"
            style={{ background: "var(--rust)", color: "#3B2E2A" }}
            aria-hidden
          >
            k
          </div>
          kaas-ui
        </div>

        <Card>
          <CardHeader className="text-center">
            <CardTitle className="text-xl">Welcome back</CardTitle>
            <CardDescription>
              Sign in to see the clusters your roles cover
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            {/* A plain link, not a fetch: the whole point of this navigation
                is that the browser leaves for the provider and comes back
                with a cookie.

                Deliberately unbranded. Which providers exist is Dex's
                configuration, not this build's — a deployment can front
                GitHub, Entra, both, or something neither of us has heard of,
                and with more than one Dex asks which before the login form.
                Naming one here is wrong the moment a second is added, and
                nothing in this bundle knows how many there are. */}
            <Button asChild className="w-full">
              <a href={withBase("/auth/login")}>
                <LogIn />
                Sign in
              </a>
            </Button>

            {enforcing ? null : (
              // Worth saying rather than leaving as a surprise: a provider
              // with no roles configured authenticates people and grants them
              // nothing extra, because everything was already visible.
              <p className="text-ink-faint text-center text-[12px]">
                This deployment has no roles configured, so signing in changes
                nothing about what is visible — it only records who is looking.
              </p>
            )}
          </CardContent>
        </Card>

        {/* Where a template would put terms nobody reads, the one property
            worth knowing before you sign in. */}
        <p className="text-ink-faint px-6 text-center text-[12px]">
          kaas-ui is <strong className="font-medium">read-only</strong>. It can
          describe every cluster in this fleet and change none of them.
        </p>
      </div>
    </div>
  )
}
