# Phase 6 — schema registry and rich payloads

*PLAN.md milestone M6.*

**Goal.** Per-cluster registry client, Avro / Protobuf / JSON Schema decoding, a
schema browser with version history, per-topic serializer overrides, and the
user JS predicate.

Fills out `crates/kaas-ui-serde` behind the `PayloadCodec` trait Phase 3
established.

## The registry is per cluster

```yaml
clusters:
  - id: strimzi
    schema_registry:
      url: http://apicurio-registry.apicurio.svc.cluster.local:8080/apis/ccompat/v7
  - id: kaas
    # none — and that is fine
```

A Strimzi cluster with Apicurio and a kaas instance with none coexist fine. The
registry client is a field on `ClusterHandle`, `Option`al, and every code path
that touches it already has an absent branch because that branch is the common
case on `kaas`.

> **Note.** `kafbat-ui`'s config in this cluster points at
> `apicurio-registry.apicurio.svc.cluster.local:8080`, but there is **no
> `apicurio` namespace** — that URL is stale and resolves to nothing. If a
> registry is wanted for development, deploying one (`apps/apicurio/` exists in
> the cluster repo) is a prerequisite for the live half of this phase's
> acceptance. Until then the codecs are exercised against fixtures.

## Codecs

| format | crate | notes |
|---|---|---|
| raw / hex / string / JSON | — | Phase 3, no dependencies |
| Avro | `apache-avro` 0.21 + `schema_registry_converter` 4.10 | Confluent wire format: magic byte, 4-byte schema id, payload |
| Protobuf | `prost-reflect` 0.16 | dynamic decode from a `FileDescriptorSet`, no codegen |
| JSON Schema | `jsonschema` 0.49 | display-time validation, not decoding |
| `__consumer_offsets` | hand-rolled | **display only** — group views still go through `OffsetFetch` |

Sniff the Confluent magic byte first, fall back to per-topic config, then raw.
**Always show what was chosen and let the user override it.** Auto-detection
that cannot be corrected is worse than none — the chip in `RecordRow` is the
override control, not a label.

Key and value are decoded independently. A JSON key with an Avro value is
common and must not require choosing one.

## The schema browser

Subjects, versions, the schema text with syntax highlighting in Monaco, and the
diff between two versions. Compatibility mode per subject where the registry
reports it.

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

- **Confluent framing is not universal.** Apicurio's `ccompat` endpoint speaks
  it; Apicurio's native API does not. The magic-byte sniff must fail *softly*
  into the per-topic config rather than declaring the payload corrupt.
- **Schema ids are cached, schemas are immutable.** Cache by id forever; cache
  subject→version listings briefly.
- **A registry outage must not break the message view.** Fall back to hex with a
  visible note. The records are still there.
- **`prost-reflect` needs a descriptor set from somewhere** — uploaded through
  config, or fetched from a registry that stores them. Decide which and say so;
  "protobuf support" that requires a rebuild to add a message type is not
  support.
- **An Avro decode failure is not a malformed batch.** Phase 3 established two
  distinct rows; this is the phase where the second one starts firing.

## Acceptance

- an Avro-encoded topic decodes with the schema id resolved from the registry,
  and the id is shown;
- the same topic renders as hex when the codec is overridden to raw, without a
  refetch;
- a Protobuf topic decodes from a configured descriptor set;
- registry unreachable → records still render as hex, with a visible note, and
  the page does not error;
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

- [ ] four codecs behind one trait, per-topic override, choice always visible
- [ ] key and value decoded independently
- [ ] registry per cluster, absent on `kaas`, and absence is a normal path
- [ ] JS predicate sandboxed: memory cap, interrupt handler, no host bindings
- [ ] cheap filters provably run first
- [ ] `__consumer_offsets` decoded for display only
