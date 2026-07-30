---
status: locked
date: 2026-07-30
authors: [codex, claude, morgan]
purpose: >
  Places snapshot persistence at anneal-core's runtime facade and introduces a
  validated SnapshotTime that preserves exact history wire bytes. Closes the
  first known transitive-exposure violation in the public-facade admission
  rule without changing valid history, query, or rendering behavior.
depends-on:
  - 2026-05-13-corpus-runtime.md
  - 2026-07-29-anneal-core-public-api-altitude.md
  - 2026-07-29-handle-id-boundary.md
---

# Snapshot persistence facade - 2026-07-30

## 1. Finding

The public-facade admission rule says that an item belongs at the crate root
only when both consumer classes require it or it is an adapter/provider
extension contract. Admission is transitive through public signatures: a
legitimate root type cannot launder a host-only type into the shared facade.

`FactStore` legitimately belongs at the root because adapters populate it.
Its snapshot methods violate the transitive rule:

```text
FactStore::snapshots()                -> &[SnapshotFact]
FactStore::replace_snapshots(...)     -> Vec<SnapshotFact>
FactStore::replace_snapshot_history() -> &SnapshotHistory
```

`SnapshotFact`, `SnapshotHistory`, and repository-local history persistence
are consumed only by query hosts and runtime evaluation. Adapters neither emit
nor interpret them.

The leak also runs in the other direction. Root-level
`SnapshotEntry::new` and `SnapshotEntry::with_prelude_hash` accept
`runtime::PreludeSet`. The root history contract therefore already depends on
a host-facade type. The seam is not shared substrate in either direction.

This falsifies the original public-altitude catalog row that grouped `project`
and `history` together at the root. The correction is recorded rather than
silently rewriting the earlier model.

## 2. Evidence and governing constraints

The exact baseline command is:

```text
cargo public-api -p anneal-core -sss --color never
cargo-public-api 0.52.0
```

At commit `6fc80b5` it reports 1,562 simplified public API lines. The relevant
surface includes three public `FactStore` snapshot methods and root exports for
`SnapshotFact`, snapshot entry/history/error types, and persistence functions.

Two research-graph claims constrain the correction:

- **Unnecessary data abstraction introduces subjectivity and data hiding that
  undermines referential transparency.** `SnapshotTime` must enforce a real
  accepted-input invariant; a named wrapper around an arbitrary string would
  not earn its cost.
- **Protected entry points allow capability transfer across protection sphere
  boundaries without exposing the callee's internal objects.** Runtime free
  functions operate on the shared store without making host-only persistence
  look like an inherent root-store capability.

The project compatibility constraint is harder than ordinary migration
politeness. `anneal-dev` and released `anneal` run beside each other on the
maintainer's machine and share history state. Either binary must continue to
read history written by the other.

## 3. Facade decision

### 3.1 Root facade

The crate root retains `FactStore`, `CorpusId`, and `StoreError`. It no longer
exports:

- `SnapshotFact`;
- snapshot entry, history, warning, error, or append-outcome types;
- repository history read, append, capped-append, or path functions.

`FactStore` retains snapshot vectors and mutation machinery privately because
the runtime database is still built from one store. Its snapshot access and
replacement methods become `pub(crate)`.

### 3.2 Runtime facade

`anneal_core::runtime` is the sole supported facade for:

- `SnapshotTime` and `SnapshotTimeError`;
- `SnapshotFact`;
- `SnapshotEntry`, `SnapshotEntryFact`, and `SnapshotHistory`;
- `HistoryWarning`, `HistoryError`, and `SnapshotAppendOutcome`;
- repository history read, append, capped-append, and path operations.

The runtime facade exposes three thin free functions over the shared store:

```text
snapshot_facts(&FactStore) -> &[SnapshotFact]

replace_snapshot_facts(
  &mut FactStore,
  &CorpusId,
  Vec<SnapshotFact>
) -> Result<(), StoreError>

replace_snapshot_history(
  &mut FactStore,
  &SnapshotHistory
)
```

These functions delegate to crate-private store machinery. They contain no
analysis or persistence policy.

An extension trait is rejected even if sealed. `store.snapshots()` would still
teach callers and IDE completion that snapshot persistence is an inherent
property of the root-shared store. `runtime::snapshot_facts(&store)` states
the truthful direction: the runtime facade is observing or mutating runtime
state held by the shared store.

## 4. `SnapshotTime`

### 4.1 Accepted domain

`SnapshotTime::parse` accepts exactly the grammar already accepted by snapshot
history:

- an exact `YYYY-MM-DD` date; or
- the current RFC3339 subset accepted by `snapshot_days_since_epoch`.

No broader parser, offset normalization, or UTC conversion belongs in this
change. In particular, the cached day is the calendar date encoded in the
wire value. It is not a claim that two differently offset timestamps denote
the same UTC day.

### 4.2 Representation

```text
SnapshotTime(Arc<SnapshotTimeInner>)
  SnapshotTimeInner.wire: Arc<str>
  SnapshotTimeInner.day_since_epoch: i64
```

The wire text is the sole authority. `day_since_epoch` is a derived cache with
no independent construction path. Fields remain private, parsing computes
both values atomically, and no `from_parts` constructor may exist. The cache
therefore cannot disagree with the text it describes.

The outer `Arc` lets every `SnapshotFact` derived from one entry share both
the wire allocation and the derived day. The value occupies one pointer on
the supported release targets, smaller than the current `String`.

The public operations are deliberately small:

- `parse`;
- `as_str`;
- `day_since_epoch`;
- `Display` and `AsRef<str>`;
- `TryFrom<String>` and `TryFrom<&str>`;
- string-shaped `Serialize` and validating `Deserialize`.

`Ord` and `PartialOrd` are deliberately absent. Existing snapshot selection
uses the raw wire text within day and snapshot ties. A semantic-looking total
order would invite a future caller to change that behavior accidentally. The
type definition carries this reason beside the missing derive.

### 4.3 Typed entries and facts

`SnapshotEntry.at` and `SnapshotFact.at` become `SnapshotTime`.
Constructors require a `SnapshotTime`; they do not accept arbitrary strings
and validate later. Invalid timestamps are therefore unconstructible through
the public API.

Snapshot id and fact-key validity remain append/read invariants because they
are distinct from time syntax and still have independent public fields.

Serialization emits only the original wire string. Deserialization validates
that string and reconstructs the cache. Date-only values remain date-only;
offset spelling, fractional seconds, and `Z` remain byte-identical.

## 5. History behavior

History reading remains line-recoverable:

- malformed JSON becomes a `HistoryWarning`;
- a syntactically valid entry with invalid snapshot time becomes a
  `HistoryWarning`;
- one invalid line does not poison later entries;
- recognized legacy lines remain preserved during capped rewrites.

Valid history entries serialize byte-for-byte as before. There is no format
version, migration, or normalized rewrite.

The compatibility gate runs in both directions:

1. released 0.22.0 writes history that the candidate reads;
2. the candidate writes history that released 0.22.0 reads.

The second direction is load-bearing because it catches a candidate that can
read old strings but writes normalized or structurally different values.

## 6. Selection and ordering

The type migration does not change snapshot meaning:

- relation rows receive `SnapshotTime::as_str()`;
- stored snapshot ordering retains its existing fields and raw-wire lexical
  comparison;
- `snapshot:last` retains its existing latest-candidate behavior;
- named snapshot selection is unchanged;
- nearest-day ties still choose the later snapshot;
- same-day status history still uses raw wire, then status, as tie-breakers.

The cached day does not license chronological offset reinterpretation. Such a
change would be a separate semantic decision with a measured expected-delta
set.

## 7. Boundary tests

The existing adapter fixture remains root-only and must not gain a runtime
import.

The existing pure host fixture remains runtime-only.

A layered snapshot-host fixture imports `FactStore` and `CorpusId` from the
root, imports all snapshot contracts and operations from `runtime`, loads
history into the store, and evaluates the resulting `*snapshot` rows. If it
needs an implementation path or a root snapshot export, the facade placement
is wrong; the fixture must not be widened merely to compile.

Architecture fitness rejects:

- root exports of host-only snapshot contracts;
- any public `FactStore` signature that transitively exposes a snapshot
  contract;
- imports through private history, facts, store, or time modules.

Evaluation continues to seed its graph index through the one visibility-filtered
tuple path. `SnapshotTime` does not create a second typed indexing route merely
to consume its cached day; the cache remains part of the validated runtime
contract, not an excuse for duplicate ingestion authority.

## 8. Gates

### Type and wire contract

- exact dates and the existing RFC3339 forms parse;
- valid date prefixes with suffixes, invalid dates, invalid times, and invalid
  zones fail;
- `day_since_epoch` is derived only by parsing;
- string serde preserves exact bytes for date-only, `Z`, fractional, and
  offset forms;
- cloning shares the complete wire-and-day allocation;
- `SnapshotTime` occupies one pointer on supported targets;
- ordering traits remain absent and the code comment names why.

### Persistence and evaluation

- append/read and capped rewrite preserve valid history bytes;
- malformed and invalid-time lines remain recoverable warnings;
- legacy lines remain preserved;
- 0.22.0-to-candidate and candidate-to-0.22.0 reads both succeed;
- date and RFC3339 snapshot selection is unchanged;
- same-day raw-wire ordering is unchanged;
- `snapshot:last`, named snapshot, nearest-day, and later-snapshot tie
  semantics are unchanged.

### Architecture and outputs

- the exact `cargo-public-api` command records before/after line counts and
  path ledger;
- root snapshot exports and methods are absent;
- runtime owns one canonical path for every snapshot contract;
- all three facade fixtures compile and run;
- status, check, schema, search, read, context, handle, impact, lineage, eval,
  describe, explain, JSON, NDJSON, stderr, and exit artifacts are
  byte-identical on the same document state;
- `just check` passes outside a git worktree.

## 9. Expected deltas

User-visible CLI output and valid history JSON have an empty delta set.

The public Rust API intentionally changes:

- snapshot contracts move from the root to `runtime`;
- public inherent snapshot methods leave `FactStore`;
- `SnapshotTime` and `SnapshotTimeError` enter `runtime`;
- three runtime free functions become the supported store boundary.

There are no external consumers and `anneal-core` is not publishable. This is
an internal workspace migration with a recorded path ledger, not a deprecation
cycle.

## 10. Implementation evidence

At implementation commit preparation:

- `cargo public-api -p anneal-core -sss --color never` with
  `cargo-public-api 0.52.0` moves from 1,562 to 1,580 lines. The net +18 admits
  the runtime snapshot contract while removing the root paths and inherent
  store methods.
- A 30-command battery over help, schema, describe, status, check, search,
  read, context, handle, impact, lineage, eval, and explain produced 90
  byte-identical stdout, stderr, and exit artifacts against commit `6fc80b5`
  on the same document state.
- History written by released 0.22.0 is readable by the candidate, and history
  written by the candidate is readable by released 0.22.0.
- The post-change review rejected a short-lived typed snapshot indexing path:
  it bypassed tuple visibility and duplicated snapshot interpretation. The
  final implementation keeps one tuple-indexing authority and shares the
  timestamp wire-and-day representation behind one pointer.
- `just check` passes, including four facade fixtures and the compile-fail
  assertion that `SnapshotTime` does not implement `Ord`.
