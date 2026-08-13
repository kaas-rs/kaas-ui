import type { ReactNode } from "react"

import type { Capabilities, Feature } from "@/api/types"
import { TabsTrigger } from "@/components/ui/tabs"
import { UnsupportedApiPanel } from "./unsupported-api-panel"

/**
 * Whether a feature can be shown here, from the projection.
 *
 * `undefined` while the answer is in flight, and that is deliberately not
 * `false`: a tab that appears a second after the page settles moves everything
 * beside it under the cursor, which is worse than one that turns out to explain
 * itself. Absent from the list means the same — a build that has no opinion
 * about a feature does not get to hide it.
 */
export function capabilityState(
  capabilities: Capabilities | undefined,
  feature: Feature
) {
  return capabilities?.features.find((entry) => entry.feature === feature)
}

/**
 * A tab that knows it might not exist.
 *
 * Three states, and the third is the one that makes this a component rather
 * than a condition: available renders normally, unsupported renders **nothing
 * at all**, and unsupported-but-routed — someone followed a link straight at
 * it — renders the panel naming both version ranges. A hidden tab whose URL
 * 500s is a dead end for the one person who most needs to know why.
 */
export function CapabilityTab({
  value,
  label,
  capabilities,
  feature,
}: {
  value: string
  label: ReactNode
  capabilities: Capabilities | undefined
  feature: Feature
}) {
  const state = capabilityState(capabilities, feature)
  if (state && state.state !== "available") return null
  return <TabsTrigger value={value}>{label}</TabsTrigger>
}

/**
 * What a screen renders in place of itself when its api is not there, or
 * `null` when the feature is available and the screen should render itself.
 *
 * **A function and not a component**, which is the whole reason it is written
 * this way: `const gate = <CapabilityGate …/>; if (gate)` is always true — a
 * JSX element is an object, and a component that returns `null` still produces
 * one. Called, this returns the `null` a caller can branch on.
 */
export function capabilityGate(
  capabilities: Capabilities | undefined,
  feature: Feature,
  /** What is unavailable, in the reader's words rather than the api's. */
  what: string
): ReactNode | null {
  const state = capabilityState(capabilities, feature)
  if (!state || state.state === "available") return null
  return (
    <UnsupportedApiPanel
      api={state.api}
      apiKey={state.apiKey}
      broker={state.broker}
      ours={state.ours}
      what={what}
    />
  )
}
