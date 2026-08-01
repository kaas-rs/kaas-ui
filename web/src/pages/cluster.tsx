import { useState } from "react";
import {
  useCapabilities,
  useCluster,
  useClusterConfigs,
  useLogDirs,
} from "../api/client";
import type { FeatureEntry } from "../api/types";
import {
  Card,
  ClusterCounts,
  ErrorChips,
  Mono,
  Section,
  SnapshotAge,
  Spinner,
  StatusBadge,
  Table,
  Td,
  Th,
  bytes,
  count,
} from "../components";
import { PageTitle } from "../shell";

export function ClusterOverview({ clusterId }: { clusterId: string }) {
  const cluster = useCluster(clusterId);
  const [logDirBroker, setLogDirBroker] = useState<number | null>(null);
  const logDirs = useLogDirs(clusterId, logDirBroker);

  if (cluster.isLoading) return <Spinner label={`connecting to ${clusterId}`} />;
  if (cluster.error) {
    return (
      <Card className="p-5">
        <p className="text-danger font-medium mb-1">{clusterId} is not available</p>
        <p className="text-[13px] text-ink-muted">{String(cluster.error)}</p>
        <p className="text-[13px] text-ink-muted mt-3">
          kaas-ui keeps retrying in the background; this page will fill in when the
          cluster answers. Nothing else in the fleet is affected.
        </p>
      </Card>
    );
  }

  const detail = cluster.data?.items[0];
  if (!detail) return <Spinner />;
  const card = detail.cluster;

  return (
    <>
      <PageTitle
        title={card.name}
        subtitle={
          <span className="flex items-center gap-3">
            <StatusBadge status={card.status} />
            {card.clusterId ? <Mono>{card.clusterId}</Mono> : null}
          </span>
        }
        actions={
          <SnapshotAge
            ageMs={card.snapshotAgeMs}
            maxStalenessMs={card.maxStalenessMs}
          />
        }
      />

      <ErrorChips errors={cluster.data?.errors ?? []} />

      <Section title="Cluster">
        <Card className="p-4">
          <ClusterCounts card={card} />
          {detail.description === null ? (
            <p className="text-[12px] text-ink-muted mt-4 pt-3 border-t border-line">
              This cluster does not answer <Mono>DescribeCluster</Mono>, so the
              broker list below comes from the metadata snapshot alone. Everything
              on this page is real; the one thing missing is whether the controller
              has fenced a broker.
            </p>
          ) : null}
        </Card>
      </Section>

      <Section title="Brokers">
        <Table>
          <thead>
            <tr>
              <Th>node</Th>
              <Th>host</Th>
              <Th align="right">port</Th>
              <Th>rack</Th>
              <Th align="right">leads</Th>
              <Th align="right">replicas</Th>
              <Th>role</Th>
              <Th>log dirs</Th>
            </tr>
          </thead>
          <tbody>
            {detail.brokers.map((broker) => (
              <tr key={broker.nodeId} className="hover:bg-surface-sunken">
                <Td>
                  <span className="font-mono">{broker.nodeId}</span>
                </Td>
                <Td>
                  <span className="font-mono text-ink-muted">{broker.host}</span>
                </Td>
                <Td align="right">
                  <span className="font-mono">{broker.port}</span>
                </Td>
                <Td>{broker.rack ?? <span className="text-ink-faint">—</span>}</Td>
                <Td align="right">
                  <span className="font-mono">{count(broker.leaderPartitionCount)}</span>
                </Td>
                <Td align="right">
                  <span className="font-mono">
                    {count(broker.replicaPartitionCount)}
                  </span>
                </Td>
                <Td>
                  <div className="flex gap-2">
                    {broker.isController ? (
                      <span
                        className="text-[11px] px-1.5 py-0.5 rounded-sm"
                        style={{
                          background: "var(--color-accent)",
                          color: "#3B2E2A",
                        }}
                      >
                        controller
                      </span>
                    ) : null}
                    {broker.isFenced === true ? (
                      <span
                        className="text-[11px] px-1.5 py-0.5 rounded-sm"
                        style={{
                          background: "var(--color-danger-soft)",
                          color: "var(--color-danger)",
                        }}
                      >
                        fenced
                      </span>
                    ) : broker.isFenced === null ? (
                      <span
                        className="text-[11px] text-ink-faint"
                        title="this cluster does not report fencing"
                      >
                        fencing unknown
                      </span>
                    ) : null}
                  </div>
                </Td>
                <Td>
                  <button
                    type="button"
                    onClick={() =>
                      setLogDirBroker(
                        logDirBroker === broker.nodeId ? null : broker.nodeId,
                      )
                    }
                    className="text-[12px] hover:underline"
                    style={{ color: "var(--color-link)" }}
                  >
                    {logDirBroker === broker.nodeId ? "hide" : "show"}
                  </button>
                </Td>
              </tr>
            ))}
          </tbody>
        </Table>

        {logDirBroker !== null ? (
          <div className="mt-4">
            {logDirs.isLoading ? (
              <Spinner label={`reading log dirs on broker ${logDirBroker}`} />
            ) : logDirs.error ? (
              <Card className="p-4 text-[13px] text-danger">
                broker {logDirBroker}: {String(logDirs.error)}
              </Card>
            ) : (
              <Table>
                <thead>
                  <tr>
                    <Th>path (broker {logDirBroker})</Th>
                    <Th align="right">total</Th>
                    <Th align="right">usable</Th>
                    <Th align="right">replicas</Th>
                    <Th align="right">on disk</Th>
                  </tr>
                </thead>
                <tbody>
                  {(logDirs.data?.items ?? []).map((dir) => (
                    <tr key={dir.path}>
                      <Td>
                        <span className="font-mono">{dir.path}</span>
                      </Td>
                      <Td align="right">{bytes(dir.totalBytes)}</Td>
                      <Td align="right">{bytes(dir.usableBytes)}</Td>
                      <Td align="right">{count(dir.replicas.length)}</Td>
                      <Td align="right">
                        {bytes(
                          dir.replicas.reduce(
                            (total, replica) => total + replica.sizeBytes,
                            0,
                          ),
                        )}
                      </Td>
                    </tr>
                  ))}
                </tbody>
              </Table>
            )}
          </div>
        ) : null}
      </Section>
    </>
  );
}

export function CapabilitiesPage({ clusterId }: { clusterId: string }) {
  const capabilities = useCapabilities(clusterId);
  const [showAll, setShowAll] = useState(false);

  if (capabilities.isLoading) return <Spinner label="asking a broker" />;
  if (capabilities.error) {
    return (
      <Card className="p-5 text-[13px]">
        <p className="text-danger font-medium mb-1">
          the version table could not be read
        </p>
        <p className="text-ink-muted">{String(capabilities.error)}</p>
      </Card>
    );
  }

  const data = capabilities.data;
  if (!data) return <Spinner />;

  const keys = showAll
    ? data.apiKeys
    : data.apiKeys.filter((key) => key.brokerAhead || key.negotiated === null);

  return (
    <>
      <PageTitle
        title="Capabilities"
        subtitle={
          <>
            as reported by broker{" "}
            <span className="font-mono">{data.source.nodeId ?? "?"}</span>{" "}
            <span className="text-ink-faint">({data.source.peer})</span>
          </>
        }
      />

      <Card className="p-4 mb-6 text-[13px] text-ink-muted max-w-3xl">
        The version table is <strong>per connection</strong>, deliberately: brokers
        mid-rolling-upgrade genuinely disagree, and a cluster-wide table would be
        wrong during exactly the window when being right matters. So this page names
        the broker it asked instead of pretending the answer is cluster-wide.
      </Card>

      <Section title="Features">
        <div className="grid gap-2 grid-cols-[repeat(auto-fill,minmax(22rem,1fr))]">
          {data.features.map((entry) => (
            <FeatureRow key={entry.feature} entry={entry} />
          ))}
        </div>
      </Section>

      <Section
        title={`API keys (${data.apiKeys.length} advertised, ${data.brokerAheadCount} ahead of this build)`}
        actions={
          <button
            type="button"
            onClick={() => setShowAll(!showAll)}
            className="text-[12px] hover:underline"
            style={{ color: "var(--color-link)" }}
          >
            {showAll ? "show only the interesting ones" : "show all"}
          </button>
        }
      >
        <Table>
          <thead>
            <tr>
              <Th align="right">key</Th>
              <Th>name</Th>
              <Th>broker</Th>
              <Th>kaas-ui</Th>
              <Th align="right">negotiated</Th>
              <Th>note</Th>
            </tr>
          </thead>
          <tbody>
            {keys.map((key) => (
              <tr key={key.key} className="hover:bg-surface-sunken">
                <Td align="right">
                  <span className="font-mono">{key.key}</span>
                </Td>
                <Td>
                  <span className="font-mono">{key.name}</span>
                </Td>
                <Td>
                  <span className="font-mono text-ink-muted">
                    {key.broker ? `v${key.broker[0]}–v${key.broker[1]}` : "—"}
                  </span>
                </Td>
                <Td>
                  <span className="font-mono text-ink-muted">
                    {key.ours ? `v${key.ours[0]}–v${key.ours[1]}` : "—"}
                  </span>
                </Td>
                <Td align="right">
                  <span className="font-mono">
                    {key.negotiated === null ? "—" : `v${key.negotiated}`}
                  </span>
                </Td>
                <Td>
                  {key.ours === null ? (
                    <span className="text-[12px] text-warn-ink">
                      no schema in this build
                    </span>
                  ) : key.brokerAhead ? (
                    <span className="text-[12px] text-ink-muted">broker is ahead</span>
                  ) : null}
                </Td>
              </tr>
            ))}
          </tbody>
        </Table>
      </Section>
    </>
  );
}

function FeatureRow({ entry }: { entry: FeatureEntry }) {
  const available = entry.state === "available";
  return (
    <div
      className="flex items-center justify-between gap-3 border border-line rounded-sm px-3 py-2 bg-surface-raised"
      title={
        entry.state === "unsupported"
          ? `${entry.api} (key ${entry.apiKey}): broker ${
              entry.broker ? `v${entry.broker[0]}–v${entry.broker[1]}` : "does not implement it"
            }, kaas-ui ${entry.ours ? `v${entry.ours[0]}–v${entry.ours[1]}` : "has no schema"}`
          : undefined
      }
    >
      <span className="text-[13px]">{entry.feature}</span>
      {available ? (
        <span className="text-[12px] text-ok">✓ available</span>
      ) : (
        <span className="text-[12px] text-ink-faint font-mono">
          ✕ {entry.api}
        </span>
      )}
    </div>
  );
}

export function ClusterConfigs({ clusterId }: { clusterId: string }) {
  const cluster = useCluster(clusterId);
  const brokers = cluster.data?.items[0]?.brokers ?? [];
  const [selected, setSelected] = useState<string | null>(null);
  const resource = selected ?? (brokers[0] ? `broker:${brokers[0].nodeId}` : null);
  const configs = useClusterConfigs(clusterId, resource);
  const [onlyExplicit, setOnlyExplicit] = useState(false);

  const entries = configs.data?.items[0]?.entries ?? [];
  const shown = onlyExplicit ? entries.filter((entry) => entry.isExplicit) : entries;

  return (
    <>
      <PageTitle
        title="Configuration"
        subtitle="A viewer. AlterConfigs is a mutating api and is absent from kaas-ui entirely."
      />

      <div className="flex flex-wrap items-center gap-2 mb-4">
        {brokers.map((broker) => {
          const value = `broker:${broker.nodeId}`;
          const active = resource === value;
          return (
            <button
              key={broker.nodeId}
              type="button"
              onClick={() => setSelected(value)}
              className="text-[12px] px-2.5 py-1 rounded-sm border font-mono"
              style={
                active
                  ? {
                      background: "var(--color-accent)",
                      color: "#3B2E2A",
                      borderColor: "var(--color-accent-edge)",
                    }
                  : { borderColor: "var(--color-line-strong)" }
              }
            >
              broker {broker.nodeId}
            </button>
          );
        })}
        <label className="ml-auto text-[12px] flex items-center gap-2 text-ink-muted">
          <input
            type="checkbox"
            checked={onlyExplicit}
            onChange={(event) => setOnlyExplicit(event.target.checked)}
          />
          only values someone set
        </label>
      </div>

      <ErrorChips errors={configs.data?.errors ?? []} />

      {configs.isLoading ? (
        <Spinner />
      ) : (
        <ConfigTable entries={shown} total={entries.length} />
      )}
    </>
  );
}

export function ConfigTable({
  entries,
  total,
}: {
  entries: {
    name: string;
    value: string | null;
    source: string;
    isExplicit: boolean;
    isSensitive: boolean;
    readOnly: boolean;
    documentation: string | null;
  }[];
  total?: number;
}) {
  return (
    <>
      <Table>
        <thead>
          <tr>
            <Th>key</Th>
            <Th>value</Th>
            <Th>source</Th>
          </tr>
        </thead>
        <tbody>
          {entries.map((entry) => (
            <tr key={entry.name} className="hover:bg-surface-sunken">
              <Td>
                <span className="font-mono" title={entry.documentation ?? undefined}>
                  {entry.name}
                </span>
                {entry.documentation ? (
                  <span className="text-ink-faint ml-1.5 text-[11px]">ⓘ</span>
                ) : null}
              </Td>
              <Td className="max-w-[28rem] break-all">
                {entry.isSensitive ? (
                  <span
                    className="text-[12px] px-1.5 py-0.5 rounded-sm"
                    style={{
                      background: "var(--color-surface-sunken)",
                      color: "var(--color-ink-muted)",
                    }}
                    title="the broker redacted this value"
                  >
                    redacted by the broker
                  </span>
                ) : entry.value === null ? (
                  <span className="text-ink-faint">—</span>
                ) : (
                  <span className="font-mono">{entry.value}</span>
                )}
              </Td>
              <Td>
                <span
                  className={`text-[12px] font-mono ${
                    entry.isExplicit ? "text-accent-ink font-medium" : "text-ink-faint"
                  }`}
                  title={entry.isExplicit ? "set explicitly" : "inherited default"}
                >
                  {entry.source}
                </span>
              </Td>
            </tr>
          ))}
        </tbody>
      </Table>
      {total !== undefined && total !== entries.length ? (
        <p className="text-[12px] text-ink-faint mt-2">
          {entries.length} of {total} entries
        </p>
      ) : null}
    </>
  );
}
