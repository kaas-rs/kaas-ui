// Where you are, and what else you could have been looking at.
//
// The path already says where you are, so the crumbs are derived from it and
// are always right. What the path cannot say is which *environment* a cluster
// belongs to — that is a fact about the fleet, not about the URL — so the
// second crumb is fetched rather than parsed, and appears only once the answer
// has arrived rather than guessing and correcting itself.
//
// Two crumbs carry a dropdown, and both list siblings rather than children: an
// environment beside this one, a cluster beside this one. That is the move a
// breadcrumb is uniquely placed to offer — you are already reading the row
// that says which one you are in.

import { Link } from "@tanstack/react-router";
import { ChevronDown } from "lucide-react";

import { useFleet } from "@/api/client";
import type { EnvironmentSection } from "@/api/types";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { chooseEnvironment } from "@/lib/environment";

/** The cluster sub-pages a sibling cluster also has a route for. */
const SECTIONS = new Set(["topics", "groups", "configs", "capabilities"]);

export function Breadcrumbs({ pathname }: { pathname: string }) {
  const fleet = useFleet();
  const sections = fleet.data?.items ?? [];

  const parts = pathname.split("/").filter(Boolean);
  const clusterId = parts[0] === "clusters" ? parts[1] : undefined;
  // Everything after the cluster id, or the whole path when this is not a
  // cluster page at all — `/settings` is `Fleet / settings`.
  const tail = clusterId ? parts.slice(2) : parts;
  const environment = clusterId
    ? sections.find((section) => section.clusters.some((card) => card.id === clusterId))
    : undefined;

  return (
    <nav
      aria-label="Breadcrumb"
      className="flex min-w-0 items-center gap-1.5 text-[13px]"
    >
      {parts.length === 0 ? (
        <span aria-current="page" className="font-medium">
          Fleet
        </span>
      ) : (
        <Link to="/" className="shrink-0 text-ink-muted hover:text-ink hover:underline">
          Fleet
        </Link>
      )}

      {environment ? (
        <>
          <Separator />
          <EnvironmentCrumb current={environment} sections={sections} />
        </>
      ) : null}

      {clusterId ? (
        <>
          <Separator />
          <ClusterCrumb
            id={clusterId}
            environment={environment}
            // Only the section, never deeper: a topic name does not exist on
            // the cluster next to this one, and sending someone to a "topic
            // not found" page is a worse answer than sending them to the list.
            section={tail[0] && SECTIONS.has(tail[0]) ? tail[0] : undefined}
            last={tail.length === 0}
          />
        </>
      ) : null}

      {tail.map((part, index) => {
        const href = `/${parts.slice(0, parts.length - tail.length + index + 1).join("/")}`;
        const last = index === tail.length - 1;
        return (
          <span key={href} className="flex min-w-0 items-center gap-1.5">
            <Separator />
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

function Separator() {
  return (
    <span aria-hidden className="shrink-0 text-ink-faint">
      /
    </span>
  );
}

/** A crumb whose name opens a menu of the things beside it. */
function CrumbMenu({
  label,
  mono,
  current,
  children,
}: {
  label: string;
  mono?: boolean;
  current?: boolean;
  children: React.ReactNode;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        className={[
          "flex min-w-0 cursor-pointer items-center gap-0.5 rounded-sm hover:text-ink hover:underline",
          mono ? "font-mono" : "",
          current ? "text-ink" : "text-ink-muted",
        ].join(" ")}
        {...(current ? { "aria-current": "page" as const } : {})}
      >
        <span className="truncate">{label}</span>
        <ChevronDown aria-hidden className="size-3 shrink-0 opacity-60" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-48">
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * The environment this cluster is in, and the others.
 *
 * Choosing one goes to the fleet. There is no page for an environment on its
 * own — the fleet is where its clusters and its registries are — and no
 * cluster in it is the counterpart of the one being left, so anywhere else
 * would be a guess. It also switches the sidebar, because ending up on the
 * fleet with the nav still showing the environment you just left would be the
 * application disagreeing with itself.
 */
function EnvironmentCrumb({
  current,
  sections,
}: {
  current: EnvironmentSection;
  sections: EnvironmentSection[];
}) {
  return (
    <CrumbMenu label={current.name}>
      {sections.map((section) => (
        <DropdownMenuItem key={section.id} asChild>
          <Link to="/" onClick={() => chooseEnvironment(section.id)}>
            <span className="truncate">{section.name}</span>
          </Link>
        </DropdownMenuItem>
      ))}
    </CrumbMenu>
  );
}

/**
 * The cluster, and the others in the same environment.
 *
 * Switching keeps the section you were reading — topics stay topics — which is
 * the comparison this whole application is for: the same question asked of two
 * clusters, one click apart. Where the environment is not known yet the menu
 * is skipped rather than filled with every cluster in the fleet.
 */
function ClusterCrumb({
  id,
  environment,
  section,
  last,
}: {
  id: string;
  environment: EnvironmentSection | undefined;
  section: string | undefined;
  last: boolean;
}) {
  const siblings = environment?.clusters ?? [];

  if (siblings.length < 2) {
    return last ? (
      <span aria-current="page" className="truncate font-mono text-ink">
        {id}
      </span>
    ) : (
      <Link
        to="/clusters/$clusterId"
        params={{ clusterId: id }}
        className="truncate font-mono text-ink-muted hover:text-ink hover:underline"
      >
        {id}
      </Link>
    );
  }

  return (
    <CrumbMenu label={id} mono current={last}>
      {siblings.map((card) => (
        <DropdownMenuItem key={card.id} asChild>
          {/* A plain `string`, not a template-literal type: the router's `to`
              accepts one, and spelling the typed form here would need a
              separate branch per section for no extra safety. */}
          <Link
            to={
              (section
                ? `/clusters/${card.id}/${section}`
                : `/clusters/${card.id}`) as string
            }
            className="font-mono"
          >
            {/* Red rather than a word: the row is one line and the name is
                already on it, so the colour is the whole message. On the span
                rather than the link, where no class of the menu item's own can
                out-order it. */}
            <span
              className={card.status === "unreachable" ? "truncate text-danger" : "truncate"}
            >
              {card.id}
            </span>
          </Link>
        </DropdownMenuItem>
      ))}
    </CrumbMenu>
  );
}
