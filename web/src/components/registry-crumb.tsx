import { Link } from "@tanstack/react-router"

import type { EnvironmentSection } from "@/api/types"
import { DropdownMenuItem } from "@/components/ui/dropdown-menu"
import { CrumbMenu } from "@/components/crumb-menu"

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
export function RegistryCrumb({
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
