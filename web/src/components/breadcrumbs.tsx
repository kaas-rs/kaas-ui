// Where you are, and what else you could have been looking at.
//
// The path is the hierarchy now — `/environments/{env}/clusters/{id}/…` and
// `/environments/{env}/schema-registries/{id}/…` — so the crumbs are parsed
// from it rather than reconstructed. That is the change worth knowing about:
// the environment used to be *fetched*, because a flat `/clusters/{id}` could
// not say which environment a cluster was in, and the crumb had to appear late
// and could be wrong in the gap. It is a path segment now, so it is right
// immediately. The fleet is still fetched, but only for **names** and for the
// siblings in the two menus — never to decide what the chain is.
//
// Two crumbs carry a dropdown, and both list siblings rather than children: an
// environment beside this one, a cluster beside this one. That is the move a
// breadcrumb is uniquely placed to offer — you are already reading the row
// that says which one you are in.

import { Link } from "@tanstack/react-router"
import { ChevronDown } from "lucide-react"

import { useFleet } from "@/api/client"
import type { EnvironmentSection } from "@/api/types"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { chooseEnvironment } from "@/lib/environment"

/** The cluster sub-pages a sibling cluster also has a route for. */
const SECTIONS = new Set(["topics", "groups", "configs", "capabilities"])

/**
 * What the path says, before any of it is looked up.
 *
 * Three shapes, and the third is everything else — `/settings` and `/account`
 * hang off the fleet with nothing in between.
 */
interface Trail {
  envId?: string
  /** The cluster subtree. */
  clusterId?: string
  /** The registry subtree, which is a peer of the cluster one. */
  registryId?: string
  /** Whatever follows the addressed thing, in order. */
  tail: string[]
  /** The path prefix `tail[i]` hangs off, for building an href. */
  prefix: string[]
}

function read(parts: string[]): Trail {
  if (parts[0] !== "environments" || !parts[1]) {
    return { tail: parts, prefix: [] }
  }
  const envId = parts[1]
  if (parts[2] === "clusters" && parts[3]) {
    return {
      envId,
      clusterId: parts[3],
      tail: parts.slice(4),
      prefix: parts.slice(0, 4),
    }
  }
  // `subjects` is a collection segment with no page of its own, so it is
  // dropped rather than rendered as a crumb nobody can click.
  if (parts[2] === "schema-registries" && parts[3]) {
    const rest = parts.slice(4)
    const tail = rest[0] === "subjects" ? rest.slice(1) : rest
    return {
      envId,
      registryId: parts[3],
      tail,
      prefix: parts.slice(0, rest[0] === "subjects" ? 5 : 4),
    }
  }
  return { envId, tail: parts.slice(2), prefix: parts.slice(0, 2) }
}

export function Breadcrumbs({ pathname }: { pathname: string }) {
  const fleet = useFleet()
  const sections = fleet.data?.items ?? []

  const parts = pathname.split("/").filter(Boolean)
  const trail = read(parts)
  const environment = trail.envId
    ? sections.find((section) => section.id === trail.envId)
    : undefined
  // The registry's *name*, which only the fleet knows. The id is in the path,
  // so the crumb renders either way rather than waiting for the answer.
  const registry = environment?.schemaRegistries.find(
    (entry) => entry.registry.id === trail.registryId
  )

  const leafIsHere = trail.tail.length === 0

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
        <Link
          to="/"
          className="shrink-0 text-ink-muted hover:text-ink hover:underline"
        >
          Fleet
        </Link>
      )}

      {trail.envId ? (
        <>
          <Separator />
          <EnvironmentCrumb
            envId={trail.envId}
            current={environment}
            sections={sections}
            last={leafIsHere && !trail.clusterId && !trail.registryId}
          />
        </>
      ) : null}

      {trail.clusterId ? (
        <>
          <Separator />
          <ClusterCrumb
            envId={trail.envId ?? ""}
            id={trail.clusterId}
            environment={environment}
            // Only the section, never deeper: a topic name does not exist on
            // the cluster next to this one, and sending someone to a "topic
            // not found" page is a worse answer than sending them to the list.
            section={
              trail.tail[0] && SECTIONS.has(trail.tail[0])
                ? trail.tail[0]
                : undefined
            }
            last={leafIsHere}
          />
        </>
      ) : null}

      {trail.registryId ? (
        <>
          <Separator />
          <RegistryCrumb
            envId={trail.envId ?? ""}
            id={trail.registryId}
            label={registry?.registry.name ?? trail.registryId}
            environment={environment}
            last={leafIsHere}
          />
        </>
      ) : null}

      {trail.tail.map((part, index) => {
        const href = `/${[...trail.prefix, ...trail.tail.slice(0, index + 1)].join("/")}`
        const last = index === trail.tail.length - 1
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
        )
      })}
    </nav>
  )
}

function Separator() {
  return (
    <span aria-hidden className="shrink-0 text-ink-faint">
      /
    </span>
  )
}

/** A crumb whose name opens a menu of the things beside it. */
function CrumbMenu({
  label,
  mono,
  current,
  children,
}: {
  label: string
  mono?: boolean
  current?: boolean
  children: React.ReactNode
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
  )
}

/**
 * The environment, and the others.
 *
 * It goes somewhere now. It used to lead to the fleet, because an environment
 * had no page of its own — the URL existed only to carry a parameter down to
 * the clusters beneath it. `/environments/{env}` is a page, so the crumb is
 * the link its position always implied, and choosing a sibling opens that
 * environment rather than dropping you back at the top.
 *
 * It still switches the sidebar, because arriving in one environment with the
 * nav showing another would be the application disagreeing with itself.
 */
function EnvironmentCrumb({
  envId,
  current,
  sections,
  last,
}: {
  envId: string
  current: EnvironmentSection | undefined
  sections: EnvironmentSection[]
  last: boolean
}) {
  // The id until the fleet answers with a name. Never blank, never late.
  const label = current?.name ?? envId

  if (sections.length < 2) {
    return last ? (
      <span aria-current="page" className="truncate text-ink">
        {label}
      </span>
    ) : (
      <Link
        to="/environments/$envId"
        params={{ envId }}
        className="truncate text-ink-muted hover:text-ink hover:underline"
      >
        {label}
      </Link>
    )
  }

  return (
    <CrumbMenu label={label} current={last}>
      {sections.map((section) => (
        <DropdownMenuItem key={section.id} asChild>
          <Link
            to="/environments/$envId"
            params={{ envId: section.id }}
            onClick={() => chooseEnvironment(section.id)}
          >
            <span className="truncate">{section.name}</span>
          </Link>
        </DropdownMenuItem>
      ))}
    </CrumbMenu>
  )
}

/**
 * The cluster, and the others in the same environment.
 *
 * Switching keeps the section you were reading — topics stay topics — which is
 * the comparison this whole application is for: the same question asked of two
 * clusters, one click apart. Siblings come from *this* environment only, which
 * is now a fact the path establishes rather than one a lookup by cluster id had
 * to guess at.
 */
function ClusterCrumb({
  envId,
  id,
  environment,
  section,
  last,
}: {
  envId: string
  id: string
  environment: EnvironmentSection | undefined
  section: string | undefined
  last: boolean
}) {
  const siblings = environment?.clusters ?? []

  if (siblings.length < 2) {
    return last ? (
      <span aria-current="page" className="truncate font-mono text-ink">
        {id}
      </span>
    ) : (
      <Link
        to="/environments/$envId/clusters/$clusterId"
        params={{ envId, clusterId: id }}
        className="truncate font-mono text-ink-muted hover:text-ink hover:underline"
      >
        {id}
      </Link>
    )
  }

  return (
    <CrumbMenu label={id} mono current={last}>
      {siblings.map((card) => (
        <DropdownMenuItem key={card.id} asChild>
          {/* A plain `string`, not a template-literal type: the router's `to`
              accepts one, and spelling the typed form here would need a
              separate branch per section for no extra safety. The environment
              is in it — a sibling is a sibling *within* one, and the flat form
              this used to build no longer routes anywhere. */}
          <Link
            to={
              (section
                ? `/environments/${envId}/clusters/${card.id}/${section}`
                : `/environments/${envId}/clusters/${card.id}`) as string
            }
            className="font-mono"
          >
            {/* Red rather than a word: the row is one line and the name is
                already on it, so the colour is the whole message. On the span
                rather than the link, where no class of the menu item's own can
                out-order it. */}
            <span
              className={
                card.status === "unreachable"
                  ? "truncate text-danger"
                  : "truncate"
              }
            >
              {card.id}
            </span>
          </Link>
        </DropdownMenuItem>
      ))}
    </CrumbMenu>
  )
}

/**
 * The schema registry, and the others in the same environment.
 *
 * A peer of the cluster crumb rather than a child of one, which is what the
 * route says and what a registry is: it serves the environment, and the
 * clusters that decode against it are its users rather than its parents.
 *
 * Labelled with the registry's name, falling back to the id until the fleet
 * answers — the crumb must never be blank, because it is the row telling you
 * where you are.
 */
function RegistryCrumb({
  envId,
  id,
  label,
  environment,
  last,
}: {
  envId: string
  id: string
  label: string
  environment: EnvironmentSection | undefined
  last: boolean
}) {
  const siblings = environment?.schemaRegistries ?? []

  if (siblings.length < 2) {
    return last ? (
      <span aria-current="page" className="truncate text-ink">
        {label}
      </span>
    ) : (
      <Link
        to="/environments/$envId/schema-registries/$registryId"
        params={{ envId, registryId: id }}
        className="truncate text-ink-muted hover:text-ink hover:underline"
      >
        {label}
      </Link>
    )
  }

  return (
    <CrumbMenu label={label} current={last}>
      {siblings.map((entry) => (
        <DropdownMenuItem key={entry.registry.id} asChild>
          <Link
            to="/environments/$envId/schema-registries/$registryId"
            params={{ envId, registryId: entry.registry.id }}
          >
            <span className="truncate">{entry.registry.name}</span>
          </Link>
        </DropdownMenuItem>
      ))}
    </CrumbMenu>
  )
}
