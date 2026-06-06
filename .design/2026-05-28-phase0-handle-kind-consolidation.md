---
status: draft
updated: 2026-05-28
author: claude (Phase 0 design — handle-kind consolidation for v0.14+)
reviewers: codex (independent review pending), large-corpus reviewer (substrate cross-corpus check pending)
depends-on:
  - 2026-05-13-corpus-runtime.md
  - 2026-05-26-surface-evolution-framework.md
  - 2026-05-28-v014-calibration-design.md
bd: anneal-gbuz
description: >
  Phase 0 design conversation for v0.14+ handle-kind consolidation.
  Six required sections per codex's outline. Working assumption from
  corpus evidence: 5 → 4 (fold section, keep version, keep external).
  Outcome decides whether substrate change ships in v0.14 (no break),
  v0.15 (break with migration), or v1.0 (major).
---

# Phase 0 — Handle-Kind Consolidation — 2026-05-28

## §1 Current uses inventory

### §1.1 The substrate enum

```rust
// crates/anneal-legacy/src/handle.rs
pub enum HandleKind {
    File(PathBuf),
    Section { parent: NodeId, heading: String },
    Label { prefix: String, number: u32 },
    Version { artifact: NodeId, version: String },
    External { url: String },
}
```

Five variants, declared in CR-D8 master spec §10:
> *handle.kind ∈ {file, section, label, version, external}

### §1.2 Match-arm distribution across the codebase

Total `HandleKind::` match arms in legacy: **99**. By variant:

| Variant | Match arms | Notes |
|---|---|---|
| `File` | 17 | Path operations, area derivation, parse pipeline |
| `Label` | 17 | Namespace resolution, orphan checks, S001/S005 |
| `Version` | 8 | Supersedes traversal, version chain, S001 orphan |
| `External` | 7 | URL recording, external reference handling |
| `Section` | 6 | Heading capture, ref recording, area exclude |

`Section` has the **lowest match-arm count** (6) despite being one
of the highest-cardinality kinds in stored facts.

### §1.3 Prelude `kind:` usage in `.dl` files

Direct filtering by `*handle{kind: "..."}` in prelude rules:

| Kind | Filter sites | Rule purpose |
|---|---|---|
| `"file"` | 9 | freshness_decay, missing_meta, area_file_count, etc. |
| `"label"` | 4 | orphan_label, namespace_label, S001, S005 |
| `"version"` | 1 | s001_orphaned (version-specific orphan rule) |
| `"section"` | 0 | sections are never filtered IN to a rule body |
| `"external"` | 0 | externals are never filtered IN to a rule body |

**Sections and externals are never explicitly addressed by prelude
rules.** They only appear via implicit `*handle{}` joins (no kind
filter), which is where the noise problem manifests.

### §1.4 Diagnostic-rule kind dependencies

| Rule | Depends on which kinds |
|---|---|
| E001 broken_reference | edges only — kind-agnostic |
| W001 stale_reference | source + target both pre-terminal — kind-agnostic but typically file-to-file or file-to-label |
| W002 implausible_ref | file only |
| W003 missing_frontmatter_file | file only |
| W004 stale_reference variant | edges — kind-agnostic |
| I001 section_refs (count) | section-related metadata, NOT section *handle* rows |
| I002 namespace inventory | label |
| S001 orphaned_handle | label OR version |
| S003 pipeline_stall | status counts, kind-agnostic |
| S004 abandoned_namespace | label |
| S005 concern_group_candidate | label |

**Only S001 distinguishes between label and version.** S001 fires
when a label has zero incoming citations, OR when a version has
zero incoming citations AND has at least one outgoing Supersedes.
Folding version into label would collapse two semantically distinct
S001 cases.

## §2 Corpus evidence

Measured on `.design` (anneal's own design corpus) and
`/path/to/large-corpus/.design` (the test corpus, ~13MB markdown).

### §2.1 Kind cardinality

| Corpus | file | section | label | version | external | total *handle |
|---|---|---|---|---|---|---|
| `.design` | 29 | **1,033** | 25 | 1 | 0 | 1,088 |
| `large-corpus/.design` | 429 | **13,415** | 692 | 42 | 0 | 14,578 |

Section is **94.9%** of all handles on `.design` and **92.0%** of
all handles on large-corpus.

### §2.2 Edge participation

| Corpus | Section handles with ≥1 edge | Version handles with ≥1 Supersedes |
|---|---|---|
| `large-corpus` | **0 of 13,415** | 28 of 42 (66%) |
| `.design` | **0 of 1,033** | (1 version handle; trivial) |

**Sections have zero edge participation on the test corpus.** Every
single one of the 13,415 section handles on large-corpus has zero
outgoing and zero incoming edges. They are referenced *implicitly*
through `*handle{id: h, file: file}` joins by file path, but
nothing in the substrate addresses them as graph nodes.

Version handles participate in real `Supersedes` chains (28 edges
across 42 versions on large-corpus — every long-running spec series).

### §2.3 Content participation

| Corpus | Section handles with `*content{handle: h}` rows | File handles with content |
|---|---|---|
| `large-corpus` | **0 of 13,415** | 429 of 429 |
| `.design` | **0 of 1,033** | 29 of 29 |

**Sections hold no content.** Content is stored against file
handles only. Sections inherit file-level content via the `file`
field but are not addressable for `read` or `search`.

### §2.4 Summary participation

| Corpus | Section handles with non-empty summary | File handles with non-empty summary |
|---|---|---|
| `large-corpus` | **0 of 13,415** | 429 (most non-empty) |
| `.design` | **0 of 1,033** | 29 (most non-empty) |

**Section summaries are uniformly empty.** No teaching signal there.

### §2.5 The bottom line

Across both real corpora, section handles are:
- 92-95% of all `*handle` rows
- Have zero edges (in or out)
- Have zero content
- Have empty summaries
- Inherit file metadata through implicit joins
- Are never explicitly addressed by prelude rules
- Show up as 30:1 noise in temporal queries like `changed_within`

Version handles are:
- 42 of 14,578 on large-corpus (<0.3% of handles)
- Participate in real Supersedes chains (28 of 42 are in a chain)
- Have semantic identity as artifacts in a version series
- Are addressed by S001 with version-specific orphan logic

External handles are:
- Zero on both corpora (anneal-md does not emit external)
- Reserved for future adapters that integrate external refs

## §3 Candidate models

Four candidates evaluated, ranked by working-assumption preference.

### §3.1 Candidate A: 5 → 4 (fold section, keep version, keep external)

**Working assumption from corpus evidence.**

```
file       (file handles — load-bearing)
label      (cross-reference / namespace handles)
version    (versioned artifacts with Supersedes chains)
external   (other-adapter URLs and refs — kept even if unused now)
```

Section becomes structural metadata, not a *handle* kind:
- Sections become `*span{}` rows (this relation already exists for
  citable regions)
- Heading metadata moves to `*span.summary` and `*span.start_line` /
  `*span.end_line`
- Section refs in markdown body remain queryable via `*edge` to the
  parent file with line metadata (already the case)
- `search` and `read` already narrow by `span_id` — no surface
  change for content retrieval

**Pros:**
- Removes 92-95% of handle rows on real corpora
- Restores `changed_within(h, days)` to file-cardinality
- Closes the section-as-noise problem in every implicit `*handle{}`
  join
- Source trait change is minimal (adapter stops emitting section
  handles; emits heading spans instead via the existing `*span`
  shape)
- Prelude rules require no kind-filter additions (sections were
  never explicitly filtered IN)

**Cons:**
- Breaks any user query that uses `*handle{kind: "section"}` —
  needs migration path
- Snapshot history contains old section handle ids; needs
  compatibility decision (§4.2)
- Heading-as-span requires adapter work to emit stable span ids
  for headings (currently *span emits only full-file and
  label-definition spans)
- Externals stay declared even though no adapter emits them yet —
  philosophical question about declaring unused kinds

**Acceptance:**
- File and label cardinality unchanged on both corpora
- Section cardinality goes to zero
- `*span` cardinality rises by approximately the former section
  count, with `*span.summary` carrying the heading text and
  `*span.start_line` / `*span.end_line` covering the heading's
  scope
- Search and read can narrow to a heading via span_id without
  reaching for a section handle id

### §3.2 Candidate B: 5 → 4 (fold external, keep section)

Status: rejected.

External is the only zero-cardinality kind today, but:
- Other adapters (`anneal-mdx`, `anneal-code`, `anneal-host`,
  future `anneal-issues`) plausibly emit external refs
- Removing external means future adapters re-introduce it
- The noise problem (section explosion) is not addressed
- Same cost as section fold, less benefit

### §3.3 Candidate C: 5 → 3 (fold section, fold version into *meta)

```
file
label
external
```

Version becomes `*meta{handle: file_id, key: "version", value: "17"}`
on the file handle.

**Pros:**
- Even tighter substrate
- Versioned artifacts collapse into "file with a version label"
- Cleaner conceptual model for spec series

**Cons:**
- Loses typed `Supersedes` semantics — `*edge{from: file_id_v17,
  to: file_id_v16, kind: "Supersedes"}` becomes a *meta query
  rather than a graph relationship
- S001 version-orphan logic needs reformulation
- Large Corpus's 42 version handles in 28 Supersedes chains all need
  query rewriting
- Other corpora with formal spec series (mathematical papers,
  RFCs) lose first-class version semantics

**Status:** plausible but breaks query-shape compatibility for a
real semantic concept. Defer pending evidence the simplification
is worth the migration.

### §3.4 Candidate D: 5 → 3 (fold version into label)

```
file
label   (includes versioned artifacts as a label namespace)
external
```

Status: rejected.

Labels mean "cross-reference / obligation namespace." Versions are
artifacts. Folding versions into labels conflates two distinct
concepts. Worse than candidate C.

### §3.5 Working selection: Candidate A

Lean 5 → 4 (fold section, keep version, keep external). Evidence-
backed by corpus measurement. Lowest migration cost. Closes the
noise problem. Preserves real semantic distinctions.

## §4 Migration plan

### §4.1 Adapter side (anneal-md)

1. **Stop emitting section *handle facts.** The parser currently
   emits one *handle per heading (kind=Section). Replace with
   *span emission.
2. **Emit heading spans into *span.** Each heading becomes a
   `*span{id, handle: file_id, start_line, end_line, summary:
   heading_text}` row. Span id format proposal:
   `file_path#heading-slug-line-N` (stable, sortable).
3. **Section refs in markdown body** continue to record as
   `*edge{from: file, to: target_file, kind: Cites, file, line}`
   with the section ref captured in evidence metadata. No change
   to edge semantics.

### §4.2 Snapshot history compatibility

Snapshot history (`*snapshot{}`) contains handle ids and kinds
from previous runs. Three options:

**Option a: Tolerate old kinds with warning.**
- `at("snapshot:last")` queries that match `kind = "section"`
  return rows from history but emit a warning ("snapshot contains
  retired kind 'section' from before v0.14")
- New queries against current state don't see section handles

**Option b: Reset snapshot history on v0.14.0 upgrade.**
- First `anneal status` after upgrade clears existing snapshot
  history and starts fresh
- Loses history, gains clean substrate

**Option c: Translate snapshot history at load.**
- On load, rewrite snapshot rows: drop section handle rows,
  preserve file/label/version/external rows with their original
  status
- Most preserving; most engineering

**Recommendation: Option a.** History preservation matters for
trend analysis; warning makes the transition explicit without
silently dropping data.

### §4.3 Eval query compatibility

Queries using `*handle{kind: "section"}` break at static analysis
(no rows match). Recovery story:

- Add a runtime warning specifically for `kind = "section"` query
  patterns: "the section kind was retired in v0.14; use *span{}
  to query heading spans instead"
- Update describe runtime examples that mention section handles
- README and SKILL.md teaching update

### §4.4 CLI surface (no change)

The 9 visible commands and 2 hidden aliases are unaffected. The
`handle` command continues to work on file, label, version,
external. Asking `anneal handle "file.md#section-slug"` would
either:

**Option α: 404 with recovery.** "Section handles were retired in
v0.14; for heading content, use `anneal read file.md#section-slug`
or `anneal -e '? *span{handle: \"file.md\", summary: heading}.'`"

**Option β: Auto-route.** Detect the `file.md#heading-slug` shape
and route to `*span` lookup automatically, returning the heading
span's content.

**Recommendation: Option α** for v0.14; consider Option β for v0.15
if cold-agent evidence shows agents reach for the old shape.

### §4.5 Docs and teaching

- README "Work the Convergence Frontier" section reviewed for
  section references
- skills/anneal/SKILL.md reviewed
- describe runtime: section subsection removed or rewritten
- CHANGELOG v0.14 BehaviorChange section explicit about the kind
  cardinality change

## §5 Acceptance tests

For Candidate A landing, the following must hold:

### §5.1 Substrate-level

- [ ] `*handle{kind: k}` returns rows only with k ∈ {file, label,
      version, external}
- [ ] Section cardinality goes to zero on `.design` and `large-corpus`
- [ ] File cardinality unchanged on both corpora
- [ ] Label cardinality unchanged on both corpora
- [ ] Version cardinality unchanged on large-corpus (42)
- [ ] `*span` cardinality rises by approximately the former
      section count on both corpora

### §5.2 Prelude-level

- [ ] `area_health` continues to grade every area (no regression
      from v0.13.1 fix)
- [ ] `S001 orphaned_handle` continues to fire for label-orphans
      and version-orphans (no regression)
- [ ] `freshness_decay` no longer joins to section handles
- [ ] `missing_frontmatter_file` cardinality unchanged
- [ ] `frontier`, `blocker`, `area_frontier` row counts within 5%
      of pre-migration values on `.design` and large-corpus

### §5.3 Cold-agent smoke

- [ ] `anneal --root large-corpus/.design status` text output renders
      with similar shape (broken/blocked/work/advancing) and
      similar handle counts in each section
- [ ] `anneal --root large-corpus/.design -e '? changed_within(h, 7).'`
      returns file-cardinality results (~10-20 rows, not ~165)
- [ ] `anneal --root large-corpus/.design context "convergence"` returns
      similar quality hits
- [ ] `anneal --root large-corpus/.design handle <some file>` works
- [ ] `anneal --root large-corpus/.design handle <some file>#some-heading`
      returns Option α recovery message naming the *span path

### §5.4 Heading-content retrieval

- [ ] `anneal -e '? *span{handle: "file.md", summary: heading}.'`
      returns heading spans
- [ ] `anneal read "file.md" --span-id "file.md#heading-slug-line-N"`
      returns the heading's body (CR-D14 read_full capability still
      applies)
- [ ] Search results that previously pointed at section handles
      now point at span_ids with same readable text

### §5.5 Migration recovery

- [ ] `anneal -e '? *handle{kind: "section"}.'` returns 0 rows
      with a warning naming the *span recovery path
- [ ] Snapshot history queries with section handles emit a
      warning and skip those rows (per §4.2 Option a)
- [ ] CHANGELOG v0.14 explicit about the change
- [ ] README, SKILL.md, describe runtime all updated

## §6 Review section

### Claude (author, 2026-05-28)

Working assumption is Candidate A (5 → 4: fold section, keep
version, keep external). Evidence is strong: 13,415 section handles
with zero edge or content participation on large-corpus; same shape on
`.design`. The kind never gets explicitly filtered IN by a prelude
rule, so removing it is purely subtractive at the rule level.

The biggest open question for me is whether this ships in v0.14
or v0.15. If snapshot-history compatibility (§4.2 Option a) holds
clean and `*span` heading emission lands cleanly, this is an
additive substrate refinement (existing user queries break, but
they break LOUDLY with teaching recovery — same shape as the
v0.13 retired-command pattern). That argues for v0.14.

If the heading-span migration uncovers complications (span id
stability under file edits, summary extraction performance, etc.),
this splits to v0.15.

### Codex (pending independent review)

Awaiting review. Specific asks:
- Push back on Candidate A vs C (version as *meta). I rejected C
  on Supersedes-typing grounds, but it deserves a deeper look at
  whether Supersedes can live cleanly without version-as-kind.
- Confirm the migration story is honest, especially §4.2 Option a
  snapshot tolerance.
- Verify the §5 acceptance tests cover the cold-agent contract.
- Evidence push-back: any corpus shape we haven't seen (very old
  Issues-style, code-as-corpus) that would change the calculus?

### Large-corpus reviewer (pending cross-corpus check)

Awaiting review. Large-corpus reviewer's earlier proposal was 5 → 3 (fold
section into file as granularity; fold version/external into
"external references"). Specific asks:
- Is 5 → 4 the right halfway point, or does the large-corpus use case
  pull harder for 5 → 3?
- Cross-corpus migration story: does large-corpus's specs/papers
  structure migrate cleanly under Candidate A?
- Heading-span emission specifics: what id format does large-corpus
  Claude prefer for stable section addressing?

### project owner decision (pending)

Hold for Codex and large-corpus reviewer reviews. After convergence, the
decision is binary: ship Candidate A in v0.14 (no migration
break) or v0.15 (with explicit BehaviorChange).
