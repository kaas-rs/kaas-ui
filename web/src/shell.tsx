import { Link, Outlet, useRouterState } from "@tanstack/react-router";
import { useEffect, useState, type ReactNode } from "react";
import {
  ChevronRight,
  Cog,
  Layers,
  ListTree,
  Monitor,
  Moon,
  Sun,
  Users,
} from "lucide-react";

import { useCapabilities, useClusters, useIdentity } from "@/api/client";
import { SignIn } from "@/pages/sign-in";
import { NavUser } from "@/components/nav-user";
import type { ClusterCard, Feature, Resource } from "@/api/types";
import { clusterTone } from "@/components/domain";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Separator } from "@/components/ui/separator";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
  SidebarProvider,
  SidebarRail,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

/* ------------------------------------------------------------------- theme */

type Theme = "light" | "dark" | "system";

function resolve(theme: Theme): "light" | "dark" {
  if (theme !== "system") return theme;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function apply(theme: Theme) {
  const resolved = resolve(theme);
  document.documentElement.setAttribute("data-theme", resolved);
  document.documentElement.classList.toggle("dark", resolved === "dark");
}

function useTheme(): [Theme, (theme: Theme) => void] {
  const [theme, setTheme] = useState<Theme>(() => {
    const stored = localStorage.getItem("kaas-ui-theme");
    return stored === "dark" || stored === "light" ? stored : "system";
  });

  useEffect(() => {
    if (theme === "system") {
      localStorage.removeItem("kaas-ui-theme");
    } else {
      localStorage.setItem("kaas-ui-theme", theme);
    }
    apply(theme);

    // Following the OS is only meaningful if it keeps following it.
    if (theme !== "system") return;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const listener = () => apply("system");
    media.addEventListener("change", listener);
    return () => media.removeEventListener("change", listener);
  }, [theme]);

  return [theme, setTheme];
}

/* ----------------------------------------------------------------- the nav */

/**
 * A cluster section's items, and the capability each one needs to exist.
 *
 * No overview item: the cluster's own name is the link to it. A section header
 * that is also the section's landing page is one row rather than two saying
 * nearly the same thing.
 */
const CLUSTER_NAV: {
  label: string;
  to: string;
  icon: typeof ListTree;
  feature?: Feature;
  /** The permission this item needs `view` on. */
  resource?: Resource;
}[] = [
  {
    label: "topics",
    to: "/clusters/$clusterId/topics",
    icon: ListTree,
    resource: "topic",
  },
  {
    label: "groups",
    to: "/clusters/$clusterId/groups",
    icon: Users,
    feature: "consumerGroups",
    resource: "consumer",
  },
  {
    label: "configs",
    to: "/clusters/$clusterId/configs",
    icon: Cog,
    feature: "configs",
    resource: "cluster_config",
  },
  {
    label: "capabilities",
    to: "/clusters/$clusterId/capabilities",
    icon: Layers,
    resource: "cluster_config",
  },
];

/**
 * One cluster's section of the sidebar.
 *
 * Items are rendered from the capability answer, so a cluster that cannot
 * answer `ListGroups` has **no groups item** rather than one that errors on
 * click. Put two clusters side by side in this sidebar and the differing item
 * sets are a conformance report — the same claim the capabilities page makes,
 * made permanently visible.
 *
 * The routes still exist, so a URL shared from one cluster and opened against
 * another degrades into an explanation rather than a dead end.
 */
function ClusterSection({ card, active }: { card: ClusterCard; active: boolean }) {
  const capabilities = useCapabilities(card.id);
  const [open, setOpen] = useState(active);
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const tone = clusterTone(card.id, card.labels);
  /** On the cluster's own page, which the name in the header is the link to. */
  const onOverview = pathname === `/clusters/${card.id}`;

  useEffect(() => {
    if (active) setOpen(true);
  }, [active]);

  const items = CLUSTER_NAV.filter((item) => {
    // Two reasons an item can be absent, and they are different claims: the
    // broker cannot answer it, or this caller may not ask. Both end as "no
    // item" rather than an item that errors on click.
    if (item.resource && card.grants[item.resource]?.includes("view") !== true) {
      return false;
    }
    if (!item.feature) return true;
    // Until the answer arrives, show it: an item that appears under the cursor
    // is worse than one that errors once.
    const state = capabilities.data?.features.find(
      (feature) => feature.feature === item.feature,
    );
    return state === undefined || state.state === "available";
  });

  return (
    <>
      {/* The icon rail. Collapsed to icons the sidebar hides group labels and
          submenus by its own rules, so without a row here a dozen clusters
          collapse to an empty strip. One dot per cluster, its colour the same
          identity the expanded header uses, and the tooltip carries the name
          the rail has no room for. */}
      <SidebarMenu className="hidden px-2 group-data-[collapsible=icon]:flex">
        <SidebarMenuItem>
          <SidebarMenuButton
            asChild
            isActive={active}
            tooltip={`${card.id} — ${card.status}`}
          >
            <Link to="/clusters/$clusterId" params={{ clusterId: card.id }}>
              <span
                aria-hidden
                className={cn(
                  // The status ring, rather than a second dot: at sixteen
                  // pixels there is room for one shape, and a cluster nobody
                  // can reach must not look like one that is fine.
                  "size-4 shrink-0 rounded-full ring-2 ring-offset-0",
                  card.status === "ready" && "ring-transparent",
                  card.status === "connecting" && "ring-warn",
                  card.status === "unreachable" && "ring-danger",
                )}
                style={{ background: tone.bg }}
              />
              <span>{card.id}</span>
            </Link>
          </SidebarMenuButton>
        </SidebarMenuItem>
      </SidebarMenu>

      <Collapsible
        open={open}
        onOpenChange={setOpen}
        className="group/cluster group-data-[collapsible=icon]:hidden"
      >
        <SidebarGroup className="py-1">
          {/* The name navigates, the chevron folds. Two targets in one row
              because they are two different intentions: "show me this cluster"
              and "get these items out of my way" — and a header that did both on
              one click would do the wrong one half the time. */}
          <SidebarGroupLabel
            className={cn(
              "h-8 gap-0 pr-1",
              active && "text-sidebar-accent-foreground",
              onOverview && "bg-sidebar-accent text-sidebar-accent-foreground",
            )}
          >
            <Link
              to="/clusters/$clusterId"
              params={{ clusterId: card.id }}
              // Following the name is also asking to see what is under it.
              onClick={() => setOpen(true)}
              className="flex min-w-0 flex-1 items-center rounded-sm hover:text-sidebar-accent-foreground"
            >
              <span
                aria-hidden
                className="mr-2 size-2 shrink-0 rounded-full"
                style={{ background: tone.bg }}
              />
              <span className="truncate font-mono">{card.id}</span>
              <span
                aria-hidden
                title={card.status}
                className={cn(
                  "ml-1.5 size-1.5 shrink-0 rounded-full",
                  card.status === "ready" && "bg-ok",
                  card.status === "connecting" && "bg-warn",
                  card.status === "unreachable" && "bg-danger",
                )}
              />
            </Link>

            <CollapsibleTrigger
              aria-label={`${open ? "Collapse" : "Expand"} ${card.id}`}
              className="ml-1 shrink-0 cursor-pointer rounded-sm p-1 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
            >
              <ChevronRight
                aria-hidden
                className="size-3.5 transition-transform group-data-[state=open]/cluster:rotate-90"
              />
            </CollapsibleTrigger>
          </SidebarGroupLabel>

          <CollapsibleContent>
            <SidebarGroupContent>
              <SidebarMenu>
                <SidebarMenuItem>
                  <SidebarMenuSub className="mx-0 border-sidebar-border px-0">
                    {items.map((item) => {
                      const href = item.to.replace("$clusterId", card.id);
                      const isActive = pathname.startsWith(href);
                      return (
                        <SidebarMenuSubItem key={item.label}>
                          <SidebarMenuSubButton asChild isActive={isActive}>
                            <Link to={item.to} params={{ clusterId: card.id }}>
                              <item.icon aria-hidden className="size-3.5" />
                              <span>{item.label}</span>
                            </Link>
                          </SidebarMenuSubButton>
                        </SidebarMenuSubItem>
                      );
                    })}
                  </SidebarMenuSub>
                </SidebarMenuItem>
              </SidebarMenu>
            </SidebarGroupContent>
          </CollapsibleContent>
        </SidebarGroup>
      </Collapsible>
    </>
  );
}

/**
 * The dark nav band is the strongest visual anchor and it is dark in both
 * modes — same as the book's sidebar. It is also where cluster identity lives,
 * because with a dozen clusters in one UI "which cluster am I looking at" must
 * be answerable without reading the URL.
 */
export function Shell() {
  const [theme, setTheme] = useTheme();
  const identity = useIdentity();
  const clusters = useClusters();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const activeCluster = pathname.match(/^\/clusters\/([^/]+)/)?.[1];

  const ThemeIcon = theme === "dark" ? Moon : theme === "light" ? Sun : Monitor;

  // Signed out, on a deployment that enforces roles: there is nothing behind
  // this to render. An open deployment never reaches here, and neither does
  // one whose provider is configured but whose roles are not — that caller
  // already sees everything, so demanding a login first would be theatre.
  const me = identity.data;
  if (me && me.loginAvailable && me.enforcing && !me.authenticated) {
    return <SignIn enforcing={me.enforcing} />;
  }

  return (
    <SidebarProvider>
      <Sidebar collapsible="icon">
        <SidebarHeader>
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
                    <span className="truncate text-[11px] opacity-70">read-only</span>
                  </div>
                </Link>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarHeader>

        <SidebarContent>
          {/* No fleet item: the mark in the header is the way back to it, and
              a nav whose first row is "everything" competes with the list of
              everything directly beneath it. */}
          <SidebarGroup className="pb-0">
            <SidebarGroupLabel>clusters</SidebarGroupLabel>
          </SidebarGroup>

          {(clusters.data?.items ?? []).map((card) => (
            <ClusterSection key={card.id} card={card} active={card.id === activeCluster} />
          ))}
        </SidebarContent>

        <SidebarFooter>
          {/* Rendered whenever `/api/me` has answered, signed in or not:
              anonymous is a real state here and hiding it makes "who am I"
              unanswerable on exactly the deployments where it is least
              obvious. */}
          {me ? <NavUser identity={me} /> : null}
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton
                onClick={() =>
                  setTheme(
                    theme === "system" ? "light" : theme === "light" ? "dark" : "system",
                  )
                }
                tooltip={`theme: ${theme}`}
              >
                <ThemeIcon aria-hidden />
                <span>{theme}</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarFooter>
        <SidebarRail />
      </Sidebar>

      <SidebarInset className="min-w-0">
        <header className="flex h-12 shrink-0 items-center gap-2 border-b px-4">
          <SidebarTrigger className="-ml-1" />
          <Separator orientation="vertical" className="mr-1 h-4" />
          <Breadcrumb pathname={pathname} />
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="sm" asChild className="ml-auto text-[12px]">
                <a href="/api/openapi.json">openapi.json</a>
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              the document every endpoint above is described by
            </TooltipContent>
          </Tooltip>
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
  );
}

/**
 * Where you are, from the path. Cheap, and always right.
 *
 * Every crumb but the last is a link to its own prefix, which is what makes
 * this the way *up*: from a message on a topic to that topic, to the topic
 * list, to the cluster. The href is built from the raw segments rather than
 * the decoded ones, because a topic named `a/b` is `a%2Fb` in the path and
 * only stays one segment while it is spelled that way.
 */
function Breadcrumb({ pathname }: { pathname: string }) {
  const parts = pathname.split("/").filter(Boolean);
  if (parts.length === 0) {
    return <span className="text-[13px] font-medium">Fleet</span>;
  }

  // `/clusters` is a prefix, not a page — the cluster id is the first crumb.
  const skip = parts[0] === "clusters" ? 1 : 0;
  const crumbs = parts.slice(skip).map((part, index) => ({
    part,
    href: `/${parts.slice(0, skip + index + 1).join("/")}`,
  }));

  return (
    <nav aria-label="Breadcrumb" className="flex min-w-0 items-center gap-1.5 text-[13px]">
      {crumbs.map(({ part, href }, index) => {
        const last = index === crumbs.length - 1;
        return (
          <span key={href} className="flex min-w-0 items-center gap-1.5">
            {index > 0 ? <span className="text-ink-faint">/</span> : null}
            {last ? (
              <span aria-current="page" className="truncate font-mono text-ink">
                {decodeURIComponent(part)}
              </span>
            ) : (
              <Link
                to={href}
                className="truncate font-mono text-ink-muted hover:text-ink hover:underline"
              >
                {decodeURIComponent(part)}
              </Link>
            )}
          </span>
        );
      })}
    </nav>
  );
}

export function PageTitle({
  title,
  subtitle,
  actions,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div className="mb-6 flex items-start justify-between gap-6">
      <div className="min-w-0">
        <h1 className="truncate text-[22px] font-semibold tracking-tight">{title}</h1>
        {subtitle ? (
          <div className="mt-1 text-[13px] text-ink-muted">{subtitle}</div>
        ) : null}
      </div>
      {actions ? <div className="flex items-center gap-3">{actions}</div> : null}
    </div>
  );
}
