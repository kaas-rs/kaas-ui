// The one screen an unauthenticated caller sees, when this deployment has an
// identity provider and roles to enforce.
//
// A page with a button rather than an automatic redirect. A redirect is
// tempting — one fewer click — but it fires again the moment a session
// expires, which means it fires in the middle of reading a topic and returns
// you to the fleet with no idea why. A button is a place to stand.

import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { withBase } from "@/api/base";

/**
 * GitHub's mark, inline.
 *
 * Not from `lucide-react`: it dropped brand icons, and the ones it kept are
 * drawn to a stroke grid this is not. Inlining one path is cheaper than a
 * second icon dependency for a single glyph.
 */
function GithubMark() {
  return (
    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" aria-hidden>
      <path
        d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12"
        fill="currentColor"
      />
    </svg>
  );
}

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
                with a cookie. */}
            <Button asChild className="w-full">
              <a href={withBase("/auth/login")}>
                <GithubMark />
                Login with GitHub
              </a>
            </Button>

            {enforcing ? null : (
              // Worth saying rather than leaving as a surprise: a provider
              // with no roles configured authenticates people and grants them
              // nothing extra, because everything was already visible.
              <p className="text-ink-faint text-center text-[12px]">
                This deployment has no roles configured, so signing in changes nothing
                about what is visible — it only records who is looking.
              </p>
            )}
          </CardContent>
        </Card>

        {/* Where a template would put terms nobody reads, the one property
            worth knowing before you sign in. */}
        <p className="text-ink-faint px-6 text-center text-[12px]">
          kaas-ui is <strong className="font-medium">read-only</strong>. It can describe
          every cluster in this fleet and change none of them.
        </p>
      </div>
    </div>
  );
}
