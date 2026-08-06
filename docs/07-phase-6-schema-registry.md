# Phase 6 — schema registry and rich payloads

*PLAN.md milestone M6.*

**Goal.** Per-environment registry client, Avro / Protobuf / JSON Schema
decoding, a schema browser with version history, per-topic serializer
overrides, and the user JS predicate.

Fills out `crates/kaas-ui-serde` behind the `PayloadCodec` trait Phase 3
established.

## The registry is per environment, not per cluster

A schema registry serves an **environment** — the dev one, the staging one, the
production one — and every cluster in that environment shares it. It is not a
property of a cluster the way a bootstrap list is. Two clusters in `dev`
resolve schema id 42 to the same schema, because it is the same registry
answering both.

So a registry is declared once and clusters *reference* one by id:

```yaml
schema_registries:
  - id: dev
    url: http://apicurio-registry.apicurio.svc.cluster.local:8080/apis/ccompat/v7

clusters:
  - id: strimzi
    labels: { env: dev }
    schema_registry: dev
  - id: kaas
    labels: { env: dev }
    # none — and that is fine
```

Reference, not membership: a cluster in an environment need not use that
environment's registry. A Strimzi cluster with Apicurio and a kaas instance
with none coexist fine, and the registry client is an
`Option<Arc<RegistryHandle>>` on `ClusterHandle` — **shared, not owned** — so
every code path that touches it still has an absent branch, because that branch
is the common case on `kaas`. Add a second cluster to `dev` and it names the
same id, gets the same client and the same warm cache: no second Apicurio, no
duplicated URL, no second copy of every schema in memory.

`RegistryHandle` is one set of `schema_registry_converter` decoders — one per
format, built once from that registry's settings. Sharing is therefore a
property of *construction*, not a cache someone remembered to key correctly:
the decoders hold the id→schema cache themselves, so a decoder built per
cluster would be a second cache, and one built per request would be no cache at
all.

The binding is that explicit `schema_registry: <id>` line and not the `env`
label, which stays what it is today — free-form display metadata. A typo in a
label would silently point a cluster at nothing; an unknown registry id is a
startup error naming the id, like every other unknown key in the config.

> **Note.** `kafbat-ui`'s config in this cluster points at
> `apicurio-registry.apicurio.svc.cluster.local:8080`, but there is **no
> `apicurio` namespace** — that URL is stale and resolves to nothing. If a
> registry is wanted for development, deploying one (`apps/apicurio/` exists in
> the cluster repo) is a prerequisite for the live half of this phase's
> acceptance. Until then the codecs are exercised against fixtures.

## Codecs

`schema_registry_converter` 4.10 is **the** serde library — one crate resolving
schema ids and decoding all three registry-backed formats, rather than three
integrations sharing a hand-rolled framing parser.

| format                    | decoded by                  | notes                                                         |
|---------------------------|-----------------------------|---------------------------------------------------------------|
| raw / hex / string / JSON | —                           | Phase 3, no dependencies, no registry                         |
| Avro                      | `schema_registry_converter` | schema resolved by id                                         |
| JSON Schema               | `schema_registry_converter` | framed JSON — a decode path, not only validation              |
| Protobuf                  | `schema_registry_converter` | descriptor resolved by id: no codegen, no configured `.proto` |
| `__consumer_offsets`      | hand-rolled                 | **display only** — group views still go through `OffsetFetch` |

All three registry formats are the same Confluent wire format — magic byte `0`,
a 4-byte big-endian schema id, then the body — and all three resolve that id
against the registry. That sameness is the argument for one library.

Decoding happens **on the server**, in `kaas-ui-serde`, and the choice is
forced rather than preferred: the registry is an in-cluster service name a
browser cannot resolve, and the JS predicate below runs over decoded values
before the response exists. A browser-side decoder would mean proxying the
registry to the public surface *and* shipping every record so the predicate
could reject it.

### ccompat only

The registry must speak the Confluent API — for Apicurio, the
`/apis/ccompat/v7` endpoint the config above already points at. The native
`/apis/registry/v3` API is **not supported**, and that is a decision, not a gap:
one wire format and one client is what buys three formats for one integration.

So the `url` is checked when the registry is first reached, and one that
answers but is not ccompat is a **config error naming the endpoint expected**.
The failure mode to design against is a deployment where every record on every
Avro topic renders as hex and the cause is one missing path segment.

### The magic byte routes, it does not guess

Sniffing byte 0 is still the first step, but with one library owning the framed
formats it no longer means "detect the codec" — it decides whether the payload
is registry-backed at all.

- **framed** → `schema_registry_converter` resolves the id, and the registry
  says whether 42 is Avro, JSON Schema or Protobuf. Nothing is left to guess.
- **not framed** → a Phase 3 codec, from per-topic config, defaulting to raw.

An unframed payload is therefore **not a decode error**. It is a payload that
was never registry-backed, and the softness the old sniff needed is now
structural: the two paths never compete for the same bytes.

**Always show what was chosen and let the user override it.** Auto-detection
that cannot be corrected is worse than none — the chip in `RecordRow` is the
override control, not a label. The override is only free in one direction:
falling back to hex or string needs no schema and no refetch, because the raw
bytes travel beside the decoded value, while overriding *up* to Avro cannot
invent a schema id and is refused with a reason.

Key and value are decoded independently. A JSON key with an Avro value is
common and must not require choosing one.

## The schema browser

Subjects, versions, the schema text with syntax highlighting in Monaco, and the
diff between two versions. Compatibility mode per subject where the registry
reports it.

It is a view of a **registry**, reached from a cluster. Two clusters sharing
`dev` show the same subject list, so the browser says which registry is
answering rather than implying the subjects belong to the cluster whose nav you
arrived through. The routes stay rooted at `/api/clusters/{id}/schemas`, as in
`docs/reference/http-contract.md` — there is deliberately no
`/api/schema-registries/{id}`, so registry ids never become a second
enumerable namespace beside cluster ids.

Read-only, like everything else: no registering, no compatibility changes.

## The JS predicate

A user-supplied JavaScript expression over the **decoded** value, run in
`rquickjs` 0.12 with a hard memory cap and an interrupt handler. This is the
feature that makes searching a large topic practical and it is also the only
place kaas-ui runs code it did not write.

Three non-negotiables:

1. **Hard memory cap and an interrupt handler**, both set before the first
   evaluation. A predicate that allocates or loops forever must be killed by the
   runtime, not by the pod's OOM killer taking every other cluster down with it.
2. **Never run it on a record a cheap filter could have dropped.** kaas-lib's
   `RecordFilter` — offset, timestamp, partition, key prefix, headers — runs
   first, before deserialization. This ordering is a correctness property of the
   design.
3. **No host bindings.** No fetch, no fs, no timers. The predicate sees one
   argument and returns a boolean.

The evaluation budget is per record and the scan reports how many records were
skipped by the budget, so a predicate that is too slow is visible rather than
mysterious.

## Traps

- **Confluent framing is not universal, and that is why ccompat is required.**
  Apicurio's `ccompat` endpoint speaks it; Apicurio's native API does not. The
  supported configuration is the one where framing is guaranteed, so a
  `url` pointing at the native API is caught as configuration rather than
  rediscovered once per record.
- **Schema ids are cached, schemas are immutable.** Cache by id forever; cache
  subject→version listings briefly.
- **The id cache belongs to the registry, and lives inside the decoders.** A
  schema id is unique *within* a registry, not across them — building a decoder
  per cluster gives N copies of one cache, and a `schema_registry_converter`
  decoder handed the wrong registry's settings would answer `dev`'s schema 42
  as `prod`'s. One decoder set per registry id makes both mistakes
  unrepresentable, which is why `RegistryHandle` is shared rather than cloned.
- **A subject on two clusters is one subject.** `TopicNameStrategy` turns topic
  `orders` into subject `orders-value` whichever cluster in the environment it
  was produced to. That is not a collision to disambiguate — it is the registry
  doing its job. Do not prefix subjects with a cluster id.
- **A registry outage degrades an environment, not a cluster.** Connect and
  back off once per registry: ten clusters sharing `dev` must not mean ten
  retry storms against one unreachable Apicurio. Connect lazily too, for the
  same reason clusters do — an unreachable registry must not block startup or
  `/health`.
- **A shared registry must not leak invisible clusters.** Cluster visibility is
  a 404 here, and "which clusters use this registry" is a list that can name a
  cluster the caller may not see. It goes through the same registry lookup as
  everything else; a caller reaches a registry only via a cluster they can
  already see.
- **Per-topic overrides stay per cluster.** The registry is shared; the
  decision that `orders` on `strimzi` is Avro is not. The override key is
  (cluster, topic, key-or-value).
- **A registry outage must not break the message view.** Fall back to hex with a
  visible note naming the registry. The records are still there.
- **Protobuf descriptors come from the registry, and nowhere else.** The old
  open question — descriptor set uploaded through config, or fetched from the
  registry — is closed by this choice: ccompat stores the schema, the wire
  format carries the id, and Protobuf resolves exactly like Avro. No configured
  `.proto`, no rebuild to add a message type, and no second source of truth
  that can disagree with the id in the payload.
- **Schema references resolve transitively.** A `.proto` that imports another
  subject, or an Avro schema naming a record defined elsewhere, is a reference
  the registry stores separately. A resolver that fetches only the id in the
  payload decodes the simple topics and fails on the interesting ones, and it
  fails at *decode* time rather than at configuration time.
- **A decoded value is not a DTO.** Whatever `schema_registry_converter` hands
  back is an upstream type, and rule 4 says it never reaches a `utoipa` schema.
  Convert to kaas-ui's own decoded-value type at the boundary, or a bump in a
  serde library rewrites the generated TypeScript client.
- **An Avro decode failure is not a malformed batch.** Phase 3 established two
  distinct rows; this is the phase where the second one starts firing.

## Acceptance

- an Avro-encoded topic decodes with the schema id resolved from the registry,
  and the id is shown;
- the same topic renders as hex when the codec is overridden to raw, without a
  refetch;
- a Protobuf topic decodes with its descriptor resolved from the registry by
  schema id, with nothing about that topic in the config;
- a JSON Schema topic decodes through the framed path, and a record that parses
  but violates its subject is shown as non-conforming rather than as valid;
- a schema carrying a reference to another subject decodes, both formats;
- a registry `url` pointing at Apicurio's native API is reported as a
  **configuration** error naming the ccompat endpoint — on first use, since
  connecting is lazy — and no topic silently degrades to hex instead;
- an unframed payload on a topic with no per-topic codec renders as raw without
  producing a decode-error row — absence of framing is not a failure;
- two clusters referencing the same `schema_registry` id share one client and
  one cache: after the first resolves schema id 42, the second decodes a record
  carrying that id with **zero** registry requests, asserted with a counter;
- the schema browser reached from either of those clusters shows the same
  subject list and names the registry answering;
- an unknown `schema_registry: <id>` reference fails startup with the id named,
  rather than starting with decoding silently off;
- registry unreachable → every cluster in that environment still renders records
  as hex, with a visible note naming the registry, and the page does not error;
- that unreachable registry is retried on one backoff schedule, not one per
  referencing cluster, and it delays neither startup nor `/health`;
- a payload that is not valid Avro renders as a **payload** error row, visibly
  different from a malformed-batch row;
- a JS predicate `v => v.amount > 100` filters a JSON topic correctly;
- a predicate `while(true){}` is killed by the interrupt handler within the
  budget and reported, and the server's RSS is unchanged afterwards;
- a predicate allocating in a loop hits the memory cap and is killed, not OOM;
- with a partition filter and a JS predicate both set, the JS predicate is
  evaluated **zero times** for records outside the partition — asserted with a
  counter, because this is the property most likely to regress silently.

## Exit criteria

- [ ] Avro, JSON Schema and Protobuf through `schema_registry_converter`,
      ccompat only, beside the Phase 3 codecs behind one trait
- [ ] per-topic override, choice always visible, framing decides the path
- [ ] key and value decoded independently
- [ ] registry per environment: declared once, referenced by id, one client and
      one cache shared by every cluster that names it
- [ ] caches keyed by `(registry id, schema id)`, never by cluster
- [ ] a registry absent on `kaas`, and absence is a normal path
- [ ] JS predicate sandboxed: memory cap, interrupt handler, no host bindings
- [ ] cheap filters provably run first
- [ ] `__consumer_offsets` decoded for display only
