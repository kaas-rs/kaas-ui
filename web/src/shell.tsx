import { Link, Outlet, useRouterState } from "@tanstack/react-router";
import { useEffect, useState, type ReactNode } from "react";
import {
  BookOpen,
  ChevronRight,
  Cog,
  Layers,
  LayoutGrid,
  ListTree,
  Monitor,
  Moon,
  ScrollText,
  Sun,
  Users,
} from "lucide-react";

import { useCapabilities, useClusters } from "@/api/client";
import type { ClusterCard, Feature } from "@/api/types";
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

/** A cluster section's items, and the capability each one needs to exist. */
const CLUSTER_NAV: {
  label: string;
  to: string;
  icon: typeof LayoutGrid;
  feature?: Feature;
  exact?: boolean;
}[] = [
  { label: "overview", to: "/clusters/$clusterId", icon: LayoutGrid, exact: true },
  { label: "topics", to: "/clusters/$clusterId/topics", icon: ListTree },
  {
    label: "groups",
    to: "/clusters/$clusterId/groups",
    icon: Users,
    feature: "consumerGroups",
  },
  { label: "configs", to: "/clusters/$clusterId/configs", icon: Cog, feature: "configs" },
  { label: "capabilities", to: "/clusters/$clusterId/capabilities", icon: Layers },
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

  useEffect(() => {
    if (active) setOpen(true);
  }, [active]);

  const items = CLUSTER_NAV.filter((item) => {
    if (!item.feature) return true;
    // Until the answer arrives, show it: an item that appears under the cursor
    // is worse than one that errors once.
    const state = capabilities.data?.features.find(
      (feature) => feature.feature === item.feature,
    );
    return state === undefined || state.state === "available";
  });

  return (
    <Collapsible open={open} onOpenChange={setOpen} className="group/cluster">
      <SidebarGroup className="py-1">
        <CollapsibleTrigger asChild>
          <SidebarGroupLabel
            className={cn(
              "h-8 cursor-pointer hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
              active && "text-sidebar-accent-foreground",
            )}
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
            <ChevronRight className="ml-auto size-3.5 transition-transform group-data-[state=open]/cluster:rotate-90" />
          </SidebarGroupLabel>
        </CollapsibleTrigger>

        <CollapsibleContent>
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuSub className="mx-0 border-sidebar-border px-0">
                  {items.map((item) => {
                    const href = item.to.replace("$clusterId", card.id);
                    const isActive = item.exact
                      ? pathname === href
                      : pathname.startsWith(href);
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
  const clusters = useClusters();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const activeCluster = pathname.match(/^\/clusters\/([^/]+)/)?.[1];

  const ThemeIcon = theme === "dark" ? Moon : theme === "light" ? Sun : Monitor;

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
          <SidebarGroup>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton asChild isActive={pathname === "/"} tooltip="Fleet">
                  <Link to="/">
                    <ScrollText aria-hidden />
                    <span>fleet</span>
                  </Link>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton
                  asChild
                  isActive={pathname === "/api-docs"}
                  tooltip="API"
                >
                  <Link to="/api-docs">
                    <BookOpen aria-hidden />
                    <span>api</span>
                  </Link>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroup>

          <Separator className="bg-sidebar-border" />

          <SidebarGroup className="pb-0">
            <SidebarGroupLabel>clusters</SidebarGroupLabel>
          </SidebarGroup>

          {(clusters.data?.items ?? []).map((card) => (
            <ClusterSection key={card.id} card={card} active={card.id === activeCluster} />
          ))}
        </SidebarContent>

        <SidebarFooter>
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

/** Where you are, from the path. Cheap, and always right. */
function Breadcrumb({ pathname }: { pathname: string }) {
  const parts = pathname.split("/").filter(Boolean);
  if (parts.length === 0) {
    return <span className="text-[13px] font-medium">Fleet</span>;
  }
  const crumbs = parts[0] === "clusters" ? parts.slice(1) : parts;
  return (
    <nav className="flex min-w-0 items-center gap-1.5 text-[13px]">
      {crumbs.map((part, index) => (
        <span key={`${part}-${index}`} className="flex min-w-0 items-center gap-1.5">
          {index > 0 ? <span className="text-ink-faint">/</span> : null}
          <span
            className={cn(
              "truncate font-mono",
              index === crumbs.length - 1 ? "text-ink" : "text-ink-muted",
            )}
          >
            {decodeURIComponent(part)}
          </span>
        </span>
      ))}
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
