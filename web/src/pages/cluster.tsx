import { useState } from "react";

import { useCapabilities, useCluster, useClusterConfigs, useLogDirs } from "@/api/client";
import type { ConfigEntry } from "@/api/types";
import {
  ClusterCounts,
  ErrorChips,
  FeatureBadge,
  Mono,
  Section,
  SnapshotAge,
  Spinner,
  StatusBadge,
  bytes,
  count,
} from "@/components/domain";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { PageTitle } from "@/shell";

export function ClusterOverview({ clusterId }: { clusterId: string }) {
  const cluster = useCluster(clusterId);
  const [logDirBroker, setLogDirBroker] = useState<number | null>(null);
  const logDirs = useLogDirs(clusterId, logDirBroker);

  if (cluster.isLoading) return <Spinner label={`connecting to ${clusterId}`} />;
  if (cluster.error) {
    return (
      <Card className="p-5">
        <p className="mb-1 font-medium text-danger">{clusterId} is not available</p>
        <p className="text-[13px] text-ink-muted">{String(cluster.error)}</p>
        <p className="mt-3 text-[13px] text-ink-muted">
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
          <SnapshotAge ageMs={card.snapshotAgeMs} maxStalenessMs={card.maxStalenessMs} />
        }
      />

      <ErrorChips errors={cluster.data?.errors ?? []} />

      <Section title="Cluster">
        <Card>
          <CardContent>
            <ClusterCounts card={card} />
            {detail.description === null ? (
              <p className="mt-4 border-t pt-3 text-[12px] text-ink-muted">
                This cluster does not answer <Mono>DescribeCluster</Mono>, so the broker
                list below comes from the metadata snapshot alone. Everything on this
                page is real; the one thing missing is whether the controller has fenced
                a broker.
              </p>
            ) : null}
          </CardContent>
        </Card>
      </Section>

      <Section title="Brokers">
        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>node</TableHead>
                <TableHead>host</TableHead>
                <TableHead className="text-right">port</TableHead>
                <TableHead>rack</TableHead>
                <TableHead className="text-right">leads</TableHead>
                <TableHead className="text-right">replicas</TableHead>
                <TableHead>role</TableHead>
                <TableHead>log dirs</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {detail.brokers.map((broker) => (
                <TableRow key={broker.nodeId}>
                  <TableCell className="font-mono">{broker.nodeId}</TableCell>
                  <TableCell className="font-mono text-ink-muted">{broker.host}</TableCell>
                  <TableCell className="text-right font-mono">{broker.port}</TableCell>
                  <TableCell>
                    {broker.rack ?? <span className="text-ink-faint">—</span>}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {count(broker.leaderPartitionCount)}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {count(broker.replicaPartitionCount)}
                  </TableCell>
                  <TableCell>
                    <div className="flex gap-2">
                      {broker.isController ? (
                        <Badge
                          style={{ background: "var(--rust)", color: "#3B2E2A" }}
                          className="border-transparent"
                        >
                          controller
                        </Badge>
                      ) : null}
                      {broker.isFenced === true ? (
                        <Badge
                          style={{
                            background: "var(--danger-soft)",
                            color: "var(--danger)",
                          }}
                          className="border-transparent"
                        >
                          fenced
                        </Badge>
                      ) : broker.isFenced === null ? (
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <span className="text-[11px] text-ink-faint">
                              fencing unknown
                            </span>
                          </TooltipTrigger>
                          <TooltipContent>
                            this cluster does not report fencing — unknown, not false
                          </TooltipContent>
                        </Tooltip>
                      ) : null}
                    </div>
                  </TableCell>
                  <TableCell>
                    <Button
                      variant="link"
                      size="sm"
                      className="h-auto p-0 text-[12px]"
                      onClick={() =>
                        setLogDirBroker(
                          logDirBroker === broker.nodeId ? null : broker.nodeId,
                        )
                      }
                    >
                      {logDirBroker === broker.nodeId ? "hide" : "show"}
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>

        {logDirBroker !== null ? (
          <div className="mt-4">
            {logDirs.isLoading ? (
              <Spinner label={`reading log dirs on broker ${logDirBroker}`} />
            ) : logDirs.error ? (
              <Card className="p-4 text-[13px] text-danger">
                broker {logDirBroker}: {String(logDirs.error)}
              </Card>
            ) : (
              <div className="rounded-md border">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>path (broker {logDirBroker})</TableHead>
                      <TableHead className="text-right">total</TableHead>
                      <TableHead className="text-right">usable</TableHead>
                      <TableHead className="text-right">replicas</TableHead>
                      <TableHead className="text-right">on disk</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {(logDirs.data?.items ?? []).map((dir) => (
                      <TableRow key={dir.path}>
                        <TableCell className="font-mono">{dir.path}</TableCell>
                        <TableCell className="text-right">{bytes(dir.totalBytes)}</TableCell>
                        <TableCell className="text-right">
                          {bytes(dir.usableBytes)}
                        </TableCell>
                        <TableCell className="text-right">
                          {count(dir.replicas.length)}
                        </TableCell>
                        <TableCell className="text-right">
                          {bytes(
                            dir.replicas.reduce(
                              (total, replica) => total + replica.sizeBytes,
                              0,
                            ),
                          )}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
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
        <p className="mb-1 font-medium text-danger">the version table could not be read</p>
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

      <Card className="mb-6 max-w-3xl">
        <CardContent className="text-[13px] text-ink-muted">
          The version table is <strong>per connection</strong>, deliberately: brokers
          mid-rolling-upgrade genuinely disagree, and a cluster-wide table would be wrong
          during exactly the window when being right matters. So this page names the
          broker it asked instead of pretending the answer is cluster-wide.
        </CardContent>
      </Card>

      <Section title="Features">
        <div className="grid gap-2 grid-cols-[repeat(auto-fill,minmax(22rem,1fr))]">
          {data.features.map((entry) => (
            <div
              key={entry.feature}
              className="flex items-center justify-between gap-3 rounded-sm border bg-card px-3 py-2"
            >
              <span className="text-[13px]">{entry.feature}</span>
              <FeatureBadge entry={entry} />
            </div>
          ))}
        </div>
      </Section>

      <Section
        title={`API keys (${data.apiKeys.length} advertised, ${data.brokerAheadCount} ahead of this build)`}
        actions={
          <Button variant="link" size="sm" onClick={() => setShowAll(!showAll)}>
            {showAll ? "show only the interesting ones" : "show all"}
          </Button>
        }
      >
        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="text-right">key</TableHead>
                <TableHead>name</TableHead>
                <TableHead>broker</TableHead>
                <TableHead>kaas-ui</TableHead>
                <TableHead className="text-right">negotiated</TableHead>
                <TableHead>note</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {keys.map((key) => (
                <TableRow key={key.key}>
                  <TableCell className="text-right font-mono">{key.key}</TableCell>
                  <TableCell className="font-mono">{key.name}</TableCell>
                  <TableCell className="font-mono text-ink-muted">
                    {key.broker ? `v${key.broker[0]}–v${key.broker[1]}` : "—"}
                  </TableCell>
                  <TableCell className="font-mono text-ink-muted">
                    {key.ours ? `v${key.ours[0]}–v${key.ours[1]}` : "—"}
                  </TableCell>
                  <TableCell className="text-right font-mono">
                    {key.negotiated === null ? "—" : `v${key.negotiated}`}
                  </TableCell>
                  <TableCell>
                    {key.ours === null ? (
                      <span className="text-[12px] text-warn-ink">
                        no schema in this build
                      </span>
                    ) : key.brokerAhead ? (
                      <span className="text-[12px] text-ink-muted">broker is ahead</span>
                    ) : null}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      </Section>
    </>
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

      <div className="mb-4 flex flex-wrap items-center gap-2">
        {brokers.map((broker) => {
          const value = `broker:${broker.nodeId}`;
          const active = resource === value;
          return (
            <Button
              key={broker.nodeId}
              size="sm"
              variant={active ? "default" : "outline"}
              className="font-mono text-[12px]"
              onClick={() => setSelected(value)}
            >
              broker {broker.nodeId}
            </Button>
          );
        })}
        <Label className="ml-auto text-[12px] font-normal text-ink-muted">
          <input
            type="checkbox"
            checked={onlyExplicit}
            onChange={(event) => setOnlyExplicit(event.target.checked)}
          />
          only values someone set
        </Label>
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
  entries: ConfigEntry[];
  total?: number;
}) {
  return (
    <>
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>key</TableHead>
              <TableHead>value</TableHead>
              <TableHead>source</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {entries.map((entry) => (
              <TableRow key={entry.name}>
                <TableCell>
                  {entry.documentation ? (
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span className="font-mono">
                          {entry.name}
                          <span className="ml-1.5 text-[11px] text-ink-faint">ⓘ</span>
                        </span>
                      </TooltipTrigger>
                      <TooltipContent className="max-w-lg">
                        <span
                          // The broker's own documentation, which is HTML.
                          dangerouslySetInnerHTML={{
                            __html: entry.documentation.replace(/<[^>]*>/g, ""),
                          }}
                        />
                      </TooltipContent>
                    </Tooltip>
                  ) : (
                    <span className="font-mono">{entry.name}</span>
                  )}
                </TableCell>
                <TableCell className="max-w-[28rem] break-all whitespace-normal">
                  {entry.isSensitive ? (
                    <Badge variant="secondary" className="text-ink-muted">
                      redacted by the broker
                    </Badge>
                  ) : entry.value === null ? (
                    <span className="text-ink-faint">—</span>
                  ) : (
                    <span className="font-mono">{entry.value}</span>
                  )}
                </TableCell>
                <TableCell>
                  <span
                    className={
                      entry.isExplicit
                        ? "font-mono text-[12px] font-medium text-rust-ink"
                        : "font-mono text-[12px] text-ink-faint"
                    }
                    title={entry.isExplicit ? "set explicitly" : "inherited default"}
                  >
                    {entry.source}
                  </span>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
      {total !== undefined && total !== entries.length ? (
        <p className="mt-2 text-[12px] text-ink-faint">
          {entries.length} of {total} entries
        </p>
      ) : null}
    </>
  );
}
