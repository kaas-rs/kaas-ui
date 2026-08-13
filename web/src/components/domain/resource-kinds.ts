import type { ComponentType } from "react"
import { Boxes, Cable, Globe } from "lucide-react"
import { RiBookShelfLine } from "react-icons/ri"
import { SiApachekafka, SiMqtt } from "react-icons/si"

import type { ResourceKind } from "@/api/types"

/**
 * What every icon in this table has to be, and all it has to be.
 *
 * Two libraries meet here — lucide for the generic kinds, react-icons for the
 * three that have a mark of their own — and neither type is the other's. The
 * call sites pass a class and hide the glyph from a screen reader, so that is
 * the contract, and both satisfy it. Sizing is CSS at the call site rather than
 * a prop, which is why no `size` appears: react-icons default to `1em` and
 * lucide to 24, and a `size-4` in the class list settles both.
 */
export type ResourceIcon = ComponentType<{
  className?: string
  "aria-hidden"?: boolean
}>

/**
 * What a Kafka cluster looks like wherever one is listed.
 *
 * Here rather than at each call site so the nav and the fleet cannot drift
 * into two different pictures of the same thing — and so that the one glyph
 * that means "cluster" is never quietly reused for a resource below. Kafka's
 * own mark, because a broker is the one thing on these screens that has one
 * and every reader already knows it.
 */
export const CLUSTER_ICON: ResourceIcon = SiApachekafka

/**
 * Icon and wording per non-cluster resource kind.
 *
 * One table, two readers: the fleet card and the sidebar. A registry that is a
 * cylinder on one screen and a box on the other is a second thing to learn.
 *
 * A brand mark where the thing has one and a generic glyph where it does not:
 * MQTT is a protocol with a logo, Kafka Connect and a REST proxy are not — and
 * a registry is shelved schemas, which says more about what it holds than a
 * database cylinder did.
 */
export const RESOURCE_KINDS: Record<
  ResourceKind,
  { icon: ResourceIcon; label: string }
> = {
  schema_registry: { icon: RiBookShelfLine, label: "schema registry" },
  mqtt_broker: { icon: SiMqtt, label: "MQTT broker" },
  kafka_connect: { icon: Cable, label: "Kafka Connect" },
  rest_proxy: { icon: Globe, label: "REST proxy" },
  other: { icon: Boxes, label: "resource" },
}

/**
 * The order kinds appear in, nearest the brokers first: the registry every
 * cluster in the environment reads, then the two things that sit directly in
 * front of one, then a broker of another protocol entirely, then the rest.
 */
const KIND_ORDER: ResourceKind[] = [
  "schema_registry",
  "kafka_connect",
  "rest_proxy",
  "mqtt_broker",
  "other",
]

/**
 * Group a mixed list so like kinds sit together.
 *
 * Stable, so the configured order survives *within* a kind — three registries
 * stay in the order someone wrote them. Used by both the nav and the fleet, so
 * the two never disagree about what comes after what.
 */
export function byResourceKind<T extends { kind: ResourceKind }>(
  resources: T[]
): T[] {
  const rank = (kind: ResourceKind) => {
    const index = KIND_ORDER.indexOf(kind)
    return index === -1 ? KIND_ORDER.length : index
  }
  return [...resources].sort((a, b) => rank(a.kind) - rank(b.kind))
}
