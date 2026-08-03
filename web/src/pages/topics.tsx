import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { useCallback, useState } from "react";
import { ArrowLeft } from "lucide-react";

import { useClusters, useTopic, useTopicConfigs, useTopics } from "@/api/client";
import type { Partition } from "@/api/types";
import { MessageBrowser } from "@/features/messages/browser";
import type { TopicSearch, TopicTab } from "@/features/messages/search";
import {
  Empty,
  ErrorChips,
  Mono,
  PlacementLegend,
  placementCell,
  SnapshotAge,
  Spinner,
  bytes,
  count,
} from "@/components/domain";
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { PageTitle } from "@/shell";
import { ConfigTable } from "./cluster";

const PAGE = 50;

export function Topics({ clusterId }: { clusterId: string }) {
  const [search, setSearch] = useState("");
  const [internal, setInternal] = useState(false);
  const [sizes, setSizes] = useState(false);
  const [sort, setSort] = useState("name");
  const [order, setOrder] = useState<"asc" | "desc">("asc");
  const [offset, setOffset] = useState(0);

  const topics = useTopics(clusterId, {
    search,
    internal,
    sizes,
    sort,
    order,
    limit: PAGE,
    offset,
  });

  const total = topics.data?.total ?? 0;
  const items = topics.data?.items ?? [];
  const showIds = items.some((topic) => topic.topicId !== null);

  const sortBy = (column: string) => {
    if (sort === column) {
      setOrder(order === "asc" ? "desc" : "asc");
    } else {
      setSort(column);
      setOrder("asc");
    }
    setOffset(0);
  };

  const heading = (label: string, column: string, right?: boolean) => (
    <TableHead className={right ? "text-right" : undefined}>
      <button type="button" onClick={() => sortBy(column)} className="hover:underline">
        {label}
        {sort === column ? (order === "asc" ? " ↑" : " ↓") : ""}
      </button>
    </TableHead>
  );

  return (
    <>
      <PageTitle
        title="Topics"
        subtitle={`${count(total)} matching`}
        actions={<SnapshotAge ageMs={topics.data?.snapshotAgeMs ?? null} />}
      />

      <div className="mb-4 flex flex-wrap items-center gap-4">
        <Input
          value={search}
          onChange={(event) => {
            setSearch(event.target.value);
            setOffset(0);
          }}
          placeholder="filter by name"
          className="h-8 max-w-xs"
        />
        <Label className="text-[12px] font-normal text-ink-muted">
          <input
            type="checkbox"
            checked={internal}
            onChange={(event) => {
              setInternal(event.target.checked);
              setOffset(0);
            }}
          />
          internal topics
        </Label>
        <Label className="text-[12px] font-normal text-ink-muted">
          <input
            type="checkbox"
            checked={sizes}
            onChange={(event) => setSizes(event.target.checked)}
          />
          sizes
          <span
            className="text-ink-faint"
            title="a DescribeLogDirs fan-out across every broker"
          >
            (extra call)
          </span>
        </Label>
      </div>

      <ErrorChips errors={topics.data?.errors ?? []} />

      {topics.isLoading ? (
        <Spinner />
      ) : items.length === 0 ? (
        <Empty>no topics match</Empty>
      ) : (
        <>
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  {heading("name", "name")}
                  {showIds ? <TableHead>id</TableHead> : null}
                  {heading("partitions", "partitions", true)}
                  <TableHead className="text-right">rf</TableHead>
                  {heading("under-replicated", "underReplicated", true)}
                  <TableHead className="text-right">offline</TableHead>
                  {sizes ? heading("size", "size", true) : null}
                </TableRow>
              </TableHeader>
              <TableBody>
                {items.map((topic) => (
                  <TableRow key={topic.name}>
                    <TableCell>
                      <Link
                        to="/clusters/$clusterId/topics/$topic"
                        params={{ clusterId, topic: topic.name }}
                        className="font-mono hover:underline"
                        style={{ color: "var(--rust-ink)" }}
                      >
                        {topic.name}
                      </Link>
                      {topic.internal ? (
                        <span className="ml-2 text-[11px] text-ink-faint">internal</span>
                      ) : null}
                    </TableCell>
                    {showIds ? (
                      <TableCell className="font-mono text-[11px] text-ink-faint">
                        {topic.topicId ?? "—"}
                      </TableCell>
                    ) : null}
                    <TableCell className="text-right font-mono">
                      {topic.partitionCount}
                    </TableCell>
                    <TableCell className="text-right font-mono">
                      {topic.replicationFactor}
                    </TableCell>
                    <TableCell className="text-right">
                      {topic.underReplicatedPartitionCount > 0 ? (
                        <span className="font-mono font-medium text-warn-ink">
                          △ {topic.underReplicatedPartitionCount}
                        </span>
                      ) : (
                        <span className="text-ink-faint">0</span>
                      )}
                    </TableCell>
                    <TableCell className="text-right">
                      {topic.offlinePartitionCount > 0 ? (
                        <span className="font-mono font-medium text-danger">
                          ✕ {topic.offlinePartitionCount}
                        </span>
                      ) : (
                        <span className="text-ink-faint">0</span>
                      )}
                    </TableCell>
                    {sizes ? (
                      <TableCell className="text-right font-mono">
                        {bytes(topic.replicatedBytes)}
                      </TableCell>
                    ) : null}
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>

          {total > PAGE ? (
            <div className="mt-3 flex items-center gap-3 text-[12px]">
              <Button
                variant="outline"
                size="sm"
                disabled={offset === 0}
                onClick={() => setOffset(Math.max(0, offset - PAGE))}
              >
                previous
              </Button>
              <span className="text-ink-muted">
                {offset + 1}–{Math.min(offset + PAGE, total)} of {count(total)}
              </span>
              <Button
                variant="outline"
                size="sm"
                disabled={offset + PAGE >= total}
                onClick={() => setOffset(offset + PAGE)}
              >
                next
              </Button>
            </div>
          ) : null}
        </>
      )}
    </>
  );
}

export function TopicDetail({
  clusterId,
  topic,
}: {
  clusterId: string;
  topic: string;
}) {
  const detail = useTopic(clusterId, topic);
  const search = useSearch({ from: "/clusters/$clusterId/topics/$topic" });
  // What this caller may do here, from the cluster's own card. A messages tab
  // that 403s on click is worse than no messages tab — the same reasoning the
  // sidebar applies to a capability the *broker* does not have. Until the
  // answer arrives, show it: a tab that appears under the cursor is worse than
  // one that errors once, and an open deployment always grants both.
  const clusters = useClusters();
  const grants = clusters.data?.items.find((card) => card.id === clusterId)?.grants;
  const mayReadMessages = grants === undefined || !!grants.topic?.includes("messages_read");
  const navigate = useNavigate();

  /**
   * Every write to this page's URL, including the message browser's.
   *
   * Replaces by default: seeking, filtering and selecting are one continuous
   * act of looking at a topic, and a back button that walks a reader out
   * through forty row selections is not a back button. Changing tab is the
   * exception — that is a place someone can want to come back to.
   */
  const setSearch = useCallback(
    (next: Partial<TopicSearch>, replace = true) => {
      void navigate({
        to: "/clusters/$clusterId/topics/$topic",
        params: { clusterId, topic },
        search: (previous) => ({ ...previous, ...next }),
        replace,
      });
    },
    [navigate, clusterId, topic],
  );

  if (detail.isLoading) return <Spinner label={`describing ${topic}`} />;

  const info = detail.data?.items[0];
  const errors = detail.data?.errors ?? [];

  if (!info) {
    return (
      <>
        <PageTitle title={topic} />
        <ErrorChips errors={errors} />
        <Card className="p-5 text-[13px] text-ink-muted">
          {errors[0]?.message ?? "the cluster did not describe this topic"}
        </Card>
      </>
    );
  }

  return (
    <>
      <PageTitle
        title={<span className="font-mono">{info.name}</span>}
        subtitle={
          <span className="flex flex-wrap items-center gap-3">
            <span>{info.partitions.length} partitions</span>
            {info.internal ? <span className="text-warn-ink">internal</span> : null}
            {info.topicId ? <Mono>{info.topicId}</Mono> : null}
          </span>
        }
        actions={
          <Button variant="ghost" size="sm" asChild>
            <Link to="/clusters/$clusterId/topics" params={{ clusterId }}>
              <ArrowLeft aria-hidden />
              all topics
            </Link>
          </Button>
        }
      />

      <ErrorChips errors={errors} />

      {/* Controlled by the URL, not by local state: `?tab=messages` alongside
          the seek parameters is what makes a link to a filtered, seeked view
          open on that view rather than on the partition table. */}
      <Tabs
        value={search.tab}
        onValueChange={(tab) => setSearch({ tab: tab as TopicTab }, false)}
      >
        <TabsList>
          <TabsTrigger value="partitions">partitions</TabsTrigger>
          <TabsTrigger value="configs">configs</TabsTrigger>
          {mayReadMessages ? (
            <TabsTrigger value="messages">messages</TabsTrigger>
          ) : null}
        </TabsList>

        <TabsContent value="partitions" className="mt-4">
          <Partitions partitions={info.partitions} brokerIds={info.brokerIds} />
        </TabsContent>
        <TabsContent value="configs" className="mt-4">
          <TopicConfigs clusterId={clusterId} topic={topic} />
        </TabsContent>
        {/* The panel is given a height rather than left to grow: the list is
            virtualized and the split pane is a flex box, and neither can work
            inside a page that scrolls. The subtraction is this page's chrome —
            app header, padding, title, tab row, footer. Leaving the tab stops
            the stream, because Radix unmounts the panel that is not shown and
            a live scan nobody is looking at is a scan that should not be open. */}
        <TabsContent value="messages" className="mt-4">
          <MessageBrowser
            clusterId={clusterId}
            topic={topic}
            search={search}
            onSearch={setSearch}
            className="h-[calc(100vh-17rem)] min-h-[26rem]"
          />
        </TabsContent>
      </Tabs>
    </>
  );
}

/**
 * A partition-table header that says what its column means.
 *
 * Every column here is a Kafka term with a precise meaning and a plausible
 * wrong reading — `records` is what is *retained*, not what was ever written,
 * and `epoch` counts leadership changes rather than anything about data. One
 * line each, on hover, is cheaper than a legend nobody scrolls to.
 */
function Head({
  label,
  hint,
  right,
  className,
}: {
  label: ReactNode;
  hint: string;
  right?: boolean;
  className?: string;
}) {
  return (
    <TableHead className={cn(right && "text-right", className)}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="cursor-help decoration-dotted underline-offset-4 hover:underline">
            {label}
          </span>
        </TooltipTrigger>
        <TooltipContent>{hint}</TooltipContent>
      </Tooltip>
    </TableHead>
  );
}

/**
 * Partitions, with the replica placement in the same rows.
 *
 * These were two tabs. They are one table because they were always one
 * question asked twice: the grid said *where* a partition lives and the table
 * said *what state* it is in, and answering "which broker is the out-of-sync
 * replica on, and how far behind is that partition" meant holding one view in
 * your head while looking at the other.
 *
 * The broker columns come first, right after the partition number, because
 * that block is the shape you scan — a column of `L` glyphs drifting to one
 * broker is a leader imbalance, and a gap in it is visible before any number
 * is read.
 */
function Partitions({
  partitions,
  brokerIds,
}: {
  partitions: Partition[];
  brokerIds: number[];
}) {
  return (
    <div className="space-y-3">
      <div className="overflow-x-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <Head label="partition" hint="its index within the topic" right />
              {/* One column per broker. Narrow and centred so the block reads
                  as a grid rather than as eight more columns of data. */}
              {brokerIds.map((broker, index) => (
                <Head
                  key={broker}
                  label={broker}
                  hint={`broker ${broker} — what it holds of each partition`}
                  className={cn(
                    "px-1 text-center font-mono font-normal",
                    index === 0 && "border-line border-l",
                    index === brokerIds.length - 1 && "border-line border-r",
                  )}
                />
              ))}
              <Head
                label="epoch"
                hint="leader epoch — it bumps on every leadership change"
                right
              />
              <Head label="earliest" hint="the oldest offset still retained" right />
              <Head label="latest" hint="the offset the next record will get" right />
              <Head
                label="records"
                hint="latest − earliest: what is retained, not what was ever written"
                right
              />
            </TableRow>
          </TableHeader>
          <TableBody>
            {partitions.map((partition) => {
              const records =
                partition.earliestOffset !== null && partition.latestOffset !== null
                  ? partition.latestOffset - partition.earliestOffset
                  : null;
              return (
                <TableRow key={partition.partition}>
                  <TableCell className="text-right font-mono whitespace-nowrap">
                    {partition.partition}
                    {/* Every other state has a glyph. "No leader at all" has
                        the *absence* of one, which reads as nothing wrong
                        unless it is said. */}
                    {partition.leader === null ? (
                      <span className="text-danger ml-1.5" title="no leader">
                        ✕
                      </span>
                    ) : null}
                  </TableCell>
                  {brokerIds.map((broker, index) => {
                    const { label, style, title, preferred } = placementCell(
                      partition,
                      broker,
                    );
                    return (
                      <TableCell
                        key={broker}
                        className={cn(
                          "px-1 py-0.5",
                          index === 0 && "border-line border-l",
                          index === brokerIds.length - 1 && "border-line border-r",
                        )}
                      >
                        <div
                          title={`p${partition.partition} on broker ${broker}: ${title}`}
                          style={style}
                          className={cn(
                            "mx-auto grid h-5 w-6 place-items-center rounded-[2px] font-mono text-[12px]",
                            preferred && "outline-ink-muted outline-2 -outline-offset-1",
                          )}
                        >
                          {label}
                        </div>
                      </TableCell>
                    );
                  })}
                  <TableCell className="text-ink-faint text-right font-mono">
                    {partition.leaderEpoch}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {count(partition.earliestOffset)}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {count(partition.latestOffset)}
                  </TableCell>
                  <TableCell className="text-right font-mono">{count(records)}</TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </div>
      {brokerIds.length > 0 ? <PlacementLegend /> : null}
    </div>
  );
}


function TopicConfigs({ clusterId, topic }: { clusterId: string; topic: string }) {
  const configs = useTopicConfigs(clusterId, topic);
  const [onlyExplicit, setOnlyExplicit] = useState(true);

  const entries = configs.data?.items[0]?.entries ?? [];
  const shown = onlyExplicit ? entries.filter((entry) => entry.isExplicit) : entries;

  return (
    <>
      <Label className="mb-3 text-[12px] font-normal text-ink-muted">
        <input
          type="checkbox"
          checked={onlyExplicit}
          onChange={(event) => setOnlyExplicit(event.target.checked)}
        />
        only values someone set on this topic
      </Label>
      <ErrorChips errors={configs.data?.errors ?? []} />
      {configs.isLoading ? (
        <Spinner />
      ) : shown.length === 0 ? (
        <Empty>this topic has no overrides — everything is inherited</Empty>
      ) : (
        <ConfigTable entries={shown} total={entries.length} />
      )}
    </>
  );
}
