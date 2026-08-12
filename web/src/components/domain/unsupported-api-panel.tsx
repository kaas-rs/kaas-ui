import { Card } from "@/components/ui/card"
import { Mono } from "./primitives"

/**
 * The degradation component.
 *
 * Shows the api name and *both* version ranges, laid out as a comparison
 * rather than prose, because the pair is the diagnosis: no broker range means
 * the cluster does not implement it, no range of ours means this build has no
 * schema for it, and two disjoint ranges mean the cluster is behind.
 */
export function UnsupportedApiPanel({
  api,
  apiKey,
  broker,
  ours,
  what,
}: {
  api: string
  apiKey: number
  broker: [number, number] | null
  ours: [number, number] | null
  what?: string
}) {
  const range = (value: [number, number] | null) =>
    value ? `v${value[0]} – v${value[1]}` : null

  return (
    <Card className="max-w-2xl gap-0 p-5">
      <div className="mb-3 flex items-baseline justify-between gap-4 border-b pb-2">
        <h3 className="text-[15px] font-semibold">{api}</h3>
        <Mono>api key {apiKey}</Mono>
      </div>
      <dl className="grid grid-cols-[10rem_1fr] gap-y-2 text-[13px]">
        <dt className="text-ink-muted">this cluster</dt>
        <dd className="font-mono">
          {range(broker) ?? (
            <span className="text-danger">does not implement it</span>
          )}
        </dd>
        <dt className="text-ink-muted">kaas-ui speaks</dt>
        <dd className="font-mono">
          {range(ours) ?? (
            <span className="text-warn-ink">no schema in this build</span>
          )}
        </dd>
      </dl>
      <p className="mt-4 text-[13px] text-ink-muted">
        {broker === null
          ? `This cluster does not answer ${api}, so ${what ?? "this view"} has nothing behind it. The same URL against a cluster that does will render normally.`
          : ours === null
            ? `This build of kaas-ui has no schema for ${api}. The cluster is ahead of the codec; upgrading kaas-ui is what fixes it.`
            : `The versions do not overlap: the cluster speaks ${range(broker)} and kaas-ui speaks ${range(ours)}.`}
      </p>
    </Card>
  )
}
