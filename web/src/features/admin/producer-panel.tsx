import { useProducers } from "@/api/client"
import { ErrorChips, HintHead, Spinner } from "@/components/domain"
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

/**
 * The producers writing to one topic, fetched when a transaction is opened.
 *
 * On demand rather than with the list: this is a `DescribeProducers` to every
 * leader holding a partition of the topic, and doing that for every row of a
 * transaction table would be a fan-out per row of a screen somebody is
 * scanning. It is the second question — "which producer" — and it is asked
 * after the first has an answer worth following.
 *
 * The row that matters is the one with a transaction start offset: that
 * producer has an open transaction on that partition, which is what holds the
 * last stable offset there and stalls every `read_committed` consumer behind
 * it.
 */
export function ProducerPanel({
  envId,
  clusterId,
  topic,
}: {
  envId: string
  clusterId: string
  topic: string
}) {
  const producers = useProducers(envId, clusterId, topic)
  const rows = producers.data?.items ?? []

  return (
    <div className="rounded-md border bg-surface-sunken p-3">
      <p className="mb-2 text-[12px] text-ink-muted">
        producers on <span className="font-mono">{topic}</span>
      </p>
      <ErrorChips errors={producers.data?.errors ?? []} />
      {producers.isLoading ? (
        <Spinner label={`asking the leaders of ${topic}`} />
      ) : rows.length === 0 ? (
        <p className="text-[12px] text-ink-faint">
          no producer state on any partition
        </p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <HintHead
                label="partition"
                hint="its index within the topic"
                right
              />
              <HintHead
                label="producer"
                hint="the producer id the broker has state for on this partition"
                right
              />
              <HintHead
                label="epoch"
                hint="the producer epoch. Not a leader epoch: this bumps when a producer is fenced, which is how a zombie is told from its replacement"
                right
              />
              <HintHead
                label="last seq"
                hint="the last sequence number the leader accepted from it — the idempotence counter, not an offset"
                right
              />
              <HintHead
                label="txn start"
                hint="where this producer's open transaction begins. A value here is the producer holding this partition's last stable offset back"
                right
              />
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((producer) => (
              <TableRow key={`${producer.partition}-${producer.producerId}`}>
                <TableCell className="text-right font-mono">
                  {producer.partition}
                </TableCell>
                <TableCell className="text-right font-mono">
                  {producer.producerId}
                </TableCell>
                <TableCell className="text-right font-mono text-ink-muted">
                  {producer.producerEpoch}
                </TableCell>
                <TableCell className="text-right font-mono text-ink-muted">
                  {producer.lastSequence}
                </TableCell>
                <TableCell className="text-right font-mono">
                  {producer.currentTxnStartOffset === null ? (
                    <span className="text-ink-faint">—</span>
                  ) : (
                    <span className="font-medium text-warn-ink">
                      {producer.currentTxnStartOffset}
                    </span>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  )
}
