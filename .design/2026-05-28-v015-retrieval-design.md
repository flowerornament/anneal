---
status: converged
updated: 2026-05-28
author: claude (post-host-corpus reviewer cold-agent feedback + v0.14 calibration shipped)
reviewers: codex (independent review converged 2026-05-28), host-corpus reviewer (cross-corpus validation converged 2026-05-28)
depends-on:
  - 2026-05-13-corpus-runtime.md
  - 2026-05-26-surface-evolution-framework.md
  - 2026-05-28-v014-calibration-design.md
  - 2026-05-28-phase0-handle-kind-consolidation.md
description: >
  v0.15 design as the "retrieval release." Folds host-corpus reviewer's
  cold-agent friction findings on retrieval quality with the Phase 0
  substrate work (sections → spans) and proposes a coherent forward
  shape that makes anneal preferentially better than grep + file
  navigation for the core "find evidence for X" agent workflow.
---

# v0.15 — Retrieval Release Design

## Narrative arc

```
v0.13.0:  "anneal becomes the language it has always claimed to be."
v0.13.1:  "anneal predicates match their describe-card promises."
v0.14.0:  "anneal calibrates the signal, simplifies the substrate,
           and sharpens retrieval."
           (bundled release — calibration + Phase 0 substrate +
            retrieval + teaching all ship together)
```

NOTE (scope pivot, 2026-05-28). This doc was originally written for
v0.15. project owner's bundled-release call folds calibration work,
Phase 0 handle-kind consolidation, retrieval, and teaching into one
v0.14.0 release. "v0.15" remains a useful internal label for the
retrieval theme of the bundle; it does NOT ship as a separate tag.

v0.13 made the convergence side of the cold-agent contract sharp:
status / frontier / blocker / flow are all named, calibrated, and
teachable. The RETRIEVAL side (search / read / context / handle) is
where host-corpus reviewer's cold-agent test surfaced the unfilled promises:

> "anneal LOSES TO grep at: finding the section that defines X (no
> section-level ranking), finding text occurrences with context (no
> line numbers in Read excerpts), most-recent-relevant ranking."
>
> "What would make me reach for anneal preferentially over grep:
> Section-level search ranking with file:line excerpts (would make
> context strictly better than grep -rn). lib/lang/file:N-M edge
> extraction (would make impact analysis meaningful for our corpus)."

The convergence story can't carry the full cold-agent contract while
retrieval falls back to grep. v0.15 closes the gap.

## The retrieval problem in three frictions

Host-corpus reviewer exercised v0.14 on host-corpus-dev's ~50+ doc corpus with
mixed architecture / proposals / research / implementation specs.
Three structural retrieval issues surfaced — none of them isolated
bugs, all of them substantive design gaps.

### Friction R1. Search hits at document level, not section level

```
$ anneal context "lease protocol" --hits 5 --format=text
Hits
 1. older-design-doc-1.md  score=0.858  body-substring
 2. older-design-doc-2.md  score=0.738  body-substring
 ...
11. 2026-05-28-authority-admission-seat-sync-contract.md  score=0.940
    ← THE authoritative doc, ranked #11 below 10 less-relevant docs
```

Three sub-problems compound:

- **R1a. Score signal is too narrow.** Body-substring matches cluster
  in the 0.7-0.9 range. The ranker can't distinguish "this doc
  mentions the term in passing" from "this doc is ABOUT the term."
- **R1b. Hits are document-grained.** Search returns *handle{} rows
  at the file level. There's no "this is the §3 that defines the
  term" signal. Agent has to grep the doc anyway.
- **R1c. Read excerpt is doc-intro, not relevant section.** When
  search returns a doc, the Read excerpt is the first N tokens of
  the doc — the abstract or §0. The section that actually defines
  the term is invisible.

The cold-agent task "find the section that defines X" requires
opening each ranked doc and grepping inside. At that point grep
wins by skipping the rank.

### Friction R2. Code-pointing references are silently dropped

```
$ anneal handle 2026-05-28-authority-admission-seat-sync-contract.md --impact
... 0 code-referencing edges shown ...

[the doc body actually contains 39 references like
 `lib/host-corpus/admission.rs:142-167`,
 `lib/host-corpus/seat_sync.ex:200-250`,
 etc. — none captured]
```

The markdown parser recognizes:
- Frontmatter `depends-on:` blocks → typed edges
- Markdown links `[text](target)` → Cites edges
- Section refs `§4.2` → diagnostic metadata
- Label patterns `OQ-42`, `FM-17` → Cites edges to label handles

The markdown parser does NOT recognize:
- File-path patterns `lib/host-corpus/admission.rs:142-167`
- Bare path mentions `crates/anneal-core/src/runtime/eval.rs`
- Multi-line ranges with column hints

These are exactly the kind of references that make a design doc
trace into the code it specifies. Without capturing them as edges,
`handle --impact` returns the typed-graph impact (which is real
but partial) and misses the code-side blast radius entirely.

### Friction R3. handle command shows typed edges, not inline body references

Host-corpus reviewer:
> "anneal handle <doc> to surface inline body cross-refs (only shows
> typed labels / frontmatter edges)"

The handle command renders:
- Outgoing typed edges (Cites, DependsOn, Supersedes, Verifies, Discharges)
- Incoming typed edges (same kinds)
- Impact (configured reverse-dep walk)

It does NOT render:
- Inline body mentions that ARE captured as edges (e.g., `[See FM-17](FM-17.md)` IS a Cites edge but does it surface in handle output?)
- Cross-references the parser catches but the human view omits

Investigation needed: is host-corpus reviewer's "only shows typed labels /
frontmatter edges" observation about (a) edges that exist but aren't
rendered, or (b) edges that aren't being captured in the first place?
Different diagnoses, different fixes.

## Design space — three independent decisions

The retrieval release has three substantive design decisions. Each
has multiple candidates, evaluated by cold-agent utility.

### D1. Search granularity — section vs document

**Candidate D1a: Search hits at *span granularity (the post-Phase-0 path).**

If Phase 0 lands the section → *span fold (lean 5 → 4), search
naturally returns span-level rows:

```
search(query, handle, span_id, score, reason, field, low_confidence)
                       ▲
                       this becomes meaningful: which span matched
```

The existing search signature already has span_id. Today it's mostly
the full-file span; under D1a it becomes the heading span where the
match landed.

**Pros:**
- Falls out naturally from Phase 0 substrate work
- Read can take span_id and return the section's body, not doc-intro
- Section-level ranking (one match per section) lets the scorer
  differentiate "abundant mentions in §3" from "one mention in §17"
- Matches the cold-agent mental model: "find the section that
  defines X"

**Cons:**
- Requires Phase 0 implementation first
- Spans need stable ids that survive file edits (already in Phase 0
  acceptance)
- Search ranking weights need recalibration for span-granular hits

**Acceptance under D1a:**
- `anneal context "X"` returns hits with section-level granularity
- Read excerpts are the matched span's body, not file-intro
- A query like `anneal context "lease protocol"` ranks the
  authoritative doc's §2 above older docs' §17

**Candidate D1b: Span-level search WITHOUT Phase 0 (interim).**

If v0.15 can't wait for Phase 0, anneal-md adapter could emit a
*span row per heading independent of the handle-kind decision.
Sections-as-handles continue to exist; spans get added in parallel.

**Pros:**
- Decoupled from Phase 0 timing
- Same retrieval benefits

**Cons:**
- Substrate duplication (sections AND span-per-heading rows)
- Future Phase 0 fold becomes more painful (delete sections, keep
  spans)
- Architectural debt

**Working selection: D1a, with Phase 0 sequenced first.**
Phase 0 is small (5 → 4, additive at substrate level) and the
correct architectural foundation. Don't add interim complexity.

### D2. Code-reference edge extraction

**Candidate D2a: Extend anneal-md to recognize code-path patterns.**

Add to the markdown body parser:
- File-path regex: `(crates|lib|src|app|test|priv|native)/[^\s)`]+`
- Optional line/range annotation: `:N`, `:N-M`, `:N:C`
- Also catch backtick-quoted code-path refs (per host-corpus reviewer
  evidence: lsp26 contract uses backticks heavily)
- Emit as **`*edge{from: doc, to: code_external_handle, kind: "Cites",
  file: doc_path, line: line_in_doc}`** — use existing Cites edge
  kind, NOT a new CodeRef kind
- Code target lives in an external handle whose target identity goes
  in metadata (not in `*handle.file`/`line` discovery-location fields)

**Candidate D2b: New code handle kind + new CodeRef edge kind.**

The over-typed: introduce `*handle{kind: "code"}` for code locations
with `(path, start_line, end_line)` and `*edge{kind: "CodeRef"}` for
references. Markdown parser emits both.

**Pros (D2b vs D2a):**
- Typed substrate for code refs (queryable, joinable)
- handle --impact for code-side: "which design docs reference this code?"
- Pairs naturally with future `anneal-code` adapter (which would emit
  code handles directly from source files)
- Foundation for design ↔ code traceability

**Cons:**
- Adds substrate complexity (codex's framework says careful)
- Currently Phase 0 is REDUCING kinds; this would ADD one
- A code handle kind earns its keep when an `anneal-code` adapter
  emits code facts as a source, not when markdown merely points at
  code paths (codex convergence)
- Could fold into `external` kind with metadata (less typed but
  smaller substrate)

**Candidate D2c: Stay in `external` kind + Cites edges, add
target metadata.** (LOCKED selection.)

```
*handle{kind: "external", id: <stable code-ref id>}    ← discovery-location fields
                                                        (file, line) describe where the
                                                        external handle was first seen,
                                                        NOT the code target

*meta{handle: <external_id>, key: "external_class", value: "code"}
*meta{handle: <external_id>, key: "code_path",      value: "lib/host-corpus/admission.rs"}
*meta{handle: <external_id>, key: "code_start_line", value: "142"}
*meta{handle: <external_id>, key: "code_end_line",   value: "167"}

*edge{from: doc, to: <external_id>, kind: "Cites",
      file: doc_path, line: line_where_ref_appears_in_doc}
```

Treat code references as external refs with target metadata.
NO new kind, NO new edge type. Renderer projects the metadata
fields when displaying (e.g., shows `target=lib/host-corpus/admission.rs:142-167`).

**Pros:**
- No substrate growth (existing external kind, existing Cites edge)
- Plays nice with multi-corpus federation (external refs ARE
  external from the markdown corpus's perspective)
- Forward-compatible: when anneal-code adapter ships, it can
  promote external code refs to first-class code handles in the
  multi-corpus index
- Keeps `*handle.file`/`line` clean as DISCOVERY-LOCATION fields
  (where the handle was first observed in the corpus), not
  overloaded with target-location data

**Working selection: D2c for the v0.14.0 bundle.**
Smallest substrate cost, immediately useful (lib/lang/file:N-M refs
become queryable as external handles with target metadata), forward
compatible with future anneal-code adapter that would do the
promotion.

**D2c → D2b promotion path (forward planning).** When the
`anneal-code` adapter ships (v0.16+), external code-ref handles in
the design corpus can be promoted to first-class `*handle{kind:
"code"}` rows in the federated index. The promotion is lossless: the
metadata fields (`external_class`, `code_path`, `code_start_line`,
`code_end_line`) become the typed fields on the new `code` kind;
existing edge references update via the federation join.
Documented now so v0.14.0 doesn't lock D2c as terminal — it's the
intentional first step toward D2b under multi-corpus federation
(the **substrate-staging pattern**: design the metadata as if it
were already the schema, because in two releases it will be).

### D3. handle command — what's rendered

Three sub-decisions:

**D3a. Investigation result (host-corpus reviewer diagnosis, 2026-05-28).**

Diagnosis RESOLVED: markdown-link Cites ARE captured AND ARE rendered
by handle. Test on host-corpus-dev:
- `anneal handle research/2026-04-02-excalibur-study.md` returns 39
  captured outgoing edges including markdown-link refs with file:line
  metadata, all rendered correctly.
- `anneal handle architecture/.../authority-admission-seat-sync-contract.md`
  returns ZERO outgoing edges. The contract has 39 `lib/host-corpus/...file.ex:N-M`
  refs but ALL in backtick code-span form, not markdown links.

The renderer is doing its job. The gap is parser-side: backtick code-
style refs aren't recognized as references. D3a (render all outgoing
edges) is NOT NEEDED; the fix is THEME R2 (extend parser to recognize
backtick code-path patterns).

**D3b. Group by kind.**

Currently outgoing edges are listed flat. Grouping by edge kind
(Cites / DependsOn / Supersedes / Verifies / Discharges), plus a
display-only Code references section once R2 lands, gives the agent a
sense of the doc's reference structure at a glance. This is the actual
renderer-side improvement worth doing.

**D3c. Code-reference section.**

After R2 lands (parser recognizes backtick code-path patterns and
emits external-kind handles with target-path metadata), the handle
command gets a dedicated "Code references" section showing the
lib/lang/file:N-M refs the doc points at. Cold-agent friction R3
closes.

### D4. Search scoring — recency boost

Host-corpus reviewer's lsp26 contract (latest, authoritative) ranked #11
below 10 less-relevant older docs. The score signal doesn't account
for document freshness or status.

**Candidate D4a: Status-aware boost.**

`*handle{status: "authoritative"}` scores higher than
`*handle{status: "draft"}` for the same body-substring match.
Configurable per-corpus via `config search_boost { ... }`.

**Candidate D4b: Recency boost.**

`*handle{date: D}` where D is recent (last 30 days) gets a boost.
Tunable threshold.

**Candidate D4c: Hub boost.**

`hub(h, degree)` factor — docs with many incoming citations rank
higher (they're more referenced, more likely authoritative).

**Working selection: D4a + D4c.**
Status-aware boost is the highest-signal change (it directly addresses
the host-corpus reviewer case where the authoritative-status doc ranked
below drafts). Hub boost is cheap given we already compute hub.
Recency is risky (recency != relevance for spec corpora); defer
until per-corpus tuning exists.

### D5. Read excerpt — span vs file-offset

Today Read returns the first N tokens of the doc (or full content
up to budget). Under D1a (span-granular search), Read should:

- When given `--span-id <id>`: return the named span's body
- When given a handle without span_id: return either the full file
  (existing behavior) OR the search-matched span's body when the
  call is part of `context` workflow

Acceptance: `anneal context "lease protocol"` returns hits where
the Read excerpt IS the relevant section, not the doc intro.

## v0.14.0 bundle scope (retrieval section)

Five themes, each grounded in host-corpus reviewer's friction findings.

### THEME R1: Section-level retrieval

**A. Phase 0 implementation first** (lean 5 → 4: fold section to *span,
keep version, keep external). Per the converged Phase 0 design doc.

**B. Search emits span-granular hits.** The existing
`search(query, handle, span_id, ...)` signature already supports
this; the parser needs to populate span_id with the matched-section
span. anneal-md emits heading spans.

**B2. Span-id format: structural, not line-based.**
Per codex convergence: `file_path#heading-slug-line-N` is NOT stable
under file edits that insert lines above the heading. Lock the
following format:

```
file_path#h/<heading-slug-path>[~<occurrence>]

Examples:
  contract.md#h/lease-protocol
  contract.md#h/authority-admission/lease-protocol
  spec.md#h/observations~2          (second sibling with same slug)
```

- `#h/` prefix marks heading spans (distinguishes from future span
  kinds without ambiguity)
- Heading-slug path encodes document structure (parent / child /
  grandchild)
- Occurrence suffix `~N` disambiguates duplicate sibling headings
- Line numbers live in `*span.start_line` / `*span.end_line` for
  sorting and excerpt rendering
- Identity uses document structure; sorting uses line position

Recovery: old line-based or section-handle ids emit a warning and
route the agent to the structural form.

**C. Read takes span_id and returns the span's body.** Existing
`read(handle, budget, span_id, text, start_line, end_line, tokens)`
signature is already span-aware. Wire up: context uses search's
span_id when reading hits.

**C2. Span-aware budget behavior.** When `--span-id` is given AND
the span body exceeds the budget, truncate the span content and
emit a clear hint:

  read: showing first N tokens of span (M total); use --budget M+
        to read the full span

This is a behavior decision, not just rendering — agents need to
know they got a partial read. Refusing the read entirely would
break the context workflow; silent truncation would lose the
agent's mental model of what they saw. Hint-on-truncation is the
balance.

**D. Hit metadata shows heading hierarchy.** Each hit row gets a
heading_path field: `"§2 / Lease Protocol"` (single delimiter `/`,
unambiguous in monospace). When section numbering is absent, omit
the `§?` prefix and render just the heading title:
`"Lease Protocol"`. Multi-level: `"§2 / Authority Admission /
Lease Protocol"`.

### THEME R2: Code-reference extraction

**E. Extend anneal-md body parser to recognize code-path patterns.**
Default regex covers Rust workspace + general + Phoenix/Elixir +
Rust-NIF paths:

  `(crates|lib|src|app|test|priv|native)/[^\s)`]+(:N(-M)?(:C)?)?`

Plus, recognize code refs INSIDE backticks as well as bare or in
markdown links — host-corpus reviewer's evidence shows the lsp26 contract
uses 39 backtick-quoted file:line refs, none of which would catch
under markdown-link-only parsing.

Emit as `*edge{from: doc, to: code_external_handle, kind: "Cites",
file: doc_path, line: line_in_doc}`.

Project-level override via `config code_path_root { ... }` to add
project-specific paths (`bin/`, `web/`, `deps/`, etc.). Common
build-output paths (`_build/`, `target/`, `node_modules/`) are
excluded by default and not user-configurable (they're never
references).

**F. Emit code references as external handles.** Use external kind
(per D2c): `*handle{kind: "external", id:
"lib/host-corpus/admission.rs:142-167", ...}`. Do not overload
`*handle.file` / `*handle.line`: those remain discovery-location
fields. Target path/range live in metadata rows such as
`*meta{handle: h, key: "md.external_class", value: "code"}`,
`md.code_path`, `md.code_start_line`, and `md.code_end_line`.

**G. handle --impact for docs surfaces code-side references.** When
a doc has code-ref Cites edges to external handles, the handle command
renders them as a "Code references" section after the typed-edge
listing.

### THEME R3: Search ranking quality

**H. Status-aware ranker boost.** Configured ranker weights include
status tier: authoritative > active > draft. Per-corpus override
via `config search_boost { ... }`.

**I. Hub-degree ranker boost.** `hub(h, degree)` factor multiplies
into the search score. Highly-cited docs rank higher for the same
body match.

**J. describe context teaches the new ranking pattern.** Updated
card explains span-granular hits, heading_path metadata, ranker
boost interactions.

### THEME R4: handle command depth

**K. (RETIRED).** Host-corpus reviewer diagnosis on host-corpus-dev confirmed
markdown-link Cites are captured AND rendered today. R4-K is not
needed; the gap was parser-side (addressed by R2).

**L. Group outgoing edges by kind.** Visual hierarchy: Cites,
DependsOn, Supersedes, Verifies, Discharges, plus Cites to
external-code refs (after R2 lands). Real renderer-side improvement.

**M. Code-reference section in handle output.** After R2 lands,
handle for docs with code-ref Cites shows them in a dedicated
"Code references" section, separated from doc-level Cites for
readability.

### THEME R5: Documentation + teaching

**N. describe search + describe read + describe context updated**
for span granularity, ranker boosts, heading metadata.

**O. README + skills/anneal/SKILL.md teach the retrieval story
with EXPLICIT delineation.** "Section-level search" becomes a
first-move pattern for "find evidence for X" agent tasks. The
teaching must explicitly distinguish:

  anneal context "X"   for find-the-section-that-defines-X workflows
                       (ranked section hits with body excerpts +
                        heading_path)

  grep -rn "X"         for find-every-occurrence-with-line-numbers
                       workflows (exhaustive, line-precise)

  anneal -e '? ...'    for find-the-handles-matching-graph-predicate
                       workflows (compositional, structural)

Cold agents need to pick the right tool without trial-and-error.
Host-corpus reviewer reported having to learn the delineation through
v0.14 friction; v0.15 SKILL.md should teach it explicitly.

**P. Cross-corpus boundary teaching.** describe external (or
adjacent topic) explains that `*handle{kind: "external"}` covers
two distinct sub-classes:
  - Documents outside the corpus (URLs, refs to other repos)
  - Code references within the same repo (lib/lang/file:N-M after
    R2)

Both share the kind for substrate simplicity; metadata
distinguishes them at query time. Future anneal-code adapter
promotes the in-repo code-ref sub-class to first-class code
handles (per D2c → D2b note).

**Q. Schema-discovery via errors as a first-class teaching pattern.**
Anneal already does this: `*edge{from: src, target: t}` returns
"unknown field 'target'; allowed fields: [from, to, kind, ...]".
Host-corpus reviewer flagged this as a quietly excellent UX. describe
schema (or describe runtime) should explicitly call this out so
agents know typo-then-read-error is a valid discovery workflow,
not just a failure mode.

**R. CHANGELOG narrative.** "anneal makes retrieval as sharp as
convergence."

## What v0.15.0 will FEEL like

### Section-level search on host-corpus-dev

```
$ anneal --root /path/to/host-corpus-dev/.design context "lease protocol" --hits 3
Hits
 1. 2026-05-28-authority-admission-seat-sync-contract.md#§2-lease-protocol
    score=0.97  field=heading+body  heading_path="§2 / Lease Protocol"
 2. 2026-05-28-authority-admission-seat-sync-contract.md#§3-failure-modes
    score=0.84  field=body  heading_path="§3 / Failure Modes"
 3. 2026-05-15-coordinator-architecture.md#§4-leases
    score=0.71  field=heading+body  heading_path="§4 / Leases"

Read
span_id=contract.md#h/lease-protocol  lines=142-218  tokens=1840
  ## §2 Lease Protocol
  
  A lease is granted by the Admission service when an authority
  passes the seat-sync handshake. The lease binds (authority_id,
  seat_id, expires_at) and renews on heartbeat...
```

Change vs v0.14:
- Hits at section granularity, not document
- score=0.97 (heading + body) outranks 0.84 (body only) — score
  signal is meaningful
- Read excerpt is §2 of the matched doc, not the doc intro
- heading_path shows where the match landed in the document

### Code references in handle output

```
$ anneal --root /path/to/host-corpus-dev/.design handle authority-admission-contract.md
Handle 2026-05-28-authority-admission-seat-sync-contract.md (52 edges)
kind=file  status=authoritative  at=contract.md:1
summary="Lease lifecycle, seat-sync handshake, failure-mode
        catalog for the authority admission service."

Outgoing
  Cites (8)
    1. CR-D29-agent-loop  at=contract.md:14
    2. SR-1-substrate-runtime  at=contract.md:28
    ...
  DependsOn (3)
    1. host-corpus-dev-architecture.md  at=contract.md:6
    ...
  Code references (39)
    1. lib/host-corpus/admission.rs:142-167  at=contract.md:88
    2. lib/host-corpus/seat_sync.ex:200-250  at=contract.md:104
    ...

Incoming
  Cites (12)
    ...

Impact
  Direct (5)         lib/host-corpus/heartbeat.rs:34-48 ...
  Indirect (18)      ...
```

Change vs v0.14:
- 39 code references surface as a dedicated section
- Cold-agent friction R2 closes: design → code traceability becomes
  queryable

### handle --impact for code

```
$ anneal --root /path/to/host-corpus-dev/.design handle lib/host-corpus/admission.rs:142-167 --impact
Handle lib/host-corpus/admission.rs:142-167
kind=external  external_class=code  target=lib/host-corpus/admission.rs:142-167

Incoming (3 Cites edges from design corpus)
  1. authority-admission-seat-sync-contract.md  at=contract.md:88  Cites
  2. 2026-05-15-coordinator-architecture.md     at=arch.md:142     Cites
  3. 2026-05-12-substrate-kernel-design.md      at=kernel.md:67    Cites

Impact
  Direct (3 design docs reference this code section)
```

A code path's "impact" is the design corpus that references it.
Bidirectional traceability without leaving anneal.

## What v0.15.0 will NOT include

```
DEFERRED to v0.16+ (out of scope here):
  - anneal-code adapter (emitting code-side facts directly)
  - Multi-corpus federation (design + code corpora joined)
  - learn verb (schema + describe collapse) — still premature
  - convergence(...) system aggregate

PARALLEL design conversations:
  - Phase 0 substrate implementation (must land before R1B)
  - Future: anneal-code adapter as v0.16 substrate work
```

## Estimated cost

| Theme | Items | Cost |
|---|---|---|
| Phase 0 prereq (subtrate impl) | (from .design/2026-05-28-phase0-handle-kind-consolidation.md) | 1-2 days |
| R1. Section-level retrieval | A, B, C, D | 1-2 days |
| R2. Code-reference extraction | E, F, G | 1 day |
| R3. Search ranking quality | H, I, J | 0.5 day |
| R4. handle command depth | K, L, M | 0.5 day |
| R5. Documentation + teaching | N, O, P | 0.5 day |
| **v0.15.0 total (after Phase 0)** | | **~4-5 days** |

Including Phase 0 prereq: ~6-7 days from v0.14 tag to v0.15.0 ship.

## Locked decisions (post-codex + host-corpus reviewer convergence)

1. **Phase 0 sequencing.** LINEAR: Phase 0 implementation first,
   then R1, then R2/R4, then R3, then R5. R3 status/hub boosts
   and R4 grouping can implement independently but their
   acceptance meaning changes after span-granular hits exist; do
   not let docs teach section-level behavior before R1 works.
   R5-Q (schema-discovery-via-errors) can land early because the
   behavior already exists.

2. **D2c (external + metadata).** External handles, Cites edges,
   target path/range in metadata (`md.external_class = "code"`,
   `md.code_path`, `md.code_start_line`, `md.code_end_line`).
   `*handle.file` and `*handle.line` stay clean as
   DISCOVERY-LOCATION fields. D2c → D2b promotion path under
   future anneal-code adapter.

3. **D4a status-aware boost defaults.** Defaults include sensible
   status tier weights (authoritative > active > draft).
   Per-corpus override via `config search_boost { ... }`.

4. **Heading path display: `§2 / Lease Protocol`.** Single delim
   `/`. Omit `§?` when section number absent.

5. **Body code-path regex defaults: `(crates|lib|src|app|test|priv|native)/...`** —
   includes Phoenix/Elixir `priv/` and Rust-NIF `native/`.
   Project extension via `config code_path_root { ... }`.
   ALSO catches backtick-quoted code refs (host-corpus reviewer evidence).
   Build-output paths (`_build/`, `target/`, `node_modules/`)
   excluded by default and not user-configurable.

6. **Backward compat — intentional behavior changes documented
   with recovery (codex convergence):**

   | Risk | Recovery |
   |---|---|
   | `*handle{kind: "section"}` returns zero rows | Maps to `*span{...}`; teach in describe |
   | Old `file#section` handle lookups fail | Recover via `read file --span-id ...` or `*span{handle: file}` queries |
   | Saved trail/span ids from old snapshots may not resolve | Warning emitted, agent prompted to re-snapshot |
   | `search(...)` cardinality changes — multiple hits per file | Callers assuming one row per handle must group or TopK after the span-granular relation |

7. **Score-clustering gate (codex convergence).** Qualitative
   ranking property, not numeric golden:
   - Authoritative defining section must land above older passing
     mentions on real corpora
   - Section hits must show materially wider spread than v0.14's
     document-level cluster (0.738-0.940)
   - Repeated spans from one authoritative file must NOT crowd
     out all diversity
   If the spread doesn't reproduce, tune defaults before tag or
   narrow CHANGELOG claim. Shipping "with knobs" is acceptable
   only after default behavior is already better.

8. **Span-id format: structural, not line-based (codex correction).**
   `file_path#h/<heading-slug-path>[~<occurrence>]`. Stable under
   file edits. Line numbers in `*span.start_line` / `*span.end_line`
   for sorting and excerpt rendering. Identity uses document
   structure.

## CHANGELOG planning for the v0.14.0 bundle

Per codex convergence: sub-narrative sections inside one v0.14.0
entry, not a flat Added/Changed/Removed list.

```
## v0.14.0 - 2026-05-XX

anneal calibrates the signal, simplifies the substrate, and
sharpens retrieval.

### Calibration
- freshness_decay default lowered from 2 to 1 (Behavior Change)
- config potential_weight { ... } override schema + 
  effective_potential_weight predicate
- describe potential_weight teaches override syntax inline
- describe blocker teaches primary_entropy join

### Substrate
- Section handles retired; *span first-class for heading addressing
  (Behavior Change: *handle{kind: "section"} returns zero rows)
- Span-id format: structural heading-path, not line-based
- Span-id recovery: warnings on old format with migration hints

### Retrieval
- search returns span-granular hits with heading_path metadata
- read returns matched-span body with hint-on-truncation
- context composes the above into "find the section that defines X"
- Code paths in markdown bodies become external Cites with target
  metadata (lib/lang/file:N-M backtick-quoted refs captured)
- Status-aware ranker boost (authoritative > active > draft)
- Hub-degree ranker boost

### Teaching
- Magic-word describe cards deepened (entropy, potential, frontier,
  blocker, advancing, holding, drifting, flow, settled, terminal,
  active, obligation, freshness, flux, hub, orphan, area_of, *concern)
- describe convergence becomes multi-section card (meta + The Act +
  Vocabulary + Tuning)
- describe runtime distinguishes snapshot / generation / trail
- SKILL.md explicit delineation: context for sections, grep for
  occurrences, eval for graph predicates
- Schema-discovery-via-errors taught as a first-class pattern
- AGENTS.md, CLAUDE.md, README.md synchronized to v0.14 surface

### Compatibility
- Behavior change: freshness_decay default 2 → 1
- Behavior change: *handle{kind: "section"} retired, recovers via
  *span{} queries with documented mapping
- Behavior change: search returns one row per matching span (not
  per file); callers assuming one row per handle must group or TopK
- Old line-based span ids emit warning with structural replacement
- Old top_work / blocked_row / recent aliases retired (deprecation
  cycle complete from v0.13)
- work_candidate remains as deprecated alias for potential through
  v0.14; retire in v0.15

### Removed
- Section handle kind (substrate fold per Phase 0 Candidate A)
- top_work, blocked_row, recent (deprecation cycle complete)
- The `anneal status --json --compact` flag is gone
- The `anneal map --around=<handle>` command (was already retired)
```

## Review section

### Claude (author, 2026-05-28)

The cold-agent contract is: arrive on an unfamiliar corpus, find
work, find evidence, do work, verify, leave better than found.
v0.13-14 made "find work" sharp. v0.15 should make "find evidence"
sharp by the same physics-of-convergence frame: section-level hits
are the analog of frontier-level work-picking, code-refs are the
graph-substrate analog of typed citations.

The biggest design decision is D2 (code-ref edge encoding). Lean
D2c (external kind + metadata) for substrate simplicity. The risk
is that it under-types code refs; the upside is that the change is
purely additive and immediately useful.

Phase 0 sequencing is the other gating question. Lean sequential:
Phase 0 implementation first, v0.15 design + implementation after.
~1 week from v0.14 tag.

### Codex convergence (2026-05-28, bundled v0.14.0 review)

Reviewed under project owner's scope pivot: calibration, Phase 0 substrate,
retrieval, and teaching all ship as one v0.14.0 bundle. I converge on
the model with two corrections: the release narrative should broaden
from "calibrates the convergence signal" to something like "anneal
calibrates the signal, simplifies the substrate, and sharpens
retrieval"; and span ids should not use line numbers as their stable
identity component.

**Model and release shape.** "anneal makes retrieval as sharp as
convergence" is the right retrieval-section narrative. It belongs in
the same bundled release because the cold-agent loop is one loop:
find work, find evidence, act, verify. Splitting retrieval from the
substrate would create a release where the language can name better
retrieval but cannot yet deliver it. The research-graph check supports
this: retrieval schema is an interface, not just storage, and global
sensemaking requires graph/aggregation structure in addition to local
lookup. Section spans, status/hub signals, and code-ref metadata are
retrieval handles, not decorative facts.

**Phase 0 sequencing.** Keep the linear path: Phase 0 section→span
fold first, then R1, then R2/R4, then R3 tuning, then R5 docs. R3
status/hub boosts and R4 grouping can be implemented independently,
but their acceptance meaning changes after span-granular hits exist.
Do not let docs teach section-level behavior before R1 works. The one
R5 item that can land early is schema-discovery-via-errors, because
the behavior already exists.

**D2c vs D2b.** D2c is the right v0.14.0 call. A `code` handle kind
earns its keep when an `anneal-code` adapter emits code facts as a
source, not when markdown merely points at code paths. Until then,
external-with-metadata is honest substrate staging: no new handle kind,
no new edge kind, immediate traceability, clean D2b promotion path.
The important guardrail is not to overload `*handle.file` /
`*handle.line`; those are discovery-location fields today. Code target
path/range should live in metadata (`md.external_class = "code"`,
`md.code_path`, `md.code_start_line`, `md.code_end_line`) and the
renderer can project that metadata as `target=...`.

**Score-clustering gate.** Make this a v0.14.0 release gate, but not
an exact numeric-golden gate. The gate should prove a qualitative
ranking property on real corpora: the authoritative defining section
lands above older passing mentions, section hits show a materially
wider spread than the v0.14 document-level cluster, and repeated spans
from one authoritative file do not crowd out all diversity. If the
spread does not reproduce, tune defaults before tag or narrow the
CHANGELOG claim. Shipping "with knobs" is acceptable only after the
default behavior is already better.

**CR-D102 budget.** The bundle stays inside the framework if it keeps
its subtraction real. Retiring section handles is a major substrate
reduction; D2c uses existing `external` handles and Cites edges; R3 is
ranker math, not a new command; R4 is rendering over existing graph
shape. No balancing retirement is needed unless implementation adds a
new edge kind or visible command. New config surfaces (`search_boost`,
`code_path_root`) need describe cards and examples because they are
surface area even if they are not commands.

**CHANGELOG shape.** Use sub-narrative sections inside one v0.14.0
entry: Calibration, Substrate, Retrieval, Teaching, and Compatibility.
Behavior changes need explicit bullets: freshness_decay default,
section handles retired in favor of `*span`, search/context now return
span-granular hits and may return multiple hits per file, and code
paths in markdown bodies become external Cites targets. A single flat
Added/Changed/Removed list will bury the migration story.

**Span-id format.** Pushback: `file_path#heading-slug-line-N` is not
stable under ordinary file edits that insert lines above the heading.
Keep line numbers in `*span.start_line` / `end_line`, not in the id.
Prefer a heading-path id such as
`file_path#h/architecture/lease-protocol`, with an occurrence suffix
for duplicate sibling headings (`~2`) and a recovery warning for old
line-based or section-handle ids. Sorting can use `start_line`; identity
should use document structure.

**Backward compatibility.** Document three silent-break risks as
intentional behavior changes with recovery: `*handle{kind: "section"}`
now maps to `*span{...}`, old `file#section` handle lookups recover to
`read file --span-id ...` / span queries, and saved trail/span ids from
old snapshots may not resolve. Also document that raw `search(...)`
cardinality changes: callers that assumed one row per handle must group
or TopK after the span-granular relation.

Net: converge on the bundled v0.14.0 shape. Required corrections before
implementation are (1) no new CodeRef edge kind in the selected path,
(2) metadata, not `*handle.file`, for code target fields, and (3) a
non-line-based stable span id.

### Host-corpus reviewer convergence (2026-05-28)

Reviewed the v0.15 design after running diagnostic queries against
host-corpus-dev corpus. Converged with six answers + two additional flags.

**D3 investigation (the critical finding).** Markdown-link Cites
ARE captured AND ARE rendered by handle. Test on host-corpus-dev:
`research/2026-04-02-excalibur-study.md` returns 39 captured outgoing
edges including markdown-link refs with file:line metadata, all
rendered correctly. `architecture/2026-05-28-authority-admission-seat-sync-contract.md`
returns ZERO outgoing edges — but the
contract has 39 `lib/host-corpus/...:N-M` refs ALL in backtick form,
ZERO as markdown links. Fix is PARSER-SIDE (R2-E extension),
not renderer-side. R4-K retired from scope; R4-L (group by kind)
+ R4-M (Code references section) remain.

**Section-level retrieval simulation: confirmed.** The 0.97 / 0.84 /
0.71 score spread is appropriately wide; the section-granular hits
plus body-excerpt-of-matched-section plus heading_path are exactly
the shape needed for "find the section that defines X."

  ONE FLAG: score-clustering on real host-corpus-dev corpus (v0.14
  showed 0.738 / 0.858 / 0.940 cluster). The design assumes
  section-granularity + status boost will widen the spread. Worth
  validating against host-corpus-dev before shipping that the simulated
  0.97/0.84/0.71 spread reproduces, not just the designed examples.

**D2c (external + metadata): confirmed.** Phase 0 is reducing kinds;
D2b adds one against that grain. D2c is purely additive and
forward-compatible with anneal-code adapter (v0.16+ promotion to
D2b under federation). Pushed back: design should document the
D2c → D2b promotion path so v0.15 isn't lock-in. Done above.

**Heading path: `§2 / Lease Protocol` confirmed.** When section
number is absent, omit `§?` cleanly — just `Lease Protocol`.

**Body regex scope: extended.** Default set is
`(crates|lib|src|app|test|priv|native)/...` — adds Phoenix/Elixir-
canonical `priv/` and Rust-NIF `native/`. `config code_path_root
{ ... }` is the project-level extension path. ALSO: catch backtick
code-style refs (`lib/host-corpus/seat_channel.ex:23-41`) not just
markdown-link refs. This is the critical addition; host-corpus-dev's
real-world corpus uses backticks heavily.

**Additional gaps surfaced (folded into design above):**
- R5-O: explicit delineation of context vs grep vs eval workflows
  in SKILL.md teaching
- R5-P: cross-corpus boundary teaching (external kind sub-classes)
- R5-Q: schema-discovery-via-errors as first-class teaching pattern
- C2: span-aware budget truncation behavior (hint, don't fail or
  silently truncate)

**Priority order from host-corpus reviewer's use case:**
1. R1 section-level retrieval — crosses threshold from "reach for
   sometimes" to "reach for first" for design corpus work
2. R2 code-ref extraction — unlocks design↔code traceability (major
   coordinator workflow)
3. R3 (ranking) + R4 (handle depth) — compound polish

**Net host-corpus reviewer verdict:** design is solid; simulations match
what they'd reach for; D2c lean is right for v0.15 substrate budget;
Phase 0 sequencing as gating is sensible.

### project owner decision (pending)

After codex + host-corpus reviewer convergence, decide:
- Phase 0 sequencing (parallel vs sequential)
- D2 candidate (D2c vs D2b)
- Ship timing
