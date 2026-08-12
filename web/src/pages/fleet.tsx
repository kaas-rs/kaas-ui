import { useFleet } from "@/api/client"
import { Empty, Spinner } from "@/components/domain"
import { Card } from "@/components/ui/card"
import { PageTitle } from "@/components/page-title"
import { EnvironmentSection } from "@/features/fleet/environment-section"
import { plural } from "@/features/fleet/plural"

/**
 * The fleet, one section per environment.
 *
 * The sections arrive assembled, in order, from `/api/fleet` — declared
 * environments first, in declared order, because "dev, staging, prod" is not
 * recoverable from three strings by any sort this page could apply. An
 * environment holding nothing this caller may see is not in the response at
 * all, so there is no empty heading here to report that prod exists.
 */
export function FleetPage() {
  const fleet = useFleet()

  if (fleet.isLoading) return <Spinner label="loading the fleet" />
  if (fleet.error) {
    return (
      <Card className="p-5 text-danger">
        the fleet could not be loaded: {String(fleet.error)}
      </Card>
    )
  }

  const sections = fleet.data?.items ?? []
  const clusters = sections.reduce(
    (total, section) => total + section.clusters.length,
    0
  )
  const resources = sections.reduce(
    (total, section) => total + section.resources.length,
    0
  )

  return (
    <>
      <PageTitle
        title="Fleet"
        subtitle={
          <>
            {plural(clusters, "cluster")}
            {resources > 0
              ? ` and ${plural(resources, "other resource")}`
              : null}
            {sections.length > 0
              ? ` across ${plural(sections.length, "environment")}`
              : null}
          </>
        }
      />

      {sections.length === 0 ? (
        <Empty>nothing configured is visible to you</Empty>
      ) : (
        sections.map((section) => (
          <EnvironmentSection key={section.id} section={section} />
        ))
      )}
    </>
  )
}
