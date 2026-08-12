import { Link } from "@tanstack/react-router"

import type { EnvironmentSection } from "@/api/types"
import { DropdownMenuItem } from "@/components/ui/dropdown-menu"
import { chooseEnvironment } from "@/lib/environment"
import { CrumbMenu } from "@/components/crumb-menu"

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
export function EnvironmentCrumb({
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
