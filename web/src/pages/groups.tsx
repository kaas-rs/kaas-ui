import { Link } from "@tanstack/react-router";
import { useCapabilities, useGroup, useGroupOffsets, useGroups } from "../api/client";
import type { GroupDetail as GroupDetailType, GroupMember } from "../api/types";
import {
  Card,
  Empty,
  ErrorChips,
  LagCell,
  Mono,
  Section,
  Spinner,
  Table,
  Td,
  Th,
  UnsupportedApiPanel,
  count,
  featureState,
} from "../components";
import { PageTitle } from "../shell";

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
        <Table>
          <thead>
            <tr>
              <Th>group</Th>
              <Th>state</Th>
              <Th>type</Th>
              <Th>protocol</Th>
            </tr>
          </thead>
          <tbody>
            {items.map((group) => (
              <tr key={group.groupId} className="hover:bg-surface-sunken">
                <Td>
                  {group.describable ? (
                    <Link
                      to="/clusters/$clusterId/groups/$groupId"
                      params={{ clusterId, groupId: group.groupId }}
                      className="font-mono hover:underline"
                      style={{ color: "var(--color-accent-ink)" }}
                    >
                      {group.groupId}
                    </Link>
                  ) : (
                    <span
                      className="font-mono text-ink-muted"
                      title="this build has no schema for this group kind"
                    >
                      {group.groupId}
                    </span>
                  )}
                </Td>
                <Td>
                  <GroupState state={group.state} />
                </Td>
                <Td>
                  <span className="font-mono text-ink-muted">
                    {group.groupType || (
                      <span
                        className="text-ink-faint"
                        title="this broker is too old to report a group type; it takes the classic path"
                      >
                        unreported
                      </span>
                    )}
                  </span>
                </Td>
                <Td>
                  <span className="font-mono text-ink-muted">{group.protocolType}</span>
                </Td>
              </tr>
            ))}
          </tbody>
        </Table>
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
          <Link
            to="/clusters/$clusterId/groups"
            params={{ clusterId }}
            className="text-[13px] hover:underline"
            style={{ color: "var(--color-link)" }}
          >
            ← all groups
          </Link>
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
        <Card className="p-5 max-w-2xl">
          <h3 className="font-semibold mb-2">This group cannot be opened</h3>
          <p className="text-[13px] text-ink-muted">
            The cluster reports it as{" "}
            <Mono>{detail.groupType || "an unnamed type"}</Mono>, which this build
            of kaas-ui has no schema for. The group is real and its state is{" "}
            <Mono>{detail.state}</Mono>; what is missing is the ability to describe
            its members. Upgrading kaas-ui is what changes this.
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
          <Table>
            <thead>
              <tr>
                <Th>topic</Th>
                <Th align="right">partition</Th>
                <Th align="right">committed</Th>
                <Th align="right">log end</Th>
                <Th align="right">lag</Th>
                <Th>metadata</Th>
              </tr>
            </thead>
            <tbody>
              {(offsets.data?.items ?? []).map((row) => (
                <tr key={`${row.topic}-${row.partition}`} className="hover:bg-surface-sunken">
                  <Td>
                    <Link
                      to="/clusters/$clusterId/topics/$topic"
                      params={{ clusterId, topic: row.topic }}
                      className="font-mono hover:underline"
                      style={{ color: "var(--color-accent-ink)" }}
                    >
                      {row.topic}
                    </Link>
                  </Td>
                  <Td align="right">
                    <span className="font-mono">{row.partition}</span>
                  </Td>
                  <Td align="right">
                    <span className="font-mono">{count(row.committedOffset)}</span>
                  </Td>
                  <Td align="right">
                    <span className="font-mono">{count(row.latestOffset)}</span>
                  </Td>
                  <Td align="right">
                    <LagCell lag={row.lag} />
                  </Td>
                  <Td>
                    <span className="font-mono text-ink-faint text-[12px]">
                      {row.metadata ?? ""}
                    </span>
                  </Td>
                </tr>
              ))}
            </tbody>
          </Table>
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
    <Table>
      <thead>
        <tr>
          <Th>member</Th>
          <Th>client</Th>
          <Th>host</Th>
          <Th align="right">epoch</Th>
          <Th>assignment</Th>
        </tr>
      </thead>
      <tbody>
        {members.map((member) => (
          <tr key={member.memberId} className="hover:bg-surface-sunken">
            <Td>
              <span className="font-mono text-[12px] break-all">{member.memberId}</span>
              {member.instanceId ? (
                <span className="block text-[11px] text-ink-faint">
                  static: {member.instanceId}
                </span>
              ) : null}
            </Td>
            <Td>
              <span className="font-mono">{member.clientId}</span>
            </Td>
            <Td>
              <span className="font-mono text-ink-muted">{member.clientHost}</span>
            </Td>
            <Td align="right">
              <span className="font-mono">{member.memberEpoch ?? "—"}</span>
            </Td>
            <Td>
              {member.assignment.length === 0 ? (
                <span
                  className="text-ink-faint text-[12px]"
                  title="the classic protocol carries an assignor-defined blob that kaas-ui does not guess at"
                >
                  not reported
                </span>
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
            </Td>
          </tr>
        ))}
      </tbody>
    </Table>
  );
}
