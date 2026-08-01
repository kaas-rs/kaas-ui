import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { useTail, useTopic, useTopicConfigs, useTopics } from "../api/client";
import {
  Card,
  ErrorChips,
  Empty,
  Mono,
  PartitionGrid,
  Section,
  SnapshotAge,
  Spinner,
  Table,
  Td,
  Th,
  bytes,
  count,
} from "../components";
import { PageTitle } from "../shell";
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

  const heading = (label: string, column: string, align?: "right") => (
    <Th align={align}>
      <button type="button" onClick={() => sortBy(column)} className="hover:underline">
        {label}
        {sort === column ? (order === "asc" ? " ↑" : " ↓") : ""}
      </button>
    </Th>
  );

  return (
    <>
      <PageTitle
        title="Topics"
        subtitle={`${count(total)} matching`}
        actions={
          <SnapshotAge ageMs={topics.data?.snapshotAgeMs ?? null} />
        }
      />

      <div className="flex flex-wrap items-center gap-3 mb-4">
        <input
          value={search}
          onChange={(event) => {
            setSearch(event.target.value);
            setOffset(0);
          }}
          placeholder="filter by name"
          className="px-3 py-1.5 text-[13px] bg-surface-raised border border-line rounded-sm min-w-[16rem]"
        />
        <label className="text-[12px] flex items-center gap-2 text-ink-muted">
          <input
            type="checkbox"
            checked={internal}
            onChange={(event) => {
              setInternal(event.target.checked);
              setOffset(0);
            }}
          />
          internal topics
        </label>
        <label className="text-[12px] flex items-center gap-2 text-ink-muted">
          <input
            type="checkbox"
            checked={sizes}
            onChange={(event) => setSizes(event.target.checked)}
          />
          sizes
          <span className="text-ink-faint" title="a DescribeLogDirs fan-out across every broker">
            (extra call)
          </span>
        </label>
      </div>

      <ErrorChips errors={topics.data?.errors ?? []} />

      {topics.isLoading ? (
        <Spinner />
      ) : items.length === 0 ? (
        <Empty>no topics match</Empty>
      ) : (
        <>
          <Table>
            <thead>
              <tr>
                {heading("name", "name")}
                {showIds ? <Th>id</Th> : null}
                {heading("partitions", "partitions", "right")}
                <Th align="right">rf</Th>
                {heading("under-replicated", "underReplicated", "right")}
                <Th align="right">offline</Th>
                {sizes ? heading("size", "size", "right") : null}
              </tr>
            </thead>
            <tbody>
              {items.map((topic) => (
                <tr key={topic.name} className="hover:bg-surface-sunken">
                  <Td>
                    <Link
                      to="/clusters/$clusterId/topics/$topic"
                      params={{ clusterId, topic: topic.name }}
                      className="font-mono hover:underline"
                      style={{ color: "var(--color-accent-ink)" }}
                    >
                      {topic.name}
                    </Link>
                    {topic.internal ? (
                      <span className="ml-2 text-[11px] text-ink-faint">internal</span>
                    ) : null}
                  </Td>
                  {showIds ? (
                    <Td>
                      <span className="font-mono text-[11px] text-ink-faint">
                        {topic.topicId ?? "—"}
                      </span>
                    </Td>
                  ) : null}
                  <Td align="right">
                    <span className="font-mono">{topic.partitionCount}</span>
                  </Td>
                  <Td align="right">
                    <span className="font-mono">{topic.replicationFactor}</span>
                  </Td>
                  <Td align="right">
                    {topic.underReplicatedPartitionCount > 0 ? (
                      <span className="font-mono text-warn-ink font-medium">
                        △ {topic.underReplicatedPartitionCount}
                      </span>
                    ) : (
                      <span className="text-ink-faint">0</span>
                    )}
                  </Td>
                  <Td align="right">
                    {topic.offlinePartitionCount > 0 ? (
                      <span className="font-mono text-danger font-medium">
                        ✕ {topic.offlinePartitionCount}
                      </span>
                    ) : (
                      <span className="text-ink-faint">0</span>
                    )}
                  </Td>
                  {sizes ? (
                    <Td align="right">
                      <span className="font-mono">{bytes(topic.replicatedBytes)}</span>
                    </Td>
                  ) : null}
                </tr>
              ))}
            </tbody>
          </Table>

          {total > PAGE ? (
            <div className="flex items-center gap-3 mt-3 text-[12px]">
              <button
                type="button"
                disabled={offset === 0}
                onClick={() => setOffset(Math.max(0, offset - PAGE))}
                className="px-2 py-1 border border-line-strong rounded-sm disabled:opacity-40"
              >
                previous
              </button>
              <span className="text-ink-muted">
                {offset + 1}–{Math.min(offset + PAGE, total)} of {count(total)}
              </span>
              <button
                type="button"
                disabled={offset + PAGE >= total}
                onClick={() => setOffset(offset + PAGE)}
                className="px-2 py-1 border border-line-strong rounded-sm disabled:opacity-40"
              >
                next
              </button>
            </div>
          ) : null}
        </>
      )}
    </>
  );
}

type Panel = "partitions" | "placement" | "configs" | "messages";

export function TopicDetail({
  clusterId,
  topic,
}: {
  clusterId: string;
  topic: string;
}) {
  const [panel, setPanel] = useState<Panel>("partitions");
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

  const panels: Panel[] = ["partitions", "placement", "configs", "messages"];

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
          <Link
            to="/clusters/$clusterId/topics"
            params={{ clusterId }}
            className="text-[13px] hover:underline"
            style={{ color: "var(--color-link)" }}
          >
            ← all topics
          </Link>
        }
      />

      <ErrorChips errors={errors} />

      <div className="flex gap-1 border-b border-line mb-5">
        {panels.map((name) => (
          <button
            key={name}
            type="button"
            onClick={() => setPanel(name)}
            className="px-3 py-2 text-[13px] text-ink-muted hover:text-ink"
            style={
              panel === name
                ? {
                    color: "var(--color-ink)",
                    boxShadow: "inset 0 -2px 0 var(--color-accent)",
                    fontWeight: 500,
                  }
                : undefined
            }
          >
            {name}
          </button>
        ))}
      </div>

      {panel === "partitions" ? <Partitions partitions={info.partitions} /> : null}
      {panel === "placement" ? (
        <Section title="Replica placement">
          <PartitionGrid partitions={info.partitions} brokerIds={info.brokerIds} />
        </Section>
      ) : null}
      {panel === "configs" ? <TopicConfigs clusterId={clusterId} topic={topic} /> : null}
      {panel === "messages" ? <Messages clusterId={clusterId} topic={topic} /> : null}
    </>
  );
}

function Partitions({
  partitions,
}: {
  partitions: {
    partition: number;
    leader: number | null;
    leaderEpoch: number;
    replicas: number[];
    isr: number[];
    offlineReplicas: number[];
    underReplicated: boolean;
    earliestOffset: number | null;
    latestOffset: number | null;
  }[];
}) {
  return (
    <Table>
      <thead>
        <tr>
          <Th align="right">partition</Th>
          <Th align="right">leader</Th>
          <Th align="right">epoch</Th>
          <Th>replicas</Th>
          <Th>isr</Th>
          <Th align="right">earliest</Th>
          <Th align="right">latest</Th>
          <Th align="right">records</Th>
        </tr>
      </thead>
      <tbody>
        {partitions.map((partition) => {
          const records =
            partition.earliestOffset !== null && partition.latestOffset !== null
              ? partition.latestOffset - partition.earliestOffset
              : null;
          return (
            <tr key={partition.partition} className="hover:bg-surface-sunken">
              <Td align="right">
                <span className="font-mono">{partition.partition}</span>
              </Td>
              <Td align="right">
                {partition.leader === null ? (
                  <span className="text-danger" title="no leader">
                    ✕ none
                  </span>
                ) : (
                  <span className="font-mono">{partition.leader}</span>
                )}
              </Td>
              <Td align="right">
                <span className="font-mono text-ink-faint">{partition.leaderEpoch}</span>
              </Td>
              <Td>
                <span className="font-mono text-ink-muted">
                  {partition.replicas.join(", ")}
                </span>
              </Td>
              <Td>
                <span
                  className={`font-mono ${
                    partition.underReplicated ? "text-warn-ink font-medium" : "text-ink-muted"
                  }`}
                >
                  {partition.isr.join(", ")}
                  {partition.underReplicated
                    ? ` △ ${partition.replicas.length - partition.isr.length} short`
                    : ""}
                </span>
                {partition.offlineReplicas.length > 0 ? (
                  <span className="text-danger font-mono ml-2">
                    ✕ offline {partition.offlineReplicas.join(", ")}
                  </span>
                ) : null}
              </Td>
              <Td align="right">
                <span className="font-mono">{count(partition.earliestOffset)}</span>
              </Td>
              <Td align="right">
                <span className="font-mono">{count(partition.latestOffset)}</span>
              </Td>
              <Td align="right">
                <span className="font-mono">{count(records)}</span>
              </Td>
            </tr>
          );
        })}
      </tbody>
    </Table>
  );
}

function TopicConfigs({ clusterId, topic }: { clusterId: string; topic: string }) {
  const configs = useTopicConfigs(clusterId, topic);
  const [onlyExplicit, setOnlyExplicit] = useState(true);

  const entries = configs.data?.items[0]?.entries ?? [];
  const shown = onlyExplicit ? entries.filter((entry) => entry.isExplicit) : entries;

  return (
    <>
      <label className="text-[12px] flex items-center gap-2 text-ink-muted mb-3">
        <input
          type="checkbox"
          checked={onlyExplicit}
          onChange={(event) => setOnlyExplicit(event.target.checked)}
        />
        only values someone set on this topic
      </label>
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

function Messages({ clusterId, topic }: { clusterId: string; topic: string }) {
  const [limit, setLimit] = useState(50);
  const [partitions, setPartitions] = useState("");
  const [live, setLive] = useState(false);
  const tail = useTail(clusterId, topic, limit, partitions, live);

  return (
    <>
      <div className="flex flex-wrap items-end gap-3 mb-4">
        <label className="text-[12px] text-ink-muted">
          records
          <input
            type="number"
            min={1}
            max={5000}
            value={limit}
            onChange={(event) => setLimit(Number(event.target.value) || 1)}
            className="block px-2 py-1 mt-1 w-24 text-[13px] font-mono bg-surface-raised border border-line rounded-sm"
          />
        </label>
        <label className="text-[12px] text-ink-muted">
          partitions
          <input
            value={partitions}
            onChange={(event) => setPartitions(event.target.value)}
            placeholder="all"
            className="block px-2 py-1 mt-1 w-40 text-[13px] font-mono bg-surface-raised border border-line rounded-sm"
          />
        </label>
        <button
          type="button"
          onClick={() => (live ? tail.refetch() : setLive(true))}
          className="px-3 py-1.5 text-[13px] rounded-sm border"
          style={{
            background: "var(--color-accent)",
            color: "#3B2E2A",
            borderColor: "var(--color-accent-edge)",
          }}
        >
          {tail.isFetching ? "reading…" : live ? "read again" : "read the tail"}
        </button>
        <p className="text-[12px] text-ink-faint max-w-md">
          The newest records first, merged across partitions. kaas-lib spreads the
          limit across partitions with <Mono>div_ceil</Mono>, so it fetches a few
          more than asked for and this view truncates after merging.
        </p>
      </div>

      <ErrorChips errors={tail.data?.errors ?? []} />

      {!live ? (
        <Empty>nothing read yet</Empty>
      ) : tail.isLoading ? (
        <Spinner label="walking back from the log end" />
      ) : tail.error ? (
        <Card className="p-4 text-[13px] text-danger">{String(tail.error)}</Card>
      ) : (tail.data?.items.length ?? 0) === 0 ? (
        <Empty>this topic is empty</Empty>
      ) : (
        <>
          <p className="text-[12px] text-ink-faint mb-2">
            {tail.data?.items.length} shown of {tail.data?.total} fetched
          </p>
          <div className="flex flex-col gap-2">
            {(tail.data?.items ?? []).map((message) => (
              <Card
                key={`${message.partition}-${message.offset}`}
                className="p-3 text-[13px]"
              >
                <div className="flex flex-wrap items-center gap-3 text-[12px] text-ink-muted mb-2">
                  <span className="font-mono">p{message.partition}</span>
                  <span className="font-mono">@{count(message.offset)}</span>
                  <span>{new Date(message.timestamp).toISOString()}</span>
                  <span className="text-ink-faint">{message.timestampType}</span>
                  {message.transactional ? (
                    <span className="text-accent-ink">transactional</span>
                  ) : null}
                  <span className="ml-auto">{bytes(message.sizeBytes)}</span>
                </div>
                <Field label="key" payload={message.key} tombstoneNote="no key" />
                <Field
                  label="value"
                  payload={message.value}
                  tombstoneNote="tombstone — a null value, not an empty one"
                />
                {message.headers.length > 0 ? (
                  <div className="mt-2 pt-2 border-t border-line">
                    {message.headers.map((header, index) => (
                      <div key={`${header.name}-${index}`} className="flex gap-2">
                        <span className="font-mono text-ink-muted">{header.name}</span>
                        <span className="font-mono break-all">
                          {header.value?.text ?? "—"}
                        </span>
                      </div>
                    ))}
                  </div>
                ) : null}
              </Card>
            ))}
          </div>
        </>
      )}
    </>
  );
}

function Field({
  label,
  payload,
  tombstoneNote,
}: {
  label: string;
  payload: { encoding: string; text: string; bytes: number; truncated: boolean } | null;
  tombstoneNote: string;
}) {
  return (
    <div className="grid grid-cols-[3.5rem_1fr] gap-2 items-baseline">
      <span className="text-[12px] text-ink-muted">{label}</span>
      {payload === null ? (
        <span className="text-[12px] text-ink-faint italic">{tombstoneNote}</span>
      ) : (
        <span className="font-mono break-all">
          {payload.encoding === "hex" ? (
            <span
              className="text-[11px] px-1 py-0.5 mr-2 rounded-sm"
              style={{
                background: "var(--color-surface-sunken)",
                color: "var(--color-ink-muted)",
              }}
              title="not valid UTF-8, so it is rendered as hex"
            >
              hex
            </span>
          ) : null}
          {payload.text}
          {payload.truncated ? (
            <span className="text-ink-faint"> … truncated ({bytes(payload.bytes)})</span>
          ) : null}
        </span>
      )}
    </div>
  );
}
