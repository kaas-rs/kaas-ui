// One environment, and everything in it.
//
// The middle of the hierarchy. `/environments/{env}` used to render nothing —
// the route existed only to carry a path parameter down to the clusters under
// it — which made the breadcrumb's environment crumb the one link in the chain
// that had nowhere to go.
//
// It renders the fleet's own section, unchanged: the fleet is every
// environment and this is one of them, so a second layout would be two things
// to keep in step for the same content.

import { useEnvironment } from "@/api/client"
import { ApiError } from "@/api/client"
import { Empty, Spinner } from "@/components/domain"
import { PageTitle } from "@/components/page-title"
import { Card } from "@/components/ui/card"
import { Environment as EnvironmentSectionView } from "@/pages/fleet"

export function EnvironmentPage({ envId }: { envId: string }) {
  const environment = useEnvironment(envId)

  if (environment.isLoading) return <Spinner label={`loading ${envId}`} />

  // A 404 here is the visibility rule, not a broken link: an environment
  // nothing visible lives in does not exist for this caller, which is the same
  // answer a cluster they may not see gives. Saying "not found" rather than
  // "forbidden" is what keeps environment ids unenumerable.
  if (environment.error) {
    const notFound =
      environment.error instanceof ApiError && environment.error.status === 404
    return (
      <>
        <PageTitle title={envId} />
        {notFound ? (
          <Empty>
            No environment <span className="font-mono">{envId}</span> is visible
            to you.
          </Empty>
        ) : (
          <Card className="p-5 text-danger">
            {(environment.error as Error).message}
          </Card>
        )}
      </>
    )
  }

  const section = environment.data?.items[0]
  if (!section) return <Empty>this environment holds nothing you can see</Empty>

  return <EnvironmentSectionView section={section} />
}
