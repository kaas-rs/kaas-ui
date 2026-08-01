# Phase 1 — capabilities and cluster views

*PLAN.md milestone M1.*

**Goal.** The capabilities endpoint, the degradation components that consume it,
and the first screens that can be absent: broker detail, log dirs, configs.

This is the phase that decides whether kaas-ui is honest about heterogeneous
clusters or quietly wrong about them. It is also the phase with the best
possible test fixture already running — see below.

## Why the degradation components come now, not later

PLAN.md §7 says to build them in M1 "not when a 4.4 cluster appears". Against
these clusters they are not speculative at all:

| component | live trigger, today |
|---|---|
| `UnsupportedApi` panel, showing **both** ranges | `DescribeCluster` on `kaas`: `broker: None, ours: Some((0,2))` |
| `UnknownCode` chip for `ErrorCode::Unknown(i16)` | Strimzi runs Kafka 4.2 against Kafka 4.0 error codes |
| `Undescribable` panel for `GroupDescription::Unrecognized` | api keys 88/89 have no schema in this build |
| "broker ahead of codec" note | 8 keys on Strimzi, including `ListOffsets` v11 |

Building them now costs a day. Building them later means every screen written in
between has invented its own error toast, and they all have to be unpicked.

## The capabilities endpoint

```
GET /api/clusters/{id}/capabilities
```

```json
{
  "features": {
    "consumerGroups": "available",
    "shareGroups":    { "state": "unsupported", "broker": null, "ours": [0, 1] },
    "quotas":         "available",
    "describeQuorum": { "state": "unsupported", "broker": [0, 2], "ours": null }
  },
  "source": { "kind": "broker", "nodeId": 1 }
}
```

**This contains no version logic.** It maps kaas-lib's already-negotiated table
onto UI features and does nothing else. One function, one `match`, no
arithmetic on version numbers, no knowledge of which Kafka release added what.
If it ever needs such knowledge, the knowledge belongs in kaas-lib.

```rust
// kaas-ui-core/src/capabilities.rs — the whole file, essentially
const FEATURES: &[(Feature, &[ApiKey])] = &[
    (Feature::ConsumerGroups, &[ApiKey::ConsumerGroupDescribe]),
    (Feature::ShareGroups,    &[ApiKey::ShareGroupDescribe]),
    (Feature::Quotas,         &[ApiKey::DescribeClientQuotas]),
    (Feature::Transactions,   &[ApiKey::ListTransactions, ApiKey::DescribeTransactions]),
    (Feature::Reassignments,  &[ApiKey::ListPartitionReassignments]),
    (Feature::ScramUsers,     &[ApiKey::DescribeUserScramCredentials]),
    (Feature::Quorum,         &[ApiKey::DescribeQuorum]),
    (Feature::Acls,           &[ApiKey::DescribeAcls]),
    (Feature::Producers,      &[ApiKey::DescribeProducers]),
];
```

Projected against `ApiVersions::get(key)` → `{ broker, ours, negotiated() }`.

### The wrinkle, and the interim rule

kaas-lib's version table is **per connection**, deliberately: brokers
mid-rolling-upgrade genuinely disagree, and a cluster-wide table would be wrong
during exactly the window when being right matters. There is therefore no
`cluster.capabilities()` to project from.

**kaas-ui must not fabricate one by picking a broker.** That produces a UI whose
tabs flicker depending on which connection answered — and, worse, it looks
perfect on every single-broker fixture and misbehaves on the first rolling
upgrade, which is the failure mode PLAN.md §10 names explicitly.

Until [upstream-asks.md](reference/upstream-asks.md) item 1 lands, the interim
rule is: **read the table from an explicitly named broker and say so in the
UI.** That is the `source` field above, and the frontend renders it as "as
reported by broker 1". A user who sees a surprising tab set can at least tell
where the answer came from.

When item 1 lands, `source` becomes `{ "kind": "agreed" }` or
`{ "kind": "disagreed", "brokers": { "1": …, "2": … } }`, and the UI gains the
thing no other Kafka tool has: "3 of 5 brokers support this, upgrade in
progress".

### What the frontend does with it

Tabs are rendered from capabilities, so a `kaas` cluster that does not answer
`DescribeClientQuotas` shows **no quotas tab** rather than a tab that errors on
click.

Routes still exist and render the explanatory panel, so a URL shared from a
Strimzi cluster and opened against `kaas` degrades into an explanation rather
than a dead end. This distinction — hidden tab, live route — is the whole of
PLAN.md §2's "deciding what absence looks like", and it is the one thing in this
area that genuinely cannot live in a client library.

## The rest of the phase

**Cluster detail.** `describe_cluster()` as enrichment over the Phase 0
snapshot: it adds `is_fenced` per broker and an authoritative controller id.
Where the api key is absent, render the snapshot data plus a small note, not an
error.

**Log dirs.** `describe_all_log_dirs()` → `PerItem<i32, Vec<LogDir>>`, per
broker: path, total and usable bytes, per-replica size and offset lag. Present
on both clusters. This is the first screen where the per-item envelope earns its
keep — one broker being down must not blank the page.

**Configs.** `describe_configs_documented()` for brokers and the cluster, with
`ConfigSource` rendered so "explicitly set" is distinguishable from "default".
`is_sensitive` values are redacted by the broker; render the redaction as such
rather than as an empty string.

Note `AlterConfigs` and `IncrementalAlterConfigs` are mutating and absent from
kaas-ui entirely. The config screen is a viewer.

## Traps

- **`describe_all_log_dirs` routes per broker.** It is one of the four "one
  specific broker" RPCs. A broker that is down yields an error for that node,
  not for the call.
- **`ConfigResource::broker(node_id)` takes the node id as the name.** Getting
  this wrong yields an empty result rather than an error.
- **Do not cache the capability projection forever.** A rolling upgrade changes
  it. Recompute on the same cadence as the metadata refresh, and let the UI show
  its age like everything else.
- **Feature names in the JSON are kaas-ui's vocabulary, not Kafka's.** They are
  a UI contract; adding an api key to an existing feature must not rename it.

## Acceptance

```sh
cargo xtask live --config config.dev.yaml
```

- `GET /api/clusters/kaas/capabilities` and `.../strimzi/capabilities` return
  **different feature sets**, and the difference matches the api-key diff
  between the two clusters as reported by `livetest probe`;
- specifically: `transactions`, `reassignments`, `scramUsers`, `quorum`,
  `producers` and `shareGroups` are `available` on `strimzi` and `unsupported`
  on `kaas`; `acls` is available on both;
- the two clusters side by side render **different tab sets** in the browser —
  the conformance report PLAN.md §5 promises;
- opening the transactions route on `kaas` renders the `UnsupportedApi` panel
  showing `broker: none` and `ours: 0–1`, not a spinner and not a toast;
- `GET /api/clusters/strimzi/brokers/1/log-dirs` returns per-replica sizes that
  agree with `livetest probe`;
- an injected `ErrorCode::Unknown(30000)` renders as the number 30000 with an
  `UnknownCode` chip, not as a generic failure (unit test — no broker will
  produce this on demand);
- `cargo xtask ci` still green, including the no-version-literal grep, which
  this phase is the most likely to break.

## Exit criteria

- [ ] capabilities endpoint contains no arithmetic on version numbers
- [ ] `source` field present and rendered; documented as interim
- [ ] all four degradation components built and reachable from a real URL
- [ ] tab sets differ between `kaas` and `strimzi` in the browser
- [ ] log dirs and configs render per-item errors without blanking the page
- [ ] [upstream-asks.md](reference/upstream-asks.md) item 1 filed against kaas-lib
