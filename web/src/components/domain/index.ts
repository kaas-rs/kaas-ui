// The domain vocabulary: the small components every page speaks in.
//
// One module per concept; this barrel is the public surface, so a page imports
// `@/components/domain` without knowing which file a badge lives in.

export { CLUSTER_ICON, RESOURCE_KINDS, byResourceKind } from "./resource-kinds"
export { Section, Mono, Empty, Spinner } from "./primitives"
export { StatusBadge, RegistryStatusBadge } from "./status-badges"
export { ClusterChip, clusterTone } from "./cluster-chip"
export { SnapshotAge } from "./snapshot-age"
export { UnknownCodeChip, ErrorChips } from "./error-chips"
export { UnsupportedApiPanel } from "./unsupported-api-panel"
export { LagCell } from "./lag-cell"
export { placementCell, PlacementLegend } from "./placement"
export { ClusterCounts, RegistryCounts } from "./counts"
export { Stat } from "./stat"
export { FeatureBadge, featureState } from "./feature-badge"
export { HintHead } from "./hint-head"
export { SortableHead } from "./sortable-head"
