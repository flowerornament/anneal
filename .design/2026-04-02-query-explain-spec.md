---
status: draft
updated: 2026-04-02
description: >
  Proposal to extend anneal with two composable structural tools: `query` for
  ad hoc graph selection and `explain` for provenance-oriented justification of
  derived results such as diagnostics, impact sets, obligations, and
  convergence summaries.
references:
  anneal-spec: .design/anneal-spec.md
  progressive-disclosure: .design/2026-04-02-progressive-disclosure-output-spec.md
  qmd: https://github.com/jamesrisberg/qmd/blob/dev/QMD-SETUP.md
  shacl: https://www.w3.org/TR/shacl/
  prov-dm: https://www.w3.org/TR/prov-dm/
  cypher-intro: https://neo4j.com/docs/getting-started/cypher-intro/
  souffle: https://www.souffle-lang.com/tutorial
---

# Query + Explain Spec

## Part I: Motivation

### §1 Problem Statement

`anneal` already has a strong command set for common workflows:

- `status` for orientation
- `check` for diagnostics
- `get` for inspecting one handle
- `find` for identity lookup
- `map` for neighborhood structure
- `impact` for reverse dependencies
- `diff` for change over time
- `obligations` for linear namespaces

This set is effective, but it leaves two important gaps.

The first gap is **structural selection**:

- "show every `DependsOn` edge crossing a confidence boundary"
- "show active labels with no incoming edges"
- "show files in `synthesis/` with more than 20 outgoing references"
- "show all undischarged obligations in namespace `P`"

Today these questions are awkward. They are too broad for `get`, too specific
for `status`, and not expressible through `find`, which is intentionally an
identity search rather than a structural query engine.

The second gap is **justification**:

- "why is this a `W002` confidence gap?"
- "why is this obligation considered mooted?"
- "why did `impact` include this file?"
- "why is convergence reported as holding rather than advancing?"

Today `anneal` reports answers, but it does not expose a dedicated provenance
surface for explaining how those answers were derived.

### §2 Why This Matters

`anneal` is not a semantic search tool. It is a structural instrument for
arriving intelligences working across sessions in a shared corpus.

That means the missing capabilities are not:

- richer fuzzy search
- ontology publishing
- workflow enforcement

The missing capabilities are:

- ask a new structural question
- understand why a structural answer was produced

These two capabilities map naturally to two tools:

- `anneal query`
- `anneal explain`

### §3 Relationship to Existing Theory

The existing spec already places anneal near several ideas from knowledge
representation and reasoning:

- typed handles and typed edges [KB-D1, KB-D5]
- convergence lattices [KB-D7]
- linear/affine obligations [KB-F4]
- local graph checks [KB-R1 through KB-R5]
- graph-level suggestions [KB-E8]

`query` and `explain` do not change the kernel. They expose additional views of
the same kernel.

In KRR terms:

- `query` is a small graph-selection surface over explicit facts
- `explain` is a provenance / derivation surface over explicit facts and rules

This is closer to:

- Datalog-style fact querying
- SHACL-like graph validation thinking
- provenance explanation

than it is to:

- vector retrieval
- open-world ontology reasoning
- enterprise workflow tools

### §4 Relationship to QMD

QMD is a useful adjacent reference because it addresses a nearby problem:
retrieval and orientation over a body of knowledge. But it does so through a
different primitive.

QMD's primitive is approximately:

- retrievable chunk
- indexed text
- lexical/vector/hybrid relevance

anneal's primitive is:

- explicit handle
- explicit typed relation
- derived structural state

This distinction matters.

QMD helps answer:

- "what content is probably relevant?"

anneal helps answer:

- "what explicitly exists?"
- "what depends on what?"
- "what is structurally wrong?"
- "what has changed while I was away?"

`query` and `explain` stay structural. They do not pull anneal toward semantic
retrieval as their primary model.

## Part II: Design Goals

### §5 Goals

This design has six goals.

#### §5.1 Structural Expressiveness

Users and agents can ask graph-shaped questions that are narrower
than `status` and broader than `get`.

#### §5.2 Provenance Clarity

Derived outputs are explainable in terms of handles, edges, statuses,
rules, and snapshots.

#### §5.3 Composability

The new tools compose naturally with the existing eight commands rather
than creating a parallel workflow.

#### §5.4 Bounded Defaults

Like the progressive-disclosure redesign, human and JSON outputs must remain
bounded by default, with explicit expansion paths.

#### §5.5 Performance Predictability

The tools run comfortably on corpora in the current scale envelope
(hundreds of files, ~10k handles, ~10k edges) without requiring a persistent
database or precomputed secondary index beyond what anneal already builds.

#### §5.6 Kernel Fidelity

The tools preserve anneal's core principles:

- files are truth [KB-P1]
- everything is a handle [KB-P2]
- capabilities over process [KB-P4]
- local checks over global propagation [KB-P7]

### §6 Non-Goals

This design does not:

- introduce semantic or vector search
- replace `find` with a fuzzy retrieval system
- introduce RDF, OWL, or SHACL as a required internal representation
- add workflow mutation commands
- persist a separate graph database
- make anneal depend on a theorem prover or external query runtime

### §6.1 Command-Set Impact

The canonical anneal spec currently defines eight commands.

This document extends anneal's top-level command set from eight to ten:

- the current eight commands in `anneal-spec.md`
- plus `query`
- plus `explain`

This document is an extension of §12 of the canonical spec while it remains in
draft.

## Part III: `anneal query`

### §7 Purpose

`anneal query` is the ad hoc structural selector.

It answers:

- "which handles/edges/diagnostics/suggestions satisfy these explicit graph predicates?"

It does **not** answer:

- "which documents are semantically related to this phrase?"

### §8 Command Shape

The initial surface uses typed subcommands rather than a freeform query
language.

```text
anneal query handles ...
anneal query edges ...
anneal query diagnostics ...
anneal query obligations ...
anneal query suggestions ...
```

This keeps the surface:

- discoverable in `--help`
- machine-safe
- easy to document
- easy to bound

### §8.1 Defaults, Scope, and Side Effects

`query` inherits anneal's progressive-disclosure discipline.

Common defaults:

- default limit: `25`
- default offset: `0`
- `--full` required for unbounded result sets
- deterministic ordering within each domain
- no snapshot append side effects

Common controls:

- `--limit <n>`
- `--offset <n>`
- `--full`
- `--scope <active|all>`

Domain-specific default scope:

- handles: active handles only (non-terminal)
- edges: edges whose source and target handles are both in the active view
- diagnostics: same active-only default as `anneal check`
- suggestions: same active-only default as `anneal check --suggest`
- obligations: non-mooted obligations whose creator remains in the active view

`--scope all` widens to the full visible corpus for the relevant domain.

For obligations, `--scope all` includes mooted obligations and obligations whose
creator is already terminal.

### §8.2 Ordering

Each query domain defines a stable default sort order:

- handles: `id`
- edges: `(source, kind, target)`
- diagnostics: `(severity, code, file, line, message)`
- obligations: `(namespace, handle)`
- suggestions: `(code, primary_handle)`

This keeps pagination stable and makes agent-side caching easier.

### §9 Query Domains

#### §9.1 `anneal query handles`

Select handles by properties and local graph counts.

Example usage:

```bash
anneal query handles --kind file --status formal
anneal query handles --namespace OQ --incoming 0
anneal query handles --terminal false --file-pattern 'synthesis/**'
anneal query handles --orphaned
```

Candidate filters:

- `--kind <file|section|label|version|external>`
- `--status <status>`
- `--namespace <prefix>`
- `--terminal <true|false>`
- `--file-pattern <glob>`
- `--incoming-min <n>`
- `--incoming-max <n>`
- `--incoming-eq <n>`
- `--outgoing-min <n>`
- `--outgoing-max <n>`
- `--outgoing-eq <n>`
- `--updated-before <date>`
- `--updated-after <date>`
- `--orphaned`

#### §9.2 `anneal query edges`

Select edges by kind and endpoint properties.

Example usage:

```bash
anneal query edges --kind DependsOn
anneal query edges --kind DependsOn --confidence-gap
anneal query edges --source LABELS.md
anneal query edges --target OQ-64
anneal query edges --source-status formal --target-status provisional
```

Candidate filters:

- `--kind <Cites|DependsOn|Supersedes|Verifies|Discharges>`
- `--source <handle>`
- `--target <handle>`
- `--source-kind <...>`
- `--target-kind <...>`
- `--source-status <status>`
- `--target-status <status>`
- `--cross-file`
- `--confidence-gap`

Path-oriented or count-oriented edge filters beyond these fields sit outside
Phase 1. They belong to a later extension once the base edge query surface is
proven in real corpora.

#### §9.3 `anneal query diagnostics`

Filter the diagnostics that `check` would otherwise print in compiler-style
form.

Example usage:

```bash
anneal query diagnostics --severity error
anneal query diagnostics --code E001
anneal query diagnostics --file formal-model/v17.md
```

Candidate filters:

- `--severity <error|warning|info|suggestion>`
- `--code <E001|W002|...>`
- `--file <path>`
- `--line <n>`
- `--errors-only`
- `--stale`
- `--obligations`
- `--suggest`

This is not a replacement for `check`; it is a structured slicer over the same
derived result set.

Operationally:

- `query diagnostics` reruns the same diagnostic derivation pipeline as the
  current invocation of `check`
- it then applies its own selectors over that fresh result set
- it inherits `check`'s active-only default unless widened via `--scope all`
- it never appends a snapshot
- it never emits extraction payloads or extraction summaries; the domain is
  diagnostics only

It is explicitly **not** a query over previously materialized diagnostics.

The family flags from `check` remain available as convenience aliases:

- `--errors-only` = `--severity error`
- `--stale` = `--code W001`
- `--obligations` = `--code E002|I002`
- `--suggest` = `--severity suggestion`

The normalized selectors remain the canonical contract in the spec and JSON
surface.

#### §9.4 `anneal query obligations`

Filter obligation-related states.

Example usage:

```bash
anneal query obligations --namespace P --undischarged
anneal query obligations --multi-discharged
anneal query obligations --mooted
```

Candidate filters:

- `--namespace <prefix>`
- `--undischarged`
- `--discharged`
- `--multi-discharged`
- `--mooted`

#### §9.5 `anneal query suggestions`

Filter structural suggestion outputs.

Example usage:

```bash
anneal query suggestions --code S001
anneal query suggestions --code S005
```

This becomes more valuable as suggestion outputs become richer and more
numerous.

### §9.6 Filter Semantics

Filters on fields that are only meaningful for some rows exclude
non-applicable rows rather than erroring.

Examples:

- `--namespace OQ` excludes non-label handles
- `--status formal` excludes rows with no declared status
- `--updated-before` excludes rows with no `updated` metadata

The command errors only when a filter is syntactically invalid or
semantically impossible for the queried domain as a whole.

### §10 Output Contract

#### §10.1 Human Output

Human output uses the same dense, aligned style proposed for anneal's
other human-readable commands:

- section label gutter
- stable columns
- bounded rows
- explicit totals
- footer with expansion guidance

Example:

```text
matches    25 of 148 handles

kind     status    incoming  outgoing  handle
label    open             0         1  OQ-64
label    open             0         2  OQ-65
label    open             0         0  OQ-66

next     anneal get OQ-64 --context
         anneal map --around=OQ-64
```

#### §10.2 JSON Output

JSON follows anneal's existing summary-first envelope pattern.

Example:

```json
{
  "_meta": {
    "schema_version": 2,
    "detail": "sample",
    "truncated": true,
    "returned": 25,
    "total": 148,
    "expand": ["--limit 50", "--offset 25", "--full"]
  },
  "kind": "handles",
  "items": [
    {
      "id": "OQ-64",
      "handle_kind": "label",
      "status": "open",
      "file": "OPEN-QUESTIONS.md",
      "incoming": 0,
      "outgoing": 1
    }
  ]
}
```

### §11 Internal Model

The initial implementation builds lightweight row projections over the
existing in-memory structures.

Example projections:

```text
HandleRow
  id
  handle_kind
  status
  file
  namespace
  terminal
  incoming_count
  outgoing_count

EdgeRow
  source
  target
  edge_kind
  source_kind
  target_kind
  source_status
  target_status
  source_file
  target_file
```

These rows are derived from the existing graph and lattice, not persisted.

Diagnostic query rows additionally expose a stable `diagnostic_id`
because that identifier is the cleanest handoff target for `explain
diagnostic`.

Suggestion query rows expose a stable `suggestion_id` for the same reason.

### §12 Performance

The initial performance model is straightforward:

- handle queries: `O(|H|)`
- edge queries: `O(|E|)`
- diagnostics queries: `O(|D|)`
- obligations queries: `O(|H| + |E|)` in the worst case, usually much smaller

At current anneal scales, full scans over these derived collections are
acceptable.

Possible later optimizations:

- precomputed in-degree / out-degree arrays
- namespace/status buckets
- reusable row projections within one invocation

But these are not required for the first version.

### §13 Future Extension

If the typed subcommand model proves useful, a later phase adds a compact
predicate language:

```bash
anneal query handles --where 'kind = "label" and namespace = "OQ" and incoming = 0'
```

This is explicitly a later extension, not part of the first version.

## Part IV: `anneal explain`

### §14 Purpose

`anneal explain` is the provenance / justification tool.

It answers:

- "why did anneal produce this result?"

The important distinction is:

- `query` discovers structure
- `explain` justifies derived outputs

### §15 Command Shape

The initial surface uses explicit explanation domains:

```text
anneal explain diagnostic ...
anneal explain impact <handle>
anneal explain convergence
anneal explain obligation <handle>
anneal explain suggestion ...
```

### §15.1 Defaults, Scope, and Side Effects

`explain` also follows progressive disclosure:

- bounded by default where a domain can fan out
- explicit `--full` for multi-path or fully expanded explanation detail
- no snapshot append side effects

The default `explain` contract answers:

- one selected derived result
- one canonical explanation
- one minimal set of supporting facts

unless the caller explicitly expands it.

### §16 Explanation Domains

#### §16.1 `anneal explain diagnostic`

Explain a specific diagnostic.

Example usage:

```bash
anneal explain diagnostic E001 --file formal-model/v17.md --line 1847
anneal explain diagnostic W002 --handle formal-model/v17.md
anneal explain diagnostic E002 --handle P-3
```

The explanation includes:

- the triggering rule
- the relevant handles
- the relevant edge kind, if any
- the relevant states or evidence fields
- source span(s), when available

This command builds on and extends the existing `Evidence` structures in
`checks.rs`.

#### §16.1.1 Diagnostic Identity

`explain diagnostic` needs a stable way to refer to one diagnostic.

Primary selector:

```bash
anneal explain diagnostic --id <diagnostic-id>
```

`diagnostic_id` is included in JSON emitted by:

- `anneal check --json`
- `anneal query diagnostics --json`

The identifier is a deterministic fingerprint over the diagnostic's
structural identity, for example:

- severity
- code
- file
- line
- normalized message
- structured evidence fields, when present

Secondary selectors such as `--code`, `--file`, `--line`, and `--handle`
remain for convenience, but they succeed only when they resolve
unambiguously. Otherwise the command instructs the caller to use `--id`.

#### §16.1.2 Phase 1 Diagnostic Coverage

Phase 1 treats explanation coverage for current diagnostic codes as a
requirement, not an optional stretch goal.

That means:

- every current diagnostic family must map to a structured explanation shape
- the existing `Evidence` enum is the seed of that model, not the limit of it
- if a diagnostic is currently emitted without structured evidence, the
  implementation phase must add the missing explanation facts

This avoids an awkward command that only explains some diagnostics while
silently failing on others.

#### §16.2 `anneal explain impact <handle>`

Explain why `impact` returned each affected handle.

Example usage:

```bash
anneal explain impact formal-model/v17.md
```

The output includes at least one causal path from the queried handle to
each affected handle:

- direct path
- shortest indirect path

This builds on the existing reverse traversal in `impact.rs`, with
predecessor recording added for explanation paths.

The default explanation path is:

- shortest by hop count
- with deterministic lexical tie-breaking when multiple shortest paths exist

`--full` may expose multiple explanation paths later, but the default returns
exactly one canonical path per affected handle.

#### §16.3 `anneal explain convergence`

Explain why the current convergence summary is:

- advancing
- holding
- drifting

Example usage:

```bash
anneal explain convergence
```

The output unpacks:

- current and previous snapshot totals
- deltas that were considered
- the specific threshold/heuristic branch selected
- why the other branches did not apply

This builds on the same snapshot delta logic that powers `status` and
`diff`.

#### §16.3.1 Convergence Reference Semantics

Phase 1 makes `explain convergence` match `status`, not `diff`.

Default behavior:

- compute the current snapshot from the current graph
- compare it against the immediately previous stored snapshot
- use the same rule branch that `status` uses before appending a new snapshot

This keeps the explanation aligned with the signal users most recently saw from
`status`.

Phase 1 does not support arbitrary `--days` or git-ref comparison yet.
Those extensions remain available for a later phase once the basic explanation
contract is stable.

#### §16.4 `anneal explain obligation <handle>`

Explain an obligation's current disposition.

Example usage:

```bash
anneal explain obligation P-3
```

The explanation includes:

- creator
- discharger(s), if any
- whether mooted
- whether creator is terminal
- why the obligation is classified as outstanding / discharged / mooted

#### §16.5 `anneal explain suggestion`

Explain why a suggestion was emitted.

Example usage:

```bash
anneal explain suggestion --id <suggestion-id>
anneal explain suggestion S001 --handle OQ-64
```

This is especially valuable for suggestions whose graph patterns may not be
obvious at a glance.

#### §16.5.1 Suggestion Identity

`explain suggestion` uses the same identity pattern as diagnostics.

Primary selector:

```bash
anneal explain suggestion --id <suggestion-id>
```

`suggestion_id` is included in JSON emitted by:

- `anneal check --json` when suggestions are present
- `anneal query suggestions --json`

Secondary selectors such as `--code` and `--handle` remain convenience forms
and succeed only when they resolve unambiguously.

### §17 Output Contract

#### §17.1 Human Output

Human output is factual and structured, not chatty.

Example:

```text
diagnostic  W002  confidence gap

rule        KB-R3

source      formal-model/v17.md      status formal
target      synthesis/v17.md         status provisional
edge        DependsOn

why         source state exceeds target state

next        anneal get formal-model/v17.md --context
            anneal get synthesis/v17.md --context
            anneal map --around=formal-model/v17.md
```

#### §17.2 JSON Output

JSON exposes structured explanation facts rather than one long string.

Example:

```json
{
  "_meta": {
    "schema_version": 2,
    "detail": "full",
    "truncated": false,
    "expand": []
  },
  "kind": "diagnostic_explanation",
  "code": "W002",
  "rule": "KB-R3",
  "facts": [
    {
      "type": "source_status",
      "handle": "formal-model/v17.md",
      "status": "formal"
    },
    {
      "type": "target_status",
      "handle": "synthesis/v17.md",
      "status": "provisional"
    },
    {
      "type": "edge",
      "kind": "DependsOn",
      "source": "formal-model/v17.md",
      "target": "synthesis/v17.md"
    }
  ]
}
```

### §18 Internal Model

The implementation uses a dedicated explanation model rather than
constructing human-readable strings directly in command code.

Suggested top-level shape:

```text
Explanation
  Diagnostic(DiagnosticExplanation)
  Impact(ImpactExplanation)
  Convergence(ConvergenceExplanation)
  Obligation(ObligationExplanation)
  Suggestion(SuggestionExplanation)
```

This keeps:

- human rendering
- JSON rendering
- future MCP surfaces

all aligned on the same underlying explanation object.

### §19 Performance

The expected cost is modest.

- diagnostic explanation: `O(|D|)` worst case to locate the target diagnostic,
  usually smaller with direct selectors
- impact explanation: `O(|H| + |E|)` to traverse with predecessor recording
- convergence explanation: effectively `O(1)` over two snapshots
- obligation explanation: local graph inspection around one handle
- suggestion explanation: similar to diagnostic explanation

This is acceptable at current corpus sizes.

The identity lookups for diagnostics and suggestions are constant-time once the
derived rows for the current invocation have been materialized.

### §19.1 Explanation Addressing

For all explanation domains, selectors resolve to exactly one target.

If a selector is ambiguous:

- the command fails
- the failure explains how to narrow the selector
- JSON output preserves that ambiguity in a structured way

This is especially important for:

- diagnostics
- suggestions
- obligations in corpora with repeated label-like aliases

## Part V: Composition

### §20 How `query` and `explain` Compose with Existing Commands

These commands are intended to interlock with the existing tools.

Example flows:

```bash
anneal status
anneal query handles --orphaned
anneal get OQ-64 --context
anneal map --around=OQ-64
```

```bash
anneal query edges --kind DependsOn --confidence-gap
anneal explain diagnostic W002 --handle formal-model/v17.md
anneal impact formal-model/v17.md
```

```bash
anneal obligations
anneal query obligations --undischarged
anneal explain obligation P-3
```

```bash
anneal diff
anneal explain convergence
```

This produces a coherent family:

- `status` / `diff` for overview
- `query` for structural discovery
- `get` / `map` for local inspection
- `impact` for dependency consequence
- `explain` for derivation and justification

## Part VI: Implementation Plan

### §21 Phase 1

Implement:

- `query handles`
- `query edges`
- `query diagnostics`
- `explain diagnostic`
- `explain impact`
- `explain convergence`

These provide the highest leverage with the least semantic ambiguity.

### §22 Phase 2

Implement:

- `query obligations`
- `query suggestions`
- `explain obligation`
- `explain suggestion`

### §23 Phase 3

Consider:

- compact `--where` expressions for `query`
- richer derivation traces
- MCP-facing explanation objects
- optional export to external graph / provenance ecosystems

## Part VII: Resolved Decisions

### §24 Query Surface

The first release of `query` uses typed flags only. A compact predicate
language remains a later extension.

### §25 Explanation Addressing

Diagnostics are identified primarily by `diagnostic_id`. Code/file/line and
handle-centric selectors remain secondary conveniences when they resolve
unambiguously.

### §26 Rule Identity

Explanations surface both anneal-local rule codes (`E001`, `W002`, `S001`) and
spec IDs (`KB-R3`) when a stable mapping exists.

### §27 Query / Explain Interop

`query diagnostics` results include `diagnostic_id` so agents can pipeline
directly into `explain diagnostic` without reconstructing the selector.

### §28 Canonical Command Surface

Once implemented, `query` and `explain` become part of anneal's canonical
top-level command set and the canonical command count in `anneal-spec.md`
updates from eight to ten.

## Part VIII: Conclusion

`anneal query` and `anneal explain` are natural extensions of the existing
kernel.

They do not pull anneal toward semantic retrieval, workflow enforcement, or
heavy ontology infrastructure. Instead they deepen what anneal already is:

- a compiler for explicit knowledge structure
- a checker for local consistency
- a coordination instrument for disconnected intelligences

`query` makes the structure askable.
`explain` makes the structure accountable.

Together they would make anneal significantly more expressive while preserving
its current architecture and intent.
