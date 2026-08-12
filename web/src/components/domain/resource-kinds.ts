import { Boxes, Cable, Database, Globe, Radio, Server } from "lucide-react"
import type { LucideIcon } from "lucide-react"

import type { ResourceKind } from "@/api/types"

/**
 * What a Kafka cluster looks like wherever one is listed.
 *
 * Here rather than at each call site so the nav and the fleet cannot drift
 * into two different pictures of the same thing — and so that the one glyph
 * that means "cluster" is never quietly reused for a resource below.
 */
export const CLUSTER_ICON: LucideIcon = Server

/**
 * Icon and wording per non-cluster resource kind.
 *
 * One table, two readers: the fleet card and the sidebar. A registry that is a
 * cylinder on one screen and a box on the other is a second thing to learn.
 */
export const RESOURCE_KINDS: Record<
  ResourceKind,
  { icon: LucideIcon; label: string }
> = {
  schema_registry: { icon: Database, label: "schema registry" },
  mqtt_broker: { icon: Radio, label: "MQTT broker" },
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
