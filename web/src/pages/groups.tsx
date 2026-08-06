import { Link } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";

import { useCapabilities, useGroup, useGroupOffsets, useGroups } from "@/api/client";
import type { GroupDetail as GroupDetailType, GroupMember } from "@/api/types";
import {
  Empty,
  ErrorChips,
  LagCell,
  Mono,
  Section,
  Spinner,
  UnsupportedApiPanel,
  count,
  featureState,
} from "@/components/domain";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { PageTitle } from "@/components/page-title";

export function Groups({ clusterId }: { clusterId: string }) {
  const capabilities = useCapabilities(clusterId);
  const groups = useGroups(clusterId);

  // The route exists even where the api does not, so a URL shared from one
  // cluster and opened against another degrades into an explanation rather
  // than a dead end.
  const state = featureState(capabilities.data?.features, "consumerGroups");
  if (state?.state === "unsupported") {
    return (
      <>
        <PageTitle title="Consumer groups" />
        <UnsupportedApiPanel
          api={state.api}
          apiKey={state.apiKey}
          broker={state.broker}
          ours={state.ours}
          what="the group list"
        />
      </>
    );
  }

  const items = groups.data?.items ?? [];

  return (
    <>
      <PageTitle title="Consumer groups" subtitle={`${count(items.length)} listed`} />
      <ErrorChips errors={groups.data?.errors ?? []} />

      {groups.isLoading ? (
        <Spinner />
      ) : items.length === 0 ? (
        <Empty>this cluster has no groups</Empty>
      ) : (
        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>group</TableHead>
                <TableHead>state</TableHead>
                <TableHead>type</TableHead>
                <TableHead>protocol</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {items.map((group) => (
                <TableRow key={group.groupId}>
                  <TableCell>
                    {group.describable ? (
                      <Link
                        to="/clusters/$clusterId/groups/$groupId"
                        params={{ clusterId, groupId: group.groupId }}
                        className="font-mono hover:underline"
                        style={{ color: "var(--rust-ink)" }}
                      >
                        {group.groupId}
                      </Link>
                    ) : (
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <span className="font-mono text-ink-muted">
                            {group.groupId}
                          </span>
                        </TooltipTrigger>
                        <TooltipContent>
                          this build has no schema for this group kind
                        </TooltipContent>
                      </Tooltip>
                    )}
                  </TableCell>
                  <TableCell>
                    <GroupState state={group.state} />
                  </TableCell>
                  <TableCell className="font-mono text-ink-muted">
                    {group.groupType || (
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <span className="text-ink-faint">unreported</span>
                        </TooltipTrigger>
                        <TooltipContent>
                          this broker is too old to report a group type; it takes the
                          classic path
                        </TooltipContent>
                      </Tooltip>
                    )}
                  </TableCell>
                  <TableCell className="font-mono text-ink-muted">
                    {group.protocolType}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </>
  );
}

function GroupState({ state }: { state: string }) {
  const tone =
    state === "Stable"
      ? "text-ok"
      : state === "Empty" || state === "Dead"
        ? "text-ink-faint"
        : "text-warn-ink";
  return <span className={`text-[12px] font-medium ${tone}`}>{state}</span>;
}

export function GroupDetail({
  clusterId,
  groupId,
}: {
  clusterId: string;
  groupId: string;
}) {
  const group = useGroup(clusterId, groupId);
  const offsets = useGroupOffsets(clusterId, groupId);

  const detail = group.data?.items[0];

  return (
    <>
      <PageTitle
        title={<span className="font-mono text-[18px]">{groupId}</span>}
        subtitle={detail ? <GroupSubtitle detail={detail} /> : undefined}
        actions={
          <Button variant="ghost" size="sm" asChild>
            <Link to="/clusters/$clusterId/groups" params={{ clusterId }}>
              <ArrowLeft aria-hidden />
              all groups
            </Link>
          </Button>
        }
      />

      <ErrorChips errors={group.data?.errors ?? []} />

      {group.isLoading ? (
        <Spinner />
      ) : !detail ? (
        <Card className="p-5 text-[13px] text-ink-muted">
          the cluster did not describe this group
        </Card>
      ) : detail.kind === "unrecognized" ? (
        // A *successful* description of an undescribable group: it exists, it
        // is listed, and this build has no schema for its kind. That is a
        // different thing from a failure and it renders differently.
        <Card className="max-w-2xl p-5">
          <h3 className="mb-2 font-semibold">This group cannot be opened</h3>
          <p className="text-[13px] text-ink-muted">
            The cluster reports it as <Mono>{detail.groupType || "an unnamed type"}</Mono>
            , which this build of kaas-ui has no schema for. The group is real and its
            state is <Mono>{detail.state}</Mono>; what is missing is the ability to
            describe its members. Upgrading kaas-ui is what changes this.
          </p>
        </Card>
      ) : (
        <Members members={detail.members} />
      )}

      <Section title="Committed offsets">
        <ErrorChips errors={offsets.data?.errors ?? []} />
        {offsets.isLoading ? (
          <Spinner />
        ) : (offsets.data?.items.length ?? 0) === 0 ? (
          <Empty>this group has committed no offsets</Empty>
        ) : (
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>topic</TableHead>
                  <TableHead className="text-right">partition</TableHead>
                  <TableHead className="text-right">committed</TableHead>
                  <TableHead className="text-right">log end</TableHead>
                  <TableHead className="text-right">lag</TableHead>
                  <TableHead>metadata</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {(offsets.data?.items ?? []).map((row) => (
                  <TableRow key={`${row.topic}-${row.partition}`}>
                    <TableCell>
                      <Link
                        to="/clusters/$clusterId/topics/$topic"
                        params={{ clusterId, topic: row.topic }}
                        className="font-mono hover:underline"
                        style={{ color: "var(--rust-ink)" }}
                      >
                        {row.topic}
                      </Link>
                    </TableCell>
                    <TableCell className="text-right font-mono">
                      {row.partition}
                    </TableCell>
                    <TableCell className="text-right font-mono">
                      {count(row.committedOffset)}
                    </TableCell>
                    <TableCell className="text-right font-mono">
                      {count(row.latestOffset)}
                    </TableCell>
                    <TableCell className="text-right">
                      <LagCell lag={row.lag} />
                    </TableCell>
                    <TableCell className="font-mono text-[12px] text-ink-faint">
                      {row.metadata ?? ""}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </Section>
    </>
  );
}

function GroupSubtitle({ detail }: { detail: GroupDetailType }) {
  if (detail.kind === "unrecognized") {
    return (
      <span className="flex gap-3">
        <span>{detail.state}</span>
        <Mono>{detail.groupType || "unnamed kind"}</Mono>
      </span>
    );
  }
  if (detail.kind === "classic") {
    return (
      <span className="flex gap-3">
        <span>classic · {detail.state}</span>
        <Mono>{detail.protocol || detail.protocolType}</Mono>
        <span>{detail.members.length} members</span>
      </span>
    );
  }
  return (
    <span className="flex gap-3">
      <span>
        {detail.kind} · {detail.state}
      </span>
      <Mono>{detail.assignor}</Mono>
      <span>
        epoch {detail.groupEpoch}/{detail.assignmentEpoch}
      </span>
      <span>{detail.members.length} members</span>
    </span>
  );
}

function Members({ members }: { members: GroupMember[] }) {
  if (members.length === 0) {
    return <Empty>no members — the group exists but nothing is consuming</Empty>;
  }
  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>member</TableHead>
            <TableHead>client</TableHead>
            <TableHead>host</TableHead>
            <TableHead className="text-right">epoch</TableHead>
            <TableHead>assignment</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {members.map((member) => (
            <TableRow key={member.memberId}>
              <TableCell>
                <span className="font-mono text-[12px] break-all">
                  {member.memberId}
                </span>
                {member.instanceId ? (
                  <span className="block text-[11px] text-ink-faint">
                    static: {member.instanceId}
                  </span>
                ) : null}
              </TableCell>
              <TableCell className="font-mono">{member.clientId}</TableCell>
              <TableCell className="font-mono text-ink-muted">
                {member.clientHost}
              </TableCell>
              <TableCell className="text-right font-mono">
                {member.memberEpoch ?? "—"}
              </TableCell>
              <TableCell>
                {member.assignment.length === 0 ? (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span className="text-[12px] text-ink-faint">not reported</span>
                    </TooltipTrigger>
                    <TooltipContent>
                      the classic protocol carries an assignor-defined blob that kaas-ui
                      does not guess at
                    </TooltipContent>
                  </Tooltip>
                ) : (
                  <div className="flex flex-col gap-0.5">
                    {member.assignment.map((assignment) => (
                      <span key={assignment.topic} className="font-mono text-[12px]">
                        {assignment.topic}{" "}
                        <span className="text-ink-faint">
                          [{assignment.partitions.join(", ")}]
                        </span>
                      </span>
                    ))}
                  </div>
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
