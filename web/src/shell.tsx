import { Link, Outlet, useParams } from "@tanstack/react-router";
import { useEffect, useState, type ReactNode } from "react";
import { useCapabilities, useClusters } from "./api/client";
import type { Feature } from "./api/types";
import { ClusterChip, featureState } from "./components";

type Theme = "light" | "dark" | "system";

function useTheme(): [Theme, (theme: Theme) => void] {
  const [theme, setTheme] = useState<Theme>(() => {
    const stored = localStorage.getItem("kaas-ui-theme");
    return stored === "dark" || stored === "light" ? stored : "system";
  });

  useEffect(() => {
    if (theme === "system") {
      document.documentElement.removeAttribute("data-theme");
      localStorage.removeItem("kaas-ui-theme");
    } else {
      document.documentElement.setAttribute("data-theme", theme);
      localStorage.setItem("kaas-ui-theme", theme);
    }
  }, [theme]);

  return [theme, setTheme];
}

/**
 * The dark nav band is the strongest visual anchor and it is dark in both
 * modes — same as the book's sidebar. It is also where the cluster chip lives,
 * because with a dozen clusters in one UI "which cluster am I looking at" must
 * be answerable without reading the URL.
 */
export function Shell() {
  const [theme, setTheme] = useTheme();
  const params = useParams({ strict: false }) as { clusterId?: string };
  const clusters = useClusters();
  const current = clusters.data?.items.find((card) => card.id === params.clusterId);

  const cycle = () =>
    setTheme(theme === "system" ? "light" : theme === "light" ? "dark" : "system");

  return (
    <div className="min-h-full flex flex-col">
      <header className="bg-surface-nav text-ink-on-nav">
        <div className="max-w-[1400px] mx-auto px-8 h-14 flex items-center gap-6">
          <Link to="/" className="font-semibold tracking-tight text-[15px]">
            kaas<span style={{ color: "var(--color-accent)" }}>-ui</span>
          </Link>

          {current ? (
            <ClusterChip id={current.id} labels={current.labels} />
          ) : (
            <span className="text-[12px] opacity-60">fleet</span>
          )}

          <nav className="flex gap-1 ml-auto items-center text-[13px]">
            {clusters.data?.items.map((card) => (
              <Link
                key={card.id}
                to="/clusters/$clusterId"
                params={{ clusterId: card.id }}
                className="px-2.5 py-1 rounded-sm hover:bg-line-nav"
                activeProps={{
                  style: {
                    background: "var(--color-line-nav)",
                    boxShadow: "inset 0 -2px 0 var(--color-accent)",
                  },
                }}
              >
                {card.id}
              </Link>
            ))}
            <button
              type="button"
              onClick={cycle}
              title={`theme: ${theme}`}
              aria-label={`theme: ${theme}, click to change`}
              className="ml-3 px-2 py-1 rounded-sm hover:bg-line-nav"
            >
              {theme === "dark" ? "☾" : theme === "light" ? "☀" : "◐"}
            </button>
          </nav>
        </div>
      </header>

      <main className="flex-1 max-w-[1400px] w-full mx-auto px-8 py-8">
        <Outlet />
      </main>

      <footer className="border-t border-line">
        <div className="max-w-[1400px] mx-auto px-8 py-4 text-[12px] text-ink-faint flex gap-4">
          <span>read-only by construction — kaas-ui has no mutating endpoint</span>
          <a className="ml-auto hover:underline" href="/api/clusters">
            api
          </a>
        </div>
      </footer>
    </div>
  );
}

/** A tab that knows it might not exist. */
interface TabSpec {
  label: string;
  to: string;
  feature?: Feature;
  exact?: boolean;
}

const TABS: TabSpec[] = [
  { label: "overview", to: "/clusters/$clusterId", exact: true },
  { label: "topics", to: "/clusters/$clusterId/topics" },
  { label: "groups", to: "/clusters/$clusterId/groups", feature: "consumerGroups" },
  { label: "configs", to: "/clusters/$clusterId/configs", feature: "configs" },
  { label: "capabilities", to: "/clusters/$clusterId/capabilities" },
];

/**
 * Tabs are rendered from capabilities, so a cluster that does not answer
 * `ListGroups` shows **no groups tab** rather than a tab that errors on click.
 *
 * The routes still exist and render the explanatory panel, so a URL shared
 * from one cluster and opened against another degrades into an explanation
 * rather than a dead end. That distinction is the whole of "deciding what
 * absence looks like", and it is the one part of this that cannot live in a
 * client library.
 */
export function ClusterTabs({ clusterId }: { clusterId: string }) {
  const capabilities = useCapabilities(clusterId);

  return (
    <nav className="flex gap-1 border-b border-line mb-6 -mt-2">
      {TABS.filter((tab) => {
        if (!tab.feature) return true;
        // Until the answer arrives, show the tab: hiding it and then adding it
        // makes the page jump, and a tab that errors once is better than a
        // navigation target that appears under the cursor.
        const state = featureState(capabilities.data?.features, tab.feature);
        return state === undefined || state.state === "available";
      }).map((tab) => (
        <Link
          key={tab.label}
          to={tab.to}
          params={{ clusterId }}
          activeOptions={{ exact: tab.exact ?? false }}
          className="px-3 py-2 text-[13px] text-ink-muted hover:text-ink"
          activeProps={{
            style: {
              color: "var(--color-ink)",
              boxShadow: "inset 0 -2px 0 var(--color-accent)",
              fontWeight: 500,
            },
          }}
        >
          {tab.label}
        </Link>
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
    <div className="flex items-start justify-between gap-6 mb-6">
      <div>
        <h1 className="text-[22px] font-semibold tracking-tight">{title}</h1>
        {subtitle ? <div className="text-[13px] text-ink-muted mt-1">{subtitle}</div> : null}
      </div>
      {actions ? <div className="flex items-center gap-3">{actions}</div> : null}
    </div>
  );
}
