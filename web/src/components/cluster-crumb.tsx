import { Link } from "@tanstack/react-router"

import type { EnvironmentSection } from "@/api/types"
import { DropdownMenuItem } from "@/components/ui/dropdown-menu"
import { CrumbMenu } from "@/components/crumb-menu"

/**
 * The cluster, and the others in the same environment.
 *
 * Switching keeps the section you were reading — topics stay topics — which is
 * the comparison this whole application is for: the same question asked of two
 * clusters, one click apart. Siblings come from *this* environment only, which
 * is now a fact the path establishes rather than one a lookup by cluster id had
 * to guess at.
 */
export function ClusterCrumb({
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
