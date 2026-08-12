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

import { useFleet } from "@/api/client"
import { ClusterCrumb } from "@/components/cluster-crumb"
import { EnvironmentCrumb } from "@/components/environment-crumb"
import { RegistryCrumb } from "@/components/registry-crumb"

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
