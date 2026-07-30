---
status: locked
date: 2026-07-29
authors: [codex, claude, morgan]
purpose: >
  Makes CR-D41 corpus-unique handle identity executable at anneal-core's typed
  fact and provider boundary. Defines one HandleId domain across every handle
  kind, rejects empty identities at construction, and rejects duplicate public
  ids transactionally when FactStore merges source generations.
depends-on:
  - 2026-05-13-corpus-runtime.md
  - 2026-07-29-anneal-core-public-api-altitude.md
---

# Handle identity boundary — 2026-07-29

## 1. The stale proposal

The 2026-04-08 review proposed three neighboring wrappers:

1. a `HandleIdentity` enum with one variant per handle kind;
2. a validated `Status` string;
3. `DateTime<Utc>` for snapshot timestamps.

The concerns were real primitive-obsession smells, but the proposed types
predate the v2 fact model. They no longer form one coherent change.

CR-D41 defines `*handle.id` as one corpus-unique identity domain across every
loaded source and every handle kind. The same domain appears in graph
endpoints, content ownership, snapshot state, retrieval providers, ranking,
and public query predicates. An unresolved edge endpoint has an identity
before its referent or kind is known. Per-kind identity variants would
therefore require the result of resolution in order to represent the input to
resolution. The proposed enum encodes the wrong order of knowledge.

The current type boundary instead uses raw strings:

```
HandleFact.id
EdgeFact.from / EdgeFact.to
MetaFact.handle
ContentFact.handle
SpanFact.handle
ConcernFact.member
SnapshotFact.id / SnapshotEntryFact.id
search and retrieval request/result handles
runtime graph indexes
```

`FactStore` validates corpus/source scope and generation, but it does not
validate CR-D41 uniqueness. A nominal wrapper alone would distinguish handles
from other strings at compile time while leaving the governing runtime
invariant as prose.

## 2. Research grounding

Three research-graph claims constrain the choice:

- **Nominal equality on entity references rather than structural equality
  prevents spurious entitlements when entity attributes coincide.** Handle
  references likewise need one nominal domain independent of their textual
  shape or eventual kind.
- **A primary key is a minimal domain combination that uniquely identifies
  every tuple in a relation.** The public handle id is the minimal graph key;
  source-native identity remains a separate provenance and retraction key.
- **Unnecessary data abstraction introduces subjectivity and data hiding that
  undermines referential transparency.** A wrapper earns its cost only by
  encoding a real distinction or invariant. A type name is not enough.

The implementation follows the related information-hiding rule that data and
the procedures validating it share one owner: `HandleId` owns local validity;
`FactStore` owns corpus-wide uniqueness because only the merged store can know
it.

## 3. Decision

### 3.1 One root-facade `HandleId`

`anneal-core` exposes one opaque `HandleId` at the crate root. Both supported
consumer classes require it: adapters emit handle-bearing facts and hosts
provide, rank, retrieve, and query those handles. It therefore passes the
public-facade admission test.

`HandleId`:

- owns a private `String`;
- rejects the empty string at construction and deserialization;
- derives equality, ordering, hashing, cloning, and transparent string serde;
- exposes only the conversions needed at the boundary (`as_str`, `Display`,
  `AsRef<str>`, and `Borrow<str>`);
- preserves input bytes exactly and does not normalize paths, fragments, URLs,
  prefixes, case, or Unicode.

Non-emptiness is the only context-free validity rule. Syntax depends on the
source and handle kind; existence and kind may remain unresolved; corpus
uniqueness requires merged store context. The type must not pretend to enforce
those stronger properties locally.

The public name is `HandleId`, not `HandleIdentity`, because `HandleId` is
already the language and output-schema term. There are no per-kind variants.

### 3.2 Complete typed-boundary migration

Every typed field whose semantic domain is CR-D41 handle identity uses
`HandleId`. This includes stored facts (including concern members), history
handle keys, search and retrieval provider contracts, ranking candidates, and
internal typed graph and content indexes. Authorization policy targets, trail
references, and code-drift evidence requests also carry the same identity
domain rather than reconstructing it from strings. The migration does not
include:

- source-native ids, which remain `NativeId`;
- span ids, snapshot ids, edge kinds, statuses, files, or arbitrary config and
  metadata values;
- Datalog `Value::String`, serialized relation rows, CLI arguments, or rendered
  output.

The dynamic runtime converts between `HandleId` and string values at its typed
VM/store boundary. There is one canonical nominal type, not a typed fact model
beside an untyped provider model.

### 3.3 CR-D41 enforcement at merge

Before mutating `FactStore`, merge computes the prospective handle set after
the requested generation operation:

- a full snapshot excludes the prior rows for that `(corpus, source)`;
- a delta excludes rows whose native ids are replaced or retracted;
- rows for other corpora remain independent.

The merge fails with a named `StoreError` if two surviving `*handle` rows in
one corpus carry the same `HandleId`. This covers duplicates within the
incoming batch and collisions with retained rows from the same or another
source. Re-emitting an id while replacing its previous source generation or
native id remains valid.

Validation happens before removal or insertion, so a rejected merge leaves the
store byte-for-byte unchanged. The error names the corpus and public id; source
and native identities remain available to diagnose both claimants.

This hard failure is the sole intentional semantic delta. It converts silent
tolerance of a CR-D41 violation into enforcement. Valid corpora and all query
and rendering bytes remain unchanged.

## 4. Rejected adjacent wrappers

### 4.1 Status does not yet earn a type

Status vocabulary is project-extensible. There is no universal constructor
that can truthfully certify a string as a valid status; validity depends on
project lattice and dependency-classification facts. Snapshot and config
relations also carry status through generic key/value rows.

A `Status` or `StatusName` wrapper on only `HandleFact.status` would therefore
be partial typing, and a wrapper with no invariant would be a label rather
than a model. No status wrapper ships in this change.

Reopen when an observed cross-domain mix-up demonstrates nominal value, or
when a second status-specific typed operation establishes a coherent
construction and validation contract.

### 4.2 Snapshot time is a separate altitude decision

Snapshot `at` accepts either an exact ISO date or RFC3339. `DateTime<Utc>` is
not byte-compatible with that domain: it cannot represent a date-only value
without invention and it normalizes offsets and wire text.

A future truthful type is `SnapshotTime`: validated date-or-RFC3339 input,
original bytes retained, and parsed day cached. It does not ship here.

The facade admission test also exposed a prior issue. Snapshot facts and
history are host/runtime-only in practice, but root-facade `FactStore` methods
transitively expose them. A root owner cannot launder a host-only signature
type into the shared facade. The snapshot persistence seam must be settled
before placing `SnapshotTime`; this is a focused follow-up, not an exception to
the admission test and not part of handle identity.

## 5. Baseline evidence

Before implementation, the current debug binary at `d5088c7` was run against
the anneal, Herald, and Murail design corpora with:

```text
duplicate_handle(h, n) :=
  *handle{id: h},
  n = Count{ (source, native_id) :
    *handle{id: h, source: source, native_id: native_id}
  }.
? duplicate_handle(h, n), n > 1.
```

All three returned zero rows. This does not prove `*handle` is empty; it proves
the evaluated relation contained no public id claimed by multiple distinct
source/native identities. The command, query, binary commit, and corpus set
travel with the number so the result is reproducible.

## 6. Gates

### Type and store contract

- empty `HandleId` construction and deserialization fail;
- nonempty values round-trip through serde with byte-identical JSON strings;
- adapter and layered-host facade fixtures import the same canonical root type;
- a compile-fail example demonstrates that another string newtype cannot be
  passed where `HandleId` is required;
- same textual id in different corpora is valid;
- duplicate id within one corpus and across sources is rejected;
- full-snapshot replacement and delta native-id replacement retain the id
  legally;
- full-snapshot and delta collision failures are atomic and preserve prior
  relations, visibility, and generation state.

### Existing-corpus safety

After implementation, anneal, Herald, and Murail must each complete a real
status/evaluation load without the new store error. The pre-change duplicate
query is repeated against the same corpus revisions. A failure is a corpus or
CR-D41 finding and must be surfaced; the gate is not relaxed.

### Compatibility and architecture

- transparent fact, history, relation-row, cache, and JSON serialization is
  byte-identical;
- status, check, schema, search, read, context, handle, impact, lineage, eval,
  describe, explain, stderr, and exit artifacts are byte-identical for valid
  corpora;
- `cargo-public-api 0.52.0` is recorded before and after with
  `cargo public-api -p anneal-core -sss --color never`;
- the intended root-facade additions are `HandleId` plus its minimal
  construction/error surface and the structured duplicate-claim conflict
  carried by the new `StoreError` variant;
- `scripts/check-arch.py`, rustdoc with warnings denied, and `just check` pass.

The percentage change in public API size is evidence, not the gate. The gate is
one canonical type at the admitted altitude, no duplicate representation
authority, and CR-D41 enforced where the full corpus state is known.

## 7. Implementation findings

The migration exposed four representations that looked handle-shaped but did
not carry the same contract:

1. `HandleFact.file` is provenance, not universally a handle id. External code
   handles legitimately carry an empty file. Search indexing therefore treats
   a nonempty, different file as a possible parent handle and preserves empty
   provenance as absence.
2. Parent-cluster scoring constructed a synthetic `SearchHit` with empty
   corpus, source, and handle fields solely to call the default calibrator.
   `HandleId` made that fabricated identity unconstructible. The cluster now
   computes its already-defined score directly; byte-identity gates protect
   the result.
3. Store validation initially recreated full/delta replacement semantics
   separately from mutation. One `ValidatedBatch` replacement plan now drives
   both the prospective uniqueness check and actual relation/visibility
   removal, so the two states cannot drift.
4. Named-row and tuple-row graph ingestion initially repeated graph mutation
   after parsing. Both formats now project into shared typed handle, edge, and
   content insertion methods. Content and search indexes retain `HandleId`
   after their dynamic-row boundary rather than reparsing typed results.

The post-implementation API measurement uses:

```text
cargo public-api -p anneal-core -sss --color never
```

with `cargo-public-api 0.52.0`. The pre-change projection at `d5088c7` is
1,541 lines with SHA-256
`9be464b487d8617368c1a331403509df6a29f7718007a508a7433c3e030f8f68`.
The candidate is 1,562 lines with SHA-256
`86a9b07eb5fcf8edd4202e4193d77c521a65ee3a692701269b501c928174b167`:
a net increase of 21 lines. The diff consists of the admitted `HandleId` and
duplicate-conflict contracts plus substitutions of that type for raw strings
in existing public signatures. Unfiltered or differently simplified
projections are not comparable.
