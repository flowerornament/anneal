---
status: draft
date: 2026-04-30
description: >
  Design proposal for anneal's agent ergonomics and retrieval layer, based on
  the qmd comparison, command-output review, existing CLI UX specs, and
  research-graph findings about multi-path retrieval and graph sensemaking.
depends-on: anneal-spec.md
note: >
  Partially subsumed by 2026-05-03-language-redesign.md. The NDJSON I/O
  contract, bounded results, and result-card shape are absorbed into
  §13/§21 of the language redesign. The orthogonal ideas — content
  search (`anneal search`), context annotations, and MCP transport —
  remain open and are tracked in bd as the "agent ergonomics" track
  outside the v2.0 language epic.
---

# Agent Ergonomics and Retrieval Layer Spec — 2026-04-30

## Summary

Anneal should learn from qmd without becoming qmd.

qmd is excellent at helping an agent find and retrieve the right text quickly:
semantic search, keyword search, result docids, line-bearing paths,
hierarchical context, batch retrieval, multiple output formats, MCP, and
status output that names the live retrieval substrate.

anneal is excellent at a different layer: typed handles, graph structure,
convergence state, diagnostics, obligations, impact, orientation, and
progressive disclosure. That layer should remain deterministic, fast, and
boring.

The target is a two-path system:

- **Retrieval path:** find relevant source bytes quickly, even when the agent
  does not know the corpus vocabulary.
- **Structure path:** explain corpus health, graph relationships, convergence,
  and edit blast radius.

The two paths should reinforce each other. Search results should carry graph
metadata. Graph outputs should provide retrieval handles. Neither should force
an arriving agent to consume unbounded output before it knows what to inspect.

## Evidence

### qmd field test

A separate qmd index was built over the qmd source checkout and this anneal
checkout:

- 115 files indexed
- 1,155 embedded chunks
- collection contexts attached for both qmd and anneal
- AST-aware chunking enabled for the qmd 2.1.0 source checkout

Useful qmd behaviors:

- `qmd status` reports index path, size, document/vector counts, collection
  contexts, model configuration, AST-chunking state, examples, and tips.
- `qmd search` result cards include `qmd://path:line`, `#docid`, title,
  context, score, and a snippet.
- `qmd --files` emits a compact selection list:
  `docid,score,path,context`.
- `qmd get #docid` and `qmd get qmd://path:line -l N` make follow-up
  retrieval cheap.
- `qmd multi-get` batches selected documents and supports markdown, JSON, CSV,
  XML, and files output.
- qmd's current source help is a compact operator card: primary commands,
  query grammar, examples, MCP, skill install, and tuning flags on one screen.

Operational cautions:

- The installed local qmd binary reported `qmd 1.0.7`, while the refreshed qmd
  source checkout was `2.1.0`.
- The installed qmd hybrid query path crashed twice during reranking on an
  over-context candidate and emitted a native Metal/llama backtrace.
- qmd 2.1.0 source improves the surface with `--no-rerank`,
  `--candidate-limit`, intent-aware query syntax, and richer help, but source
  mode still surfaced code frames for some errors.

The lesson is not "avoid semantic search." The lesson is "semantic search must
be optional, bounded, and failure-contained."

### anneal field test

Anneal's current design strengths showed up clearly:

- `anneal status` is compact and safe.
- `anneal status --json --compact` has an explicit `_meta` contract with
  detail level, truncation, and expansion hints.
- `anneal find` and `anneal orient` emit `Try` hints for the next safe
  expansion.
- `anneal orient` already speaks in a token-budgeted reading-list form.
- `anneal check --scope=active` on this corpus reported `0 errors,
  0 warnings`.

Current weaknesses:

- `anneal find` is identity search only. It cannot find a concept that is not
  already in the handle text.
- Search-style outputs do not have qmd-like result cards with line, score,
  context, and a stable short id.
- `anneal status` is so compact that it omits operational hints that would help
  an arriving agent decide what to do next.
- There is no MCP transport yet.
- There is no first-class way to attach human-written context to an area or
  path outside ad hoc frontmatter.

## qmd Lessons to Steal

### Configuration

qmd's configuration model is useful because it names retrieval collections and
lets a human attach a short context string to each collection. The context is
then visible in status and result output, so the configuration pays off during
every later query.

Anneal should steal the shape, not the exact machinery:

- named corpus areas and path prefixes
- human context annotations
- explicit retrieval backend status
- config that is visible through `status`, not hidden until failure

Anneal should avoid requiring a global daemon or global index for core
commands. A repo-local corpus should remain usable after clone with no setup
beyond the binary and `anneal.toml`.

### README and Docs

qmd's README is successful because it sells the task before the architecture:
index, search, retrieve, batch retrieve, then integrate with agents. The docs
make it obvious what an agent should type first.

Anneal's docs should keep the deeper convergence model, but the first screen
of docs should lead with the daily agent loop:

1. orient
2. search or find
3. get selected context
4. check health
5. inspect impact before editing

The detailed graph model should remain available after the operator has a
working path through the tool.

### Command Output and Styling

qmd's best terminal idea is the retrieval card:

- stable selectable id
- path with line
- score
- title/context
- bounded snippet

Anneal should combine that card shape with its existing output discipline:

- no unbounded default dumps
- `Try` blocks for safe expansion
- command-specific help
- JSON with `_meta`
- plain mode with no ANSI or decorative glyphs

qmd's progress styling is pleasant for humans but awkward in merged agent logs
because progress frames and OSC escapes can leak into captured output. Anneal
should keep progress quiet by default when stdout/stderr are not interactive,
and reserve richer terminal effects for real TTYs.

### Help

qmd's newer top-level help acts like an operator card: command list, query
syntax, examples, integrations, and tuning flags are visible together. That is
valuable for agents because `--help` can be the first retrieval step.

Anneal should preserve command-specific help, but the top-level help should
make the primary loop unmistakable:

```text
First moves:
  anneal status
  anneal orient --budget=12k
  anneal search "concept"
  anneal get <handle-or-id>
  anneal check --scope=active
```

Help should describe task intent before flag taxonomy.

### Capabilities

The strongest qmd capabilities for anneal to adopt are:

- content search separate from handle search
- line-bearing retrieval
- batch retrieval
- context annotations
- result ids for follow-up commands
- MCP transport over the same retrieval surface
- explicit local-model availability and fallback controls

The weakest capabilities to copy directly are:

- mandatory embedding or reranking paths
- native-model stack traces in normal output
- source-mode code frames as user-facing error UX
- raw JSON arrays without progressive-disclosure metadata

### Codebase Shape

qmd's implementation is pragmatic: CLI command parsing, persistent store,
retrieval/query logic, AST chunking, and MCP wrapping are close enough that a
small codebase can ship a complete agent retrieval experience.

Anneal should not copy that layering wholesale. Its stronger boundary is:

- parse and extraction own markdown interpretation
- graph and handle own identities
- lattice and checks own corpus health
- output and CLI own progressive disclosure
- future retrieval modules own search indexes and scoring
- MCP wraps CLI-equivalent contracts

The important codebase lesson is product cohesion: every qmd feature points
back to "find the right context, then retrieve it." Every anneal retrieval
feature should point back to "understand the corpus, then edit with impact
awareness."

## Research Frame

The research graph supports a hybrid direction:

- `multi-faceted memory note structure enables both semantic search and
  categorical retrieval`: retrieval schema is an interface, not just storage.
  Embedding search and structured lookup solve different problems.
- `LLM-generated contextual descriptions make implicit knowledge in
  interactions explicit and retrievable`: context annotations are not
  decoration; they create retrieval handles.
- `graph-based community structure enables global sensemaking that vector
  retrieval cannot`: vector search is a local lookup mechanism; graph structure
  is needed for corpus-wide sensemaking.
- `progressive disclosure of complexity fails when feature accumulation is not
  actively prevented`: anneal should add a small set of general mechanisms,
  not one-off convenience flags that drift from the existing output contract.

This argues for a layered design:

1. Keep anneal's graph and convergence model as the authority for structure.
2. Add retrieval handles and context annotations as first-class graph-adjacent
   metadata.
3. Add semantic search as an optional derived cache, not a required runtime
   dependency.
4. Preserve progressive disclosure and bounded JSON as the command contract.

## Design Principles

### P1. Agent Ergonomics Means Fast Correct Next Action

Agent ergonomics is not "pretty terminal output." It is the probability that an
arriving agent can choose the next correct action with minimal context and
minimal ceremony.

Every broad command should answer:

- what exists
- why it matters
- what to inspect next
- how to expand safely

### P2. Result Cards Are Retrieval Interfaces

Any command that returns candidate documents or handles should expose enough
information for immediate follow-up:

- stable short id
- canonical handle or path
- line when available
- kind/status/area
- context description
- score or reason
- bounded snippet
- next retrieval command

The output should make selection cheap and reversible:

```
  #a13f9c  0.91  search  qmd://docs/api.md:42
  API reference  active  docs/
      Context: REST API reference and examples
      The authentication flow uses...

  Try  anneal get #a13f9c -l 80
```

Short ids must be aliases for existing identities, not replacements for
handles. A handle remains the durable graph identity. A short id is a
retrieval affordance.

### P3. Dual Retrieval Beats Single Retrieval

Anneal should distinguish:

- exact/structured retrieval: handles, namespaces, statuses, areas, edge kinds
- content retrieval: text snippets and file bodies
- semantic retrieval: concept-level similarity
- graph retrieval: upstream/downstream/impact/community shape

No one path should pretend to solve all retrieval needs.

### P4. Context Annotations Are First-Class

qmd's `context add` works because it lets a human summarize a subtree once and
then returns that summary with every relevant result.

Anneal should support context annotations for:

- corpus root
- areas
- path prefixes
- specific files
- possibly concern groups

These annotations should feed:

- `status`
- `areas`
- `orient`
- `search`
- `find --context`
- MCP server instructions
- result cards

### P5. MCP Is Transport, Not New Semantics

An MCP server should wrap the same command contracts, not invent parallel
behavior. The CLI remains the executable spec. MCP tools should expose
structured versions of the existing surfaces.

### P6. Local LLM Features Must Degrade Gracefully

Semantic search and reranking should never be required for:

- `status`
- `check`
- `get`
- `find`
- `orient`
- `impact`
- `garden`

If embeddings, models, or native GPU paths fail, the tool should fall back to
exact/BM25-style retrieval or return a short actionable error. It must not emit
native stack traces in normal mode.

## Proposed Command Surface

### `anneal search`

Add a content retrieval command separate from `find`.

`find` remains identity search:

```bash
anneal find OQ --kind=label
anneal find --status=active --kind=file --context
```

`search` answers content/concept discovery:

```bash
anneal search "MCP server for agent tools"
anneal search "semantic search over knowledge corpus" --area=implementation
anneal search "how do I recover after changing this spec?" --semantic
anneal search "context annotations retrieval handles" --files
```

Output modes:

- default: bounded result cards
- `--files`: `id,score,path,line,context`
- `--json`: bounded result objects with `_meta`
- `--md`: markdown bundle suitable for agent context
- `--full`: explicit full expansion

Initial backend:

- parse all active markdown files
- exact term/phrase scoring over title, frontmatter summaries, headings, body
- return snippets with line numbers
- no model dependency

Optional backend:

- derived embedding cache under anneal state
- explicit `anneal search --semantic`
- graceful fallback when unavailable

Open naming question: `search` is clearer than extending `find`, because the
existing `find` contract is handle identity search.

### `anneal context`

Add a context annotation surface.

Possible CLI:

```bash
anneal context list
anneal context add / "Anneal design corpus and CLI implementation notes"
anneal context add area:(root) "Core product specs and CLI UX design"
anneal context add 2026-04-17-cli-ux-audit-v2.md "Presentation-focused CLI UX audit"
anneal context rm area:(root)
```

Storage options:

1. `anneal.toml` sections:

   ```toml
   [context]
   "/" = "Anneal design corpus and CLI implementation notes"
   "(root)/" = "Core product specs and CLI UX design"
   ```

2. Frontmatter:

   ```yaml
   context: Presentation-focused CLI UX audit
   ```

3. Dedicated `.design/context.toml`.

Recommendation: start with `anneal.toml` and frontmatter. Do not introduce a
new sidecar file until there is a demonstrated need.

### `anneal get`

Extend retrieval ergonomics:

```bash
anneal get #a13f9c
anneal get anneal-spec.md:120 -l 80
anneal get anneal-spec.md --from 120 -l 80
anneal get a.md b.md c.md --md
```

Required behavior:

- short ids resolve to canonical handles from the current result-id table
- line slices work for file handles
- missing handles show a short error and possible suggestions
- `--md` emits agent-ready context bundles

Short id design:

- compute from canonical identity, not source body
- display enough characters to avoid collisions in the current graph
- if collision occurs, lengthen both visible ids
- never store ids as durable state unless a future use case requires it

### `anneal status`

Keep current compactness, but add optional operational hints.

Default healthy output can remain nearly unchanged:

```text
  Corpus       10 files, 397 handles, 10 edges
               397 active, 0 terminal

  Health       0 errors, 0 warnings
  Convergence  holding, resolution +0, creation +0, obligations +0
```

When there are configured contexts or search indexes, include compact status:

```text
  Retrieval    exact ready, semantic not indexed
  Context      3 annotations
```

When there is an obvious next action, emit `Try`:

```text
  Try  anneal check          inspect diagnostics
       anneal search "..."   discover content by concept
```

Do not make `status` a full operations dashboard. qmd's status is richer
because it owns a persistent search index. anneal should show retrieval state
only when retrieval state exists.

### `anneal mcp`

Add a thin MCP server over existing contracts.

Initial tools:

- `status`
- `areas`
- `orient`
- `search`
- `get`
- `multi_get`
- `find`
- `query`
- `impact`
- `garden`
- `explain`

Server instructions should be generated dynamically from current corpus state:

- corpus root
- files/handles/edges
- active/terminal partition
- areas and context annotations
- search backend availability
- recommended first moves

Do not expose tools that are not actually callable. A small truthful tool set
is better than a broad misleading one.

## Output Contract

Anneal should keep the existing progressive-disclosure JSON contract:

```json
{
  "_meta": {
    "schema_version": 2,
    "detail": "sample",
    "truncated": true,
    "returned": 25,
    "total": 842,
    "expand": ["--limit 100", "--full"]
  }
}
```

New retrieval outputs should follow the same shape rather than qmd's raw array
shape.

Human output should use the existing anneal printer conventions:

- heading with count
- aligned rows
- dim snippets
- path tone for paths
- number tone for scores/counts
- `Try` blocks for expansion
- no color or glyphs in `--plain`
- no ANSI in piped plain output

Result-card shape:

```text
  Search results (5)
  showing 5 of 18, offset 0

  #a13f9c  0.91  anneal-spec.md:927  file  draft
      Context: Product and implementation spec for anneal
      Semantic search. `anneal find` uses identity-substring matching...

  Try  anneal get #a13f9c -l 80  retrieve
       --limit 25                expand
       --offset 5                next page
       --full                    all results
```

Error shape:

```text
handle not found: missing-file.md
try: anneal search "missing-file" --files
```

Normal mode should never show language runtime stack traces. Debug stack traces
can live behind `RUST_BACKTRACE=1` or an explicit debug flag.

## Implementation Phases

### Phase 1: Retrieval Result Cards Without New Indexing

Add shared result-card primitives and short-id resolution for current command
outputs:

- `find`
- `query handles`
- `orient`
- `get`

This phase proves the output contract before adding semantic complexity.

### Phase 2: Context Annotations

Add config/frontmatter context annotations and surface them in:

- `areas`
- `orient`
- `find --context`
- `get --context`
- future `search`

This is the highest qmd-style value with the least operational risk.

### Phase 3: Content Search

Add `anneal search` with exact/lexical scoring and line-bearing snippets.

No model dependency. No persistent index required unless measurement shows the
plain scan is too slow for real corpora.

### Phase 4: Optional Semantic Cache

Add semantic retrieval as an optional derived cache.

Possible backends:

- qmd interop as an experimental adapter
- local sqlite/vector cache owned by anneal
- external tool integration through MCP or shell

Requirements:

- explicit availability in `status`
- bounded candidates
- no required reranking
- no native stack traces
- deterministic fallback

### Phase 5: MCP Server

Expose existing contracts as tools. Start read-only.

The first version should prefer usefulness over completeness. If a CLI command
has no bounded JSON contract, do not expose it until it does.

### Phase 6: Retrieval Quality Fixtures

Add fixtures that test whether agents can find the right file from natural
language tasks.

Example fixture row:

```yaml
query: "how does anneal keep JSON output from flooding agent context?"
expected:
  - 2026-04-02-progressive-disclosure-output-spec.md
  - 2026-04-02-cli-output-audit.md
must_not_require:
  - --full
```

Metrics:

- precision at 5
- expected file present
- whether a safe follow-up command is printed
- whether output stays under a byte/token budget

## Non-Goals

- Do not replace anneal's graph model with vector search.
- Do not require qmd, node, bun, llama.cpp, or GPU access for normal anneal use.
- Do not add unbounded JSON outputs.
- Do not make MCP semantics diverge from CLI semantics.
- Do not turn `status` into a full qmd-style index dashboard unless anneal owns
  comparable retrieval state.
- Do not make short ids canonical durable handles.

## Open Questions

### OQ1. Should `search` be lexical-first or semantic-first?

Recommendation: lexical-first, semantic optional. This preserves install
simplicity and avoids making model setup part of the base experience.

### OQ2. Where should context annotations live?

Recommendation: start with `anneal.toml` plus file frontmatter. A dedicated
context sidecar can come later if config becomes crowded.

### OQ3. Should anneal interoperate with qmd directly?

Possibly, but only as an adapter. qmd is already a strong retrieval engine, and
reusing it could be valuable for local experiments. The core anneal UX should
not depend on qmd being installed.

### OQ4. Should result short ids be stable across invocations?

Recommendation: stable for a given canonical handle identity, collision-aware
within the current graph. They do not need separate storage.

### OQ5. Does anneal need graph community summaries?

Not yet. The graph-retrieval research argues that global sensemaking needs
graph structure, but anneal already has area/orient/garden as simpler graph
summaries. Community summaries are a later feature if real corpora outgrow
area topology.

## Acceptance Criteria

A future implementation satisfies this spec when:

- An agent can run `anneal status` and see whether retrieval/context features
  are available.
- An agent can run `anneal search <concept>` and get bounded result cards with
  stable short ids, line-bearing paths, context, scores, snippets, and `Try`
  hints.
- An agent can retrieve selected results with `anneal get #id`.
- Context annotations appear consistently in result cards and orientation
  outputs.
- JSON output remains bounded and carries `_meta`.
- All local LLM or embedding failures degrade to short actionable errors.
- MCP exposes the same bounded contracts as the CLI.
- Retrieval quality fixtures cover common agent tasks.
