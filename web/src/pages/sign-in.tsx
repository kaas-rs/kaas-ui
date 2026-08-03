// The one screen an unauthenticated caller sees, when this deployment has an
// identity provider and roles to enforce.
//
// A page with a button rather than an automatic redirect. A redirect is
// tempting — one fewer click — but it fires again the moment a session
// expires, which means it fires in the middle of reading a topic and returns
// you to the fleet with no idea why. A button is a place to stand.

import { LogIn } from "lucide-react";

import { Button } from "@/components/ui/button";
import { withBase } from "@/api/base";

export function SignIn({ enforcing }: { enforcing: boolean }) {
  return (
    <div className="flex min-h-[60vh] flex-col items-center justify-center gap-6 px-6 text-center">
      <div
        className="flex size-12 items-center justify-center rounded-lg font-mono text-2xl font-semibold"
        style={{ background: "var(--rust)", color: "#3B2E2A" }}
        aria-hidden
      >
        k
      </div>

      <div className="max-w-md space-y-2">
        <h1 className="text-[22px] font-semibold tracking-tight">kaas-ui</h1>
        <p className="text-[13px] text-ink-muted">
          A read-only view of every Kafka cluster in this fleet. Sign in to see the ones
          your roles cover.
        </p>
      </div>

      {/* A plain link, not a fetch: the whole point of this navigation is that
          the browser leaves for the provider and comes back with a cookie. */}
      <Button asChild size="lg">
        <a href={withBase("/auth/login")}>
          <LogIn aria-hidden className="size-4" />
          Sign in with GitHub
        </a>
      </Button>

      {enforcing ? null : (
        // Worth saying rather than leaving as a surprise: a provider with no
        // roles configured authenticates people and grants them nothing extra,
        // because everything was already visible.
        <p className="max-w-md text-[12px] text-ink-faint">
          This deployment has no roles configured, so signing in changes nothing about
          what is visible — it only records who is looking.
        </p>
      )}
    </div>
  );
}
