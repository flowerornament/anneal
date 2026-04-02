---
status: draft
updated: 2026-04-02
description: >
  Proposal to redesign anneal CLI output around progressive disclosure,
  especially for --json, so agent-facing defaults are bounded, explicit about
  truncation, and expandable without losing tool compatibility.
references:
  anneal-spec: .design/anneal-spec.md
  cli-output-audit: .design/2026-04-02-cli-output-audit.md
  napkin: https://github.com/Michaelliv/napkin
---

# Progressive Disclosure Output Spec

## Part I: Rationale

### §1 Problem Statement

`anneal` is intended for arriving intelligences that need to orient quickly in a knowledge corpus, decide what matters, and only then descend into detail. The current CLI partly supports this goal at the workflow level, but not consistently at the output-contract level.

Several commands currently emit output that is reasonable for a human terminal session but expensive for an LLM or agent context window:

- `anneal check --json` serializes all diagnostics and all extractions
- `anneal find "" --json` returns all matches
- `anneal map --json` embeds the fully rendered graph as a string
- `anneal get --json` emits full incoming/outgoing edge lists even when the human path caps them

These outputs are not merely verbose. They are structurally unbounded. An agent can invoke them with perfectly plausible commands and consume hundreds of KB or several MB of context before it has enough information to decide what to do next.

This creates a mismatch with the tool's intended role:

- the tool should surface the potential landscape
- the tool should answer "what should I inspect next?"
- the tool should not force an arriving intelligence to consume full internal state just to orient

### §2 Why This Matters

The issue is not "JSON is too large." The issue is that `--json` currently conflates two distinct ideas:

1. machine-readable output
2. complete internal output

For agent use, these must be separated. Machine-readable output should be the transport contract. Completeness should be an explicit detail level chosen by the caller.

This distinction matters because:

- LLM cost is driven by tokens, not just line count
- whitespace is a multiplier, but unbounded fields are the core problem
- agents may skip `AGENTS.md`
- agents may not load `SKILL.md`
- agents may only glance at `anneal --help`
- therefore, the CLI itself must be safe by default

### §3 Evidence

The audit in [2026-04-02-cli-output-audit.md](/Users/morgan/code/anneal/.design/2026-04-02-cli-output-audit.md) shows that:

- `check --json` on the Murail corpus emitted about `3.07 MB`
- `find "" --json` on the Murail corpus emitted about `2.08 MB`
- `map --json` on the Murail corpus emitted about `760 KB`
- `get LABELS.md --json` on the Murail corpus emitted about `59 KB`

Simulated summary-first alternatives reduced those outputs to:

- `check` summary JSON: about `737 bytes`
- `find` summary JSON with first 25 matches: about `3.6 KB`
- `map` summary JSON: about `58 bytes`
- `get` summary JSON with capped edges: about `1.4 KB`

The main conclusion is clear: progressive disclosure is not a nice-to-have. It is the right contract for an agent-oriented CLI.

### §4 Design Goals

This change should satisfy six goals.

#### §4.1 Agent Safety

Default machine-readable outputs must be bounded enough to fit comfortably into agent context windows.

#### §4.2 Explicit Truncation

Whenever a result is bounded, the output must explicitly say so in machine-readable form.

#### §4.3 Explicit Expansion

Callers must be shown exactly how to request more detail. Expansion is not hidden behavior. It is an explicit interface.

#### §4.4 Tool Compatibility

`--json` outputs must remain structured, stable, and easy to consume via `jq`, MCP wrappers, scripts, and future tool integrations.

#### §4.5 Human/Agent Coherence

Human and JSON outputs should tell the same story, but they do not need to serialize the same fields. Human output optimizes for readability. JSON output optimizes for bounded machine use.

#### §4.6 Minimal Surprise

Compact commands that are already good, such as `status`, `diff`, and `obligations`, should remain stable unless consistency requires a small adjustment.

### §5 Non-Goals

This proposal does not attempt to solve everything at once.

It does not:

- redesign search semantics (`find` remains identity search in this change)
- introduce semantic/vector search
- add an MCP server
- change the graph model
- change the five local consistency rules
- require phased rollout; the design assumes one integrated contract update

## Part II: Design

### §6 Core Principle

**Progressive disclosure is the default output contract.**

The default output of a command should answer:

> What should I know first?

The next layer of flags should answer:

> What should I inspect next?

The final layer should answer:

> Give me the full dump.

### §7 Output Tiers

Commands that can become large should expose three conceptual tiers.

#### §7.1 Summary

Bounded output that gives counts, small previews, and next-step hints.

This is the default JSON behavior for risky commands.

#### §7.2 Sample

Bounded detail with an explicit limit, such as the first 25 matches or first 50 diagnostics.

This is the common expansion tier for agent workflows.

#### §7.3 Full

Complete output for humans, scripts, or special tooling that truly needs all detail.

This must always be explicitly requested.

### §8 JSON Meta Contract

Commands with potentially unbounded results should wrap their payload in a shared metadata contract.

```json
{
  "_meta": {
    "schema_version": 2,
    "detail": "summary",
    "truncated": true,
    "returned": 25,
    "total": 8421,
    "expand": ["--limit 100", "--full"]
  }
}
```

Field meanings:

- `schema_version`
  - Integer version of the JSON output schema.
  - This proposal introduces `schema_version = 2` for redesigned outputs.

- `detail`
  - One of: `summary`, `sample`, `full`
  - Indicates which disclosure tier the caller received.

- `truncated`
  - `true` if a collection or preview was bounded below its total available size.
  - `false` otherwise.

- `returned`
  - Count of items actually included in the current payload.
  - Present when a collection is bounded.

- `total`
  - Total count of matching or available items.
  - Present when the caller did not receive everything.

- `expand`
  - A short list of concrete CLI expansions the caller can run next.
  - This is part of the machine-readable contract, not just a human hint.

### §9 JSON Formatting

`--json` must emit compact JSON by default.

Rationale:

- Compact JSON is more natural for tool pipelines.
- Pretty-printed JSON significantly increases line count and token cost.
- Formatting is a presentation concern, not a transport concern.

A new global flag `--pretty` should request pretty-printed JSON for humans.

### §10 Command Contracts

This section defines the intended output behavior for each top-level command.

#### §10.1 `anneal status`

`status` is already the best compact orientation command and remains so.

Human default:

- unchanged single-screen dashboard

JSON default:

- compact structured status payload
- bounded by nature
- no special truncation behavior needed

Flags:

- implement `--compact` as already implied in the main spec
- `status --json --compact` is the recommended agent session-start payload

`status --json --compact` should include:

- file / handle / edge counts
- active / frozen counts
- pipeline histogram or states summary
- diagnostics counts
- obligations summary
- convergence summary
- suggestion total

It should exclude:

- verbose file listings per pipeline level
- any future large per-item collections

#### §10.2 `anneal check`

`check` is the highest-priority redesign target.

Human default:

- keep current behavior: actionable diagnostics from active files

JSON default:

- summary-first
- no raw `extractions`
- no full diagnostics list

Default JSON payload:

```json
{
  "_meta": {
    "schema_version": 2,
    "detail": "summary",
    "truncated": true,
    "expand": ["--diagnostics", "--extractions-summary", "--full"]
  },
  "summary": {
    "errors": 12,
    "warnings": 48,
    "info": 3,
    "suggestions": 5,
    "terminal_errors": 0,
    "total_diagnostics": 68
  },
  "by_code": [
    {"code": "E001", "count": 4},
    {"code": "E002", "count": 8}
  ],
  "sample_diagnostics": [
    {
      "code": "E002",
      "severity": "error",
      "file": "formal-model/v17.md",
      "line": 42,
      "message": "..."
    }
  ]
}
```

New flags:

- `--diagnostics`
  - include the diagnostics collection in the response
- `--limit <N>`
  - applies to diagnostics in JSON mode
  - default `50`
- `--extractions-summary`
  - include counts and aggregate extraction facts without full extraction payloads
- `--full-extractions`
  - include full extraction payloads
- `--full`
  - shorthand for full diagnostics and full extractions

Behavior notes:

- `--file` must filter both diagnostics and extraction-related material
- `--active-only` and other existing filters must continue to work
- `--full` is the only path that returns the current "everything" shape

#### §10.3 `anneal get`

`get` already wants progressive disclosure in the main spec. This proposal formalizes it.

Human default:

- keep current readable summary with capped edges

JSON default:

- metadata
- snippet
- edge counts
- capped edge samples
- explicit truncation marker

Default JSON payload:

```json
{
  "_meta": {
    "schema_version": 2,
    "detail": "summary",
    "truncated": true,
    "expand": ["--refs", "--trace", "--limit-edges 50"]
  },
  "id": "LABELS.md",
  "kind": "file",
  "status": "living",
  "file": "LABELS.md",
  "snippet": "...",
  "edge_counts": {
    "incoming": 1,
    "outgoing": 640
  },
  "sample_incoming": [],
  "sample_outgoing": [...],
  "truncated_edges": true
}
```

New flags:

- `--refs`
  - include incoming and outgoing edge lists
  - still bounded by `--limit-edges` unless `--trace` or `--full` is also set
- `--context`
  - produce a compact agent briefing
  - intended target size: roughly 150-250 tokens in human mode
  - JSON shape may expose `briefing` plus compact structured facts
- `--trace`
  - full lineage / adjacency / provenance view
  - explicit high-detail mode
- `--limit-edges <N>`
  - default `10`
- `--full`
  - include full edge lists without sampling

Contract:

- human and JSON outputs both cap edges by default
- full adjacency must be opt-in

#### §10.4 `anneal find`

`find` is the second major progressive-disclosure target after `check`.

Human default:

- still prints matches
- but should say "showing N of M" when bounded

JSON default:

- bounded sample
- total count
- truncation metadata
- optional facets

Default JSON payload:

```json
{
  "_meta": {
    "schema_version": 2,
    "detail": "sample",
    "truncated": true,
    "returned": 25,
    "total": 8421,
    "expand": ["--limit 100", "--offset 25", "--full"]
  },
  "query": "OQ",
  "matches": [...],
  "facets": {
    "kind": [
      {"value": "label", "count": 412}
    ],
    "status": [
      {"value": "draft", "count": 17}
    ]
  }
}
```

New flags:

- `--limit <N>`
  - default `25`
- `--offset <N>`
  - offset into the ordered result set
- `--full`
  - return all matches
- `--no-facets`
  - optional optimization if facets prove costly

Empty query rule:

- empty query is only valid if at least one narrowing filter is present, or if `--full` is explicitly supplied
- otherwise return an error explaining how to refine or explicitly request a full query

Rationale:

- empty query currently behaves like "dump all visible handles"
- that is too easy to invoke accidentally in agent workflows

#### §10.5 `anneal map`

`map` should separate summary from rendering.

Human default:

- change from "full active graph dump" to graph summary unless a focus or full-render flag is supplied

JSON default:

- graph summary only
- no rendered graph content

Default JSON payload:

```json
{
  "_meta": {
    "schema_version": 2,
    "detail": "summary",
    "truncated": false,
    "expand": ["--around OQ-64", "--nodes", "--edges", "--render text --full"]
  },
  "format": "summary",
  "nodes": 5421,
  "edges": 18220,
  "by_kind": {
    "file": 310,
    "label": 6200,
    "section": 1800,
    "version": 95,
    "external": 122
  },
  "top_namespaces": [
    {"namespace": "OQ", "count": 412}
  ]
}
```

New flags:

- `--render <summary|text|dot>`
  - default `summary`
- `--full`
  - required when rendering the full graph in text or dot mode
- `--nodes`
  - structured node list, bounded unless `--full`
- `--edges`
  - structured edge list, bounded unless `--full`
- `--limit-nodes <N>`
  - default `100`
- `--limit-edges <N>`
  - default `250`

Existing flags retained:

- `--around`
- `--depth`
- `--concern`

Behavior:

- `map --around=<handle>` is the preferred focused graph inspection path
- `map --render=text --full` and `map --render=dot --full` are explicit full-render escapes
- JSON `map` no longer embeds a giant rendered `content` blob by default

#### §10.6 `anneal init`

`init` is already safe and stays mostly unchanged.

Potential small improvements:

- add `_meta`
- optionally include a compact summary of inferred fields in JSON

No behavioral redesign is required.

#### §10.7 `anneal impact`

`impact` is mostly acceptable today and should remain simple.

Suggested enhancement:

- include `direct_count` and `indirect_count`
- include `_meta`

No truncation behavior is required initially unless real corpora reveal unusually large impact sets.

#### §10.8 `anneal diff`

`diff` is already compact and should remain so.

Suggested enhancement:

- add `_meta` for consistency

No disclosure redesign required.

#### §10.9 `anneal obligations`

`obligations` is already compact and stable.

Suggested enhancement:

- add `_meta` for consistency

No disclosure redesign required.

### §11 Human Output Policy

Human output should also follow progressive disclosure where output is likely to explode, but human defaults do not need to mirror JSON exactly.

Rules:

- `status`, `diff`, `obligations`, `impact`, and `init` remain close to current human behavior
- `check` remains detailed in human mode
- `find` should announce when it is showing only the first `N` matches
- `get` continues to cap edges
- `map` should become summary-first unless explicitly focused or rendered

### §12 Help and Guidance

The CLI help must teach safe defaults without requiring users to read external documentation.

Changes:

- top-level help should distinguish compact summary commands from full-detail commands
- help text should stop implying that all `--json` outputs are equally suitable for machine consumption
- examples should prefer summary-first and bounded forms
- high-detail examples should be explicit about being full-detail

Examples:

- `anneal check --json --diagnostics --limit 50`
- `anneal find OQ --limit 25`
- `anneal get OQ-64 --context`
- `anneal map --around=OQ-64 --depth=1`

## Part III: Implementation Spec

### §13 Global CLI Changes

#### §13.1 New Global Flags

Add:

- `--pretty`
  - global
  - only meaningful with `--json`
  - pretty-print JSON when requested

Keep:

- `--json`

Behavior:

- `--json` emits compact JSON by default
- `--json --pretty` emits pretty JSON

#### §13.2 JSON Emission

Current implementation:

- [`print_json()`](/Users/morgan/code/anneal/src/cli.rs#L94) uses `serde_json::to_string_pretty`

Required change:

- replace with compact serialization by default
- add optional pretty path

Implementation shape:

```rust
pub(crate) enum JsonStyle {
    Compact,
    Pretty,
}

pub(crate) fn print_json<T: Serialize>(output: &T, style: JsonStyle) -> anyhow::Result<()>;
```

`emit_output()` in [main.rs](/Users/morgan/code/anneal/src/main.rs#L481) must accept the JSON style and pass it through.

### §14 Shared JSON Types

Introduce shared JSON metadata types in `cli.rs`.

```rust
#[derive(Serialize)]
pub(crate) struct OutputMeta {
    pub(crate) schema_version: u32,
    pub(crate) detail: DetailLevel,
    pub(crate) truncated: bool,
    pub(crate) returned: Option<usize>,
    pub(crate) total: Option<usize>,
    pub(crate) expand: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DetailLevel {
    Summary,
    Sample,
    Full,
}
```

Commands that do not need truncation can omit `returned` and `total`.

### §15 `check` Implementation

#### §15.1 CLI Surface

Extend the `Check` subcommand in [main.rs](/Users/morgan/code/anneal/src/main.rs#L172) with:

- `diagnostics: bool`
- `extractions_summary: bool`
- `full_extractions: bool`
- `full: bool`
- `limit: Option<usize>`

#### §15.2 Output Types

Split the current `CheckOutput` into:

- `CheckHumanOutput`
- `CheckJsonOutput`

Or keep `CheckOutput` for human mode and introduce a separate `CheckJsonOutputV2`.

Suggested JSON types:

```rust
#[derive(Serialize)]
pub(crate) struct CheckJsonOutput {
    pub(crate) _meta: OutputMeta,
    pub(crate) summary: CheckSummary,
    pub(crate) by_code: Vec<CodeCount>,
    pub(crate) diagnostics: Option<Vec<DiagnosticPreview>>,
    pub(crate) extractions_summary: Option<ExtractionSummary>,
    pub(crate) extractions: Option<Vec<FileExtraction>>,
}
```

`DiagnosticPreview` should include:

- `code`
- `severity`
- `file`
- `line`
- `message`

Full `evidence` may remain available in full mode if desired.

#### §15.3 Main Control Flow

In [main.rs](/Users/morgan/code/anneal/src/main.rs#L655):

- stop cloning `result.extractions` whenever `cli_args.json` is true
- instead build the correct JSON payload based on requested detail flags
- apply `--file` filtering to extraction-derived output as well

#### §15.4 Defaults

If `--json` is set and none of `--diagnostics`, `--extractions-summary`, `--full-extractions`, or `--full` are set:

- emit summary-first JSON

If `--diagnostics` is set:

- include up to `limit` diagnostics

If `--full` is set:

- include all diagnostics and full extractions

### §16 `get` Implementation

#### §16.1 CLI Surface

Extend `Get` in [main.rs](/Users/morgan/code/anneal/src/main.rs#L217) with:

- `refs: bool`
- `context: bool`
- `trace: bool`
- `full: bool`
- `limit_edges: Option<usize>`

#### §16.2 Output Types

Keep current `GetOutput` for human mode if convenient.

Add JSON types:

- `GetSummaryJson`
- `GetContextJson`
- `GetTraceJson`

Minimum summary fields:

- metadata
- snippet
- `edge_counts`
- `sample_incoming`
- `sample_outgoing`
- `truncated_edges`

#### §16.3 Semantics

- default `get --json` = summary
- `--refs` = bounded refs view
- `--context` = compact agent briefing + compact facts
- `--trace` or `--full` = full edge detail

`limit_edges` default:

- `10`

### §17 `find` Implementation

#### §17.1 CLI Surface

Extend `Find` in [main.rs](/Users/morgan/code/anneal/src/main.rs#L239) with:

- `limit: Option<usize>`
- `offset: Option<usize>`
- `full: bool`
- `no_facets: bool`

#### §17.2 Search Rules

Existing identity substring semantics remain unchanged in this change.

Ordering:

- continue sorting matches by `id`

Bounding:

- if `full` is false, default `limit = 25`
- `offset` applies after ordering

Empty query:

- if `query.is_empty()` and `full == false` and no narrowing filter is present:
  - return a usage error

#### §17.3 Output Types

Redesign `FindOutput` to support:

- `_meta`
- `query`
- `matches`
- `total`
- `returned`
- `facets`

Facets should include:

- counts by `kind`
- counts by `status`

### §18 `map` Implementation

#### §18.1 CLI Surface

Extend `Map` in [main.rs](/Users/morgan/code/anneal/src/main.rs#L329) with:

- `render: Option<MapRender>`
- `nodes: bool`
- `edges: bool`
- `full: bool`
- `limit_nodes: Option<usize>`
- `limit_edges: Option<usize>`

Add:

```rust
#[derive(Clone, Copy, ValueEnum)]
enum MapRender {
    Summary,
    Text,
    Dot,
}
```

`render` default:

- `summary`

#### §18.2 Output Types

Replace the current single `MapOutput` contract with:

- `MapSummaryOutput`
- `MapRenderedOutput`
- optionally `MapNodeListOutput`
- optionally `MapEdgeListOutput`

Key rule:

- rendered graph content must never be included unless explicitly requested via `render=text|dot`

#### §18.3 Human Defaults

Change human `map` default to summary-first.

Full text rendering requires:

- `anneal map --render=text --full`

Focused neighborhood rendering remains:

- `anneal map --around=<handle> --depth=1`

#### §18.4 JSON Defaults

Default `map --json` returns only summary fields.

If `render=text|dot` is requested:

- return summary fields plus `rendered_content`
- require `--full` when rendering the unfocused graph

### §19 `status`, `diff`, `obligations`, `impact`, `init`

These commands remain mostly intact.

Required updates:

- add `_meta` for consistency where appropriate
- add `status --compact`
- add count fields to `impact`
- leave `diff`, `obligations`, and `init` behavior stable unless consistency requires minor JSON reshaping

### §20 Help Text and Examples

Update [main.rs](/Users/morgan/code/anneal/src/main.rs) help text:

- top-level OUTPUT section
- command descriptions
- examples

Specific changes:

- `map --json` must no longer claim "JSON with node/edge counts" unless that becomes true
- `get` help should mention `--refs`, `--context`, `--trace`
- `find` help should mention `--limit`
- `check` help should show bounded JSON examples rather than raw `--json`

### §21 Spec Alignment

Update [anneal-spec.md](/Users/morgan/code/anneal/.design/anneal-spec.md) after implementation is complete.

Required alignments:

- add progressive disclosure as an explicit principle near KB-P8
- formalize `status --compact`
- formalize summary-first JSON for `check`, `find`, `get`, and `map`
- keep the command surface coherent with actual implementation
- revise the "Dual output" section in §15.3 so JSON and human output are parallel contracts, not necessarily identical structures

### §22 Skill and Agent Guidance

After implementation:

- update `skills/anneal/SKILL.md`
- update `README.md`
- update `AGENTS.md` if needed

Guidance should follow the new contract:

- prefer `status --json --compact`
- prefer plain `check` for diagnostics
- use bounded JSON expansions intentionally
- treat `--full` as an escalation step, not a default path

## Part IV: Acceptance Criteria

### §23 Behavioral

1. `check --json` on Murail no longer emits MB-scale payloads by default.
2. `find "" --json` does not dump all matches unless the caller explicitly opts in.
3. `map --json` no longer includes full rendered graph content by default.
4. `get --json` caps or samples edges by default and reports edge counts.
5. Every bounded JSON output reports truncation explicitly.
6. Every bounded JSON output gives concrete expansion guidance.
7. `status --json --compact` exists and is suitable for agent session start.

### §24 Consistency

1. Help text matches actual behavior.
2. Spec matches actual behavior.
3. JSON remains stable and tool-compatible.
4. Human output remains readable and useful.

### §25 Performance and Safety

1. Default JSON outputs for risky commands should stay comfortably below the current audit figures by orders of magnitude.
2. Expansion flags must preserve access to full information when truly needed.
3. Compact defaults must not silently hide the fact that more data exists.

## Part V: Open Questions

### §26 Remaining Choices

These questions should be resolved during implementation, but do not block the main design.

1. Should `map` human default become summary-first immediately, or should only JSON change in the first implementation?
2. Should `--full` be standardized across all expandable commands, even when a command-specific flag like `--trace` is also present?
3. Should `find` facets be included by default, or only when requested?
4. Should `check --json` default diagnostics sample size be 5, 10, or 50?
5. Should schema versioning be global to the whole CLI, or per-command?

## Closing Note

The current `anneal` CLI already contains the right building blocks for agent-first knowledge work. The redesign proposed here does not change the tool's purpose. It changes the disclosure contract so that the first answer is small, useful, and safe, while deeper detail remains explicitly reachable.

That is a better fit for disconnected intelligences, better aligned with the tool's own theory of orientation, and more consistent with `anneal` as a convergence instrument rather than a raw state dump.
