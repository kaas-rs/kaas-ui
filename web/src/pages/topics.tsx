import { Link } from "@tanstack/react-router";
import { Fragment, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  ArrowDown,
  ArrowLeft,
  ChevronDown,
  ChevronRight,
  Clock,
  FileText,
  RefreshCw,
  Search,
} from "lucide-react";

import { useTail, useTopic, useTopicConfigs, useTopics } from "@/api/client";
import type { Partition, Payload as PayloadData } from "@/api/types";
import {
  Empty,
  ErrorChips,
  Mono,
  PartitionGrid,
  Section,
  SnapshotAge,
  Spinner,
  bytes,
  count,
} from "@/components/domain";
import { Badge } from "@/components/ui/badge";
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

      <Tabs defaultValue="partitions">
        <TabsList>
          <TabsTrigger value="partitions">partitions</TabsTrigger>
          <TabsTrigger value="placement">placement</TabsTrigger>
          <TabsTrigger value="configs">configs</TabsTrigger>
          <TabsTrigger value="messages">messages</TabsTrigger>
        </TabsList>

        <TabsContent value="partitions" className="mt-4">
          <Partitions partitions={info.partitions} />
        </TabsContent>
        <TabsContent value="placement" className="mt-4">
          <Section title="Replica placement">
            <PartitionGrid partitions={info.partitions} brokerIds={info.brokerIds} />
          </Section>
        </TabsContent>
        <TabsContent value="configs" className="mt-4">
          <TopicConfigs clusterId={clusterId} topic={topic} />
        </TabsContent>
        <TabsContent value="messages" className="mt-4">
          <Messages clusterId={clusterId} topic={topic} />
        </TabsContent>
      </Tabs>
    </>
  );
}

function Partitions({ partitions }: { partitions: Partition[] }) {
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="text-right">partition</TableHead>
            <TableHead className="text-right">leader</TableHead>
            <TableHead className="text-right">epoch</TableHead>
            <TableHead>replicas</TableHead>
            <TableHead>isr</TableHead>
            <TableHead className="text-right">earliest</TableHead>
            <TableHead className="text-right">latest</TableHead>
            <TableHead className="text-right">records</TableHead>
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
                <TableCell className="text-right font-mono">
                  {partition.partition}
                </TableCell>
                <TableCell className="text-right">
                  {partition.leader === null ? (
                    <span className="text-danger" title="no leader">
                      ✕ none
                    </span>
                  ) : (
                    <span className="font-mono">{partition.leader}</span>
                  )}
                </TableCell>
                <TableCell className="text-right font-mono text-ink-faint">
                  {partition.leaderEpoch}
                </TableCell>
                <TableCell className="font-mono text-ink-muted">
                  {partition.replicas.join(", ")}
                </TableCell>
                <TableCell>
                  <span
                    className={
                      partition.underReplicated
                        ? "font-mono font-medium text-warn-ink"
                        : "font-mono text-ink-muted"
                    }
                  >
                    {partition.isr.join(", ")}
                    {partition.underReplicated
                      ? ` △ ${partition.replicas.length - partition.isr.length} short`
                      : ""}
                  </span>
                  {partition.offlineReplicas.length > 0 ? (
                    <span className="ml-2 font-mono text-danger">
                      ✕ offline {partition.offlineReplicas.join(", ")}
                    </span>
                  ) : null}
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

/**
 * The message browser, in the shape a Kafka UI reader expects: a table of
 * offset / partition / timestamp / key / value with the value clipped to one
 * line, and a row that expands to the whole record.
 *
 * A stacked-card list reads fine for five records and terribly for five
 * hundred — and five hundred is the normal ask.
 */
function Messages({ clusterId, topic }: { clusterId: string; topic: string }) {
  const [limit, setLimit] = useState(100);
  const [partitions, setPartitions] = useState("");
  const [live, setLive] = useState(false);
  const [filter, setFilter] = useState("");
  const [expanded, setExpanded] = useState<string | null>(null);
  const [elapsed, setElapsed] = useState<number | null>(null);
  const started = useRef<number | null>(null);

  const tail = useTail(clusterId, topic, limit, partitions, live);

  // Round-trip time, measured where it is actually true: around the request.
  useEffect(() => {
    if (tail.isFetching) {
      started.current = performance.now();
      return;
    }
    if (started.current !== null) {
      setElapsed(performance.now() - started.current);
      started.current = null;
    }
  }, [tail.isFetching]);

  const rows = tail.data?.items ?? [];
  const shown = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    if (!needle) return rows;
    // Filtering what is already on screen. The server-side filter is
    // `RecordFilter`, which runs before deserialization and arrives with the
    // scan endpoint; this is the cheap one over what was already fetched.
    return rows.filter(
      (row) =>
        row.key?.text.toLowerCase().includes(needle) ||
        row.value?.text.toLowerCase().includes(needle) ||
        String(row.offset).includes(needle),
    );
  }, [rows, filter]);

  const payload = rows.reduce((total, row) => total + row.sizeBytes, 0);

  const run = () => {
    setExpanded(null);
    if (live) void tail.refetch();
    else setLive(true);
  };

  return (
    <>
      <div className="mb-3 flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-wrap items-end gap-2">
          <div>
            <Label className="mb-1 text-[11px] text-ink-muted">records</Label>
            <Input
              type="number"
              min={1}
              max={5000}
              value={limit}
              onChange={(event) => setLimit(Number(event.target.value) || 1)}
              className="h-8 w-24 font-mono"
            />
          </div>
          <div>
            <Label className="mb-1 text-[11px] text-ink-muted">partitions</Label>
            <Input
              value={partitions}
              onChange={(event) => setPartitions(event.target.value)}
              placeholder="all"
              className="h-8 w-32 font-mono"
            />
          </div>
          <Button size="sm" onClick={run} disabled={tail.isFetching}>
            <RefreshCw aria-hidden className={tail.isFetching ? "animate-spin" : ""} />
            {tail.isFetching ? "reading" : live ? "refresh" : "read the tail"}
          </Button>
          <Badge variant="outline" className="h-8 gap-1.5 px-2 text-ink-muted">
            <ArrowDown aria-hidden className="size-3" />
            newest first
          </Badge>
        </div>

        <div className="flex items-end gap-3">
          <div className="relative">
            <Search
              aria-hidden
              className="pointer-events-none absolute top-2 left-2 size-4 text-ink-faint"
            />
            <Input
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              placeholder="search loaded records"
              className="h-8 w-56 pl-8"
            />
          </div>
        </div>
      </div>

      {live && !tail.isLoading ? (
        <div className="mb-3 flex flex-wrap items-center gap-2 text-[12px]">
          <ConsumeStat icon={Clock} label="round trip">
            {elapsed === null ? "—" : `${Math.round(elapsed)} ms`}
          </ConsumeStat>
          <ConsumeStat icon={ArrowDown} label="payload">
            {bytes(payload)}
          </ConsumeStat>
          <ConsumeStat icon={FileText} label="records">
            {count(rows.length)} of {count(tail.data?.total ?? 0)} fetched
          </ConsumeStat>
          <span className="text-ink-faint">
            kaas-lib spreads the limit across partitions with <Mono>div_ceil</Mono>, so it
            fetches a few more than asked and this view truncates after merging.
          </span>
        </div>
      ) : null}

      <ErrorChips errors={tail.data?.errors ?? []} />

      {!live ? (
        <Empty>nothing read yet</Empty>
      ) : tail.isLoading ? (
        <Spinner label="walking back from the log end" />
      ) : tail.error ? (
        <Card className="p-4 text-[13px] text-danger">{String(tail.error)}</Card>
      ) : shown.length === 0 ? (
        <Empty>{rows.length === 0 ? "this topic is empty" : "nothing matches"}</Empty>
      ) : (
        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-8" />
                <TableHead className="text-right">offset</TableHead>
                <TableHead className="text-right">partition</TableHead>
                <TableHead>timestamp</TableHead>
                <TableHead>key</TableHead>
                <TableHead>value</TableHead>
                <TableHead className="text-right">size</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {shown.map((message) => {
                const id = `${message.partition}-${message.offset}`;
                const open = expanded === id;
                return (
                  <Fragment key={id}>
                    <TableRow
                      className="cursor-pointer"
                      onClick={() => setExpanded(open ? null : id)}
                    >
                      <TableCell className="text-ink-faint">
                        {open ? (
                          <ChevronDown aria-hidden className="size-4" />
                        ) : (
                          <ChevronRight aria-hidden className="size-4" />
                        )}
                      </TableCell>
                      <TableCell className="text-right font-mono">
                        {count(message.offset)}
                      </TableCell>
                      <TableCell className="text-right font-mono">
                        {message.partition}
                      </TableCell>
                      <TableCell className="text-ink-muted">
                        {new Date(message.timestamp).toLocaleString()}
                      </TableCell>
                      <TableCell className="max-w-[12rem] truncate font-mono text-ink-muted">
                        {message.key ? message.key.text : <Tombstone>no key</Tombstone>}
                      </TableCell>
                      <TableCell className="max-w-[32rem] truncate font-mono">
                        {message.value ? (
                          message.value.text
                        ) : (
                          <Tombstone>tombstone</Tombstone>
                        )}
                      </TableCell>
                      <TableCell className="text-right text-ink-faint">
                        {bytes(message.sizeBytes)}
                      </TableCell>
                    </TableRow>

                    {open ? (
                      <TableRow className="hover:bg-transparent">
                        <TableCell colSpan={7} className="bg-surface-sunken p-4">
                          <div className="flex flex-wrap items-center gap-3 text-[12px] text-ink-muted">
                            <span>{new Date(message.timestamp).toISOString()}</span>
                            <span className="text-ink-faint">
                              {message.timestampType}
                            </span>
                            {message.transactional ? (
                              <Badge variant="outline" className="text-rust-ink">
                                transactional
                              </Badge>
                            ) : null}
                          </div>
                          <Payload label="key" payload={message.key} absent="no key" />
                          <Payload
                            label="value"
                            payload={message.value}
                            absent="tombstone — a null value, not an empty one"
                          />
                          {message.headers.length > 0 ? (
                            <div className="mt-3 border-t pt-2">
                              <span className="text-[11px] uppercase tracking-wide text-ink-faint">
                                headers
                              </span>
                              {message.headers.map((header, index) => (
                                <div
                                  key={`${header.name}-${index}`}
                                  className="flex gap-2 text-[13px]"
                                >
                                  <span className="font-mono text-ink-muted">
                                    {header.name}
                                  </span>
                                  <span className="font-mono break-all">
                                    {header.value?.text ?? "—"}
                                  </span>
                                </div>
                              ))}
                            </div>
                          ) : null}
                        </TableCell>
                      </TableRow>
                    ) : null}
                  </Fragment>
                );
              })}
            </TableBody>
          </Table>
        </div>
      )}
    </>
  );
}

function ConsumeStat({
  icon: Icon,
  label,
  children,
}: {
  icon: typeof Clock;
  label: string;
  children: ReactNode;
}) {
  return (
    <span
      className="inline-flex items-center gap-1.5 rounded-sm px-2 py-1"
      style={{ background: "var(--surface-sunken)" }}
      title={label}
    >
      <Icon aria-hidden className="size-3 text-ink-faint" />
      <span className="text-ink-muted">{children}</span>
    </span>
  );
}

/** A null key or value is not an empty one, and must not look like one. */
function Tombstone({ children }: { children: ReactNode }) {
  return <span className="text-[12px] italic text-ink-faint">{children}</span>;
}

function Payload({
  label,
  payload,
  absent,
}: {
  label: string;
  payload: PayloadData | null;
  absent: string;
}) {
  return (
    <div className="mt-2">
      <div className="mb-1 flex items-center gap-2">
        <span className="text-[11px] uppercase tracking-wide text-ink-faint">
          {label}
        </span>
        {payload?.encoding === "hex" ? (
          <Badge
            variant="secondary"
            className="text-ink-muted"
            title="not valid UTF-8, so it is rendered as hex"
          >
            hex
          </Badge>
        ) : null}
        {payload ? (
          <span className="text-[11px] text-ink-faint">{bytes(payload.bytes)}</span>
        ) : null}
      </div>
      {payload === null ? (
        <Tombstone>{absent}</Tombstone>
      ) : (
        <pre className="max-h-64 overflow-auto rounded-sm border bg-card p-2 font-mono text-[12px] whitespace-pre-wrap break-all">
          {payload.text}
          {payload.truncated ? (
            <span className="text-ink-faint"> … truncated</span>
          ) : null}
        </pre>
      )}
    </div>
  );
}
