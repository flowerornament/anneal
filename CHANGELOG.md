# Changelog

All notable changes to `anneal` are documented in this file.

## v0.18.0 - 2026-06-08

Retrieval follows the corpus's currency signal.

`context`, `search`, and `ranked_anchor` now factor whether a document has
been superseded. Current operative specs surface ahead of stale predecessors,
while superseded material stays reachable for "what changed?" work. Context
and search hits show each document's currency disposition, lifecycle status,
and age so orientation can distinguish live framing from stale-but-relevant or
newest-but-draft material.

### Added

- Retrieval currency ranks current operative successors ahead of superseded
  documents across `context`, `search`, and `ranked_anchor`, while keeping
  superseded handles accessible.

### Changed

- Context and search hit output includes currency disposition
  (`current`, `current_head`, or `superseded`), lifecycle status, and handle age.
- Currency is derived from displacement edges; lifecycle remains a separate
  status-band signal, and the two compose only at ranking.

## v0.17.0 - 2026-06-07

anneal runs on one planned executor, with the same answers and a smaller core.

The runtime now uses a single planned execution path for global rules,
query-local rules, recursion, aggregates, time scopes, and provenance. The
former interpreted evaluator is retired, and the core is decomposed into
focused `vm/` modules for execution, fixpoint scheduling, frames, provenance,
and time overlays. Results remain byte-identical across the query and verb
surface; the release is mostly internal, with a few user-facing correctness
fixes for honest diagnostics and graceful query errors.

### Fixed

- Parseable-but-unplannable queries report a normal error instead of panicking
  the CLI.
- External references, unresolved cross-corpus wikilinks, URI-scheme links, and
  source-code citations such as `file.rs:123` classify as external references
  rather than broken corpus links, so `anneal check` stays honest for docs that
  cite external or code targets.

### Internal

- The runtime has one planned executor. The interpreted evaluator and its
  `Binding`/`TracedBinding` path are retired; recursion, `--explain`, and
  query-local rules execute through the same planned engine as global prelude
  rules.
- `anneal-core` is split into focused VM modules: `execute`, `fixpoint`,
  `frame`, `provenance`, and `view`, with the runtime facade keeping analysis
  and CLI/MCP entry points outside the executor.
- `just check` includes `check-arch`, an architecture gate for the VM boundary
  and workspace crate graph.

## v0.16.0 - 2026-06-04

anneal evaluates over a relational tuple runtime — markedly faster, identical answers.

The Datalog runtime is rebuilt on a logical/physical split: queries and output
keep names and readable rows, while the evaluator runs on interned symbols,
schema-addressed tuple rows, and overlay-based time scoping. This removes the
per-query cost of materializing string-keyed `BTreeMap` rows, deep-cloning the
database to view a snapshot, and allocating a map per partial match. Cold-start
`status` on a 15MB reference corpus drops from ~3.1s to ~1.4s (about 2.2×) with
~54% less allocation churn. Results are unchanged — every phase was gated by
differential comparison against the prior evaluator, and the shipped engine is
byte-identical to the previous release across the full query and verb surface.

### Changed

- The runtime stores corpus facts as interned tuples (`TupleDb`) addressed by
  relation and field identifiers, replacing the string-keyed `BTreeMap` row
  store. Repeated strings — handle ids, statuses, kinds, edge kinds, paths —
  collapse to a single interned symbol.
- Time-scoped queries (`at("snapshot:last")`) compose a borrowed overlay over the
  base tuples instead of deep-cloning the database per scope, so snapshot and
  convergence queries no longer pay a full-store copy.
- Query evaluation binds into a compact slot vector rather than a per-match
  `BTreeMap`. Stored-relation output remains deterministic, and the logical
  `Value`/row representation is reconstructed only at the output boundary, so
  text, JSON, and `--explain` are unchanged.

### Internal

- The physical substrate lives behind `anneal-core`'s `ir/` (typed identifiers,
  interner, schema registry) and `vm/` (tuple value, store) modules, hidden from
  the CLI and MCP surfaces. Engine internals do not leak past the logical
  boundary.
- The transitional feature flags used to differential the rewrite are retired;
  the tuple substrate is the single unconditional eval path. Architecture is
  mapped in `.design/2026-06-04-runtime-architecture.md`.

## v0.15.2 - 2026-06-02

Ranked queries read in rank order, and stored output is deterministic.

Queries declare their own result order with `order by`, so ranked predicates
like `recent_frontier` and `ranked_anchor` arrive as reading lists — rank 1
first — instead of in binding order. Ordering is a projection-boundary language
primitive: any `?` query can use it, it composes with `--limit` for a true
top-N, and a query without `order by` is byte-identical to before. Raw
stored-relation output is now stable run-to-run, closing a determinism gap that
made repeated reads of the same query reorder their tail.

### Added

- `order by <expr> [asc|desc] [, <expr> [asc|desc]]*` on the top-level `?`
  query: a stable sort of the result at the projection boundary. Keys are
  eval-supported expressions over the result's bound variables, default
  direction is ascending, and ties preserve prior order. `order by … --limit N`
  selects a genuine top-N rather than truncating an arbitrary order. An order
  key over an unbound variable fails static analysis before any rows are
  emitted.

### Changed

- `anneal status` pointers, the README and skill cold-start ladders, and
  `describe runtime` orientation queries end with `order by rank asc`, so the
  copy-runnable reading lists arrive top-down and teach the ordering primitive
  by example.

### Fixed

- Stored-relation query output is deterministic. Source-owned relations are
  canonicalized after merge and runtime config and snapshot relations on
  replacement, each by a semantic key with full identity tie-breakers, so
  queries like `? *handle{id: h}.` return byte-identical results across runs.

## v0.15.1 - 2026-06-02

anneal status is a goal-less orientation dashboard.

`anneal status` now renders aggregate corpus vital signs — scale, lifecycle
coverage, pipeline histogram, convergence counts, and health counts — before
handing agents copy-runnable queries for reading and work. Goal-less
orientation lives in the language: `recent_frontier` ranks recently authored
files for first reading, while `anchor` and `ranked_anchor` surface the durable
spine of authoritative and high-signal files. Once a goal is known,
`anneal context GOAL` remains the focused retrieval surface.

### Added

- `recent_frontier(h, rank, recency)`: a recency-ranked reading frontier for
  cold agents. Authored dates from frontmatter or filenames dominate; git mtime
  is only a fallback for files without authored dates. Terminal and superseded
  files are excluded, statusless files remain eligible, and curated hubs are
  de-prioritized so they do not crowd out newly authored work.
- `anchor(h, score, why)` and `ranked_anchor(h, rank, score, why)`: durable
  orientation predicates for the corpus spine. `anchor` remains uncapped for
  composition; `ranked_anchor` is the ranked projection used by status pointers
  and read-first examples.

### Changed

- `anneal status` is now an aggregate dashboard instead of a per-handle garden:
  scale and lifecycle coverage, pipeline histogram, convergence counts, health
  counts, and always-visible Read-first / Work query pointers.
- Help, `describe runtime`, README, and the bundled anneal skill teach the
  cold-start ladder as `status` → `recent_frontier` / `ranked_anchor` →
  `context GOAL`.
- The retired `orient` recovery message points at the same status-led
  orientation ladder.

## v0.15.0 - 2026-06-01

anneal surfaces spec→code drift, and gets markedly faster.

This release adds spec→code coherence — a live, code-authoritative spec that
cites a code path which existed in history but is now gone is flagged as drift —
and lands a round of runtime performance work that cuts cold-start cost on large
corpora by roughly 3.6×.

### Added

- `W006` / `spec_code_drift`: a live spec that asserts current code and cites a
  path which existed in HEAD history but is now missing on disk is flagged. The
  signal is git-history-gated (a path that was never tracked here — an
  illustration, an external-codebase study, a forward plan — is not drift) and
  status-gated by `asserts_code`, so it surfaces real intent/implementation
  divergence rather than noise.
- `target_history_status` metadata (`present`, `absent`, `unavailable`) on code
  external handles, so W006's drift decision is auditable.
- `config convergence { asserts_code([...]). }` and the `asserts_code(status)`
  predicate, so each corpus declares which lifecycle statuses make claims about
  current code. Unconfigured, it defaults to active statuses minus the
  aspirational tier (`plan`, `research`, `reference`, `exploratory`).
- A `cold_start_honesty` integration-test harness covering the CR-R12
  degenerate-input cases and the W006 git-history behavior.

### Changed

- Cold start is substantially faster on large corpora (~9.4s → ~2.6s on a
  15MB reference corpus): git mtime discovery is batched into one pass instead
  of one subprocess per file, the search index builds on demand, code-target
  history probing is demand-driven, and the global fixpoint is scoped to the
  query's dependencies. Results are unchanged — verified by differential
  comparison against the prior evaluator.
- Markdown extraction reuses one parsed payload per file instead of re-reading
  for facts, frontmatter, spans, and revision hashing.
- Context ranking policy now lives in `anneal-core::ranking` rather than being
  split between the CLI and core, so `search` and `context` ranking cannot
  drift apart.

### Internal

- Centralized code-target metadata keys and relative-path policy behind shared
  core helpers; folded `potential_weight` into the config schema; replaced
  quadratic ordered-edge emission with bucketed indexing.

## v0.14.1 - 2026-05-30

anneal signals degenerate input instead of answering confidently over it.

This release hardens the cold-agent contract: when a question cannot be
answered meaningfully from the corpus as configured, anneal now says so rather
than returning a confident-looking empty or wrong result. The master spec
records this as Rule CR-R12 (degenerate-input honesty), and a cold-start test
harness guards each case.

### Fixed

- `anneal` now resolves the corpus root by walking up from the working
  directory to the nearest `.design`, `docs`, or `anneal.dl`, so running from a
  subdirectory finds the corpus instead of returning an empty result. When no
  marked root is found it reports `no marked corpus root found above <dir>;
  scanning current directory` rather than silently scanning the current
  directory.
- Search ranking weights specificity: rarer query terms count for more, so a
  section whose heading matches the query outranks short label handles and
  incidental body mentions that previously tied at the top score.
- `anneal status` lists each handle in exactly one section. A handle that is
  both blocked and holding now appears only under blocked.
- `anneal status` reasons are backed by the diagnostic stream: a handle blocked
  for `confidence_gap` now has a matching `W002` diagnostic, instead of a
  reason no query could corroborate.
- A status used by handles but absent from both the active and terminal
  partitions now raises a `W005` lifecycle-config-gap warning instead of being
  silently dropped from the convergence frontier. A `convergence.ordering` that
  does not end in a terminal status warns for the same reason.
- `S003` pipeline-stall no longer claims `based_on_history` when there is no
  snapshot baseline to compare against.

### Changed

- `anneal context --format=json` streams NDJSON event rows (`goal`, `hit`,
  `span`, `neighbor`), matching `search`, `status`, and `eval`, so one parser
  handles every command.
- `anneal context` returns ranked hits, span metadata, and neighborhood by
  default; pass `--read-spans` to inline matched span bodies.
- Markdown headings now carry stable structural span ids
  (`file.md#h/heading-slug-path`).

### Added

- A `cold_start_honesty` integration-test harness covers the degenerate-input
  cases above, so CR-R12 regressions fail in CI.

### Internal

- Test fixtures use a synthetic sample corpus instead of an external private
  corpus.

## v0.14.0 - 2026-05-29

anneal calibrates the signal, simplifies the substrate, and sharpens retrieval.

This release turns the v0.13 language surface into a sharper convergence
instrument: flow is explicit, potential can be tuned honestly, freshness noise
is quieter by default, heading structure moves into spans instead of graph
handles, and the describe/help/docs surface teaches the current vocabulary
instead of retired command habits.

### Added

- Added the convergence flow vocabulary:
  `holding(h)`, `regressed(h)`, `re_opened(h)`, `drifting(h)`, and
  `flow(h, direction)`. Flow directions are exactly `advancing`, `holding`,
  and `drifting`; settled handles remain outside flow by design.
- Added project-level potential calibration:
  `config potential_weight { freshness_decay(0). undischarged(8). }`
  overrides default signal weights, and
  `effective_potential_weight(source, weight)` shows the weights actually used
  by `potential`.
- `describe convergence` is now the multi-section teaching card for the
  annealing model: the act, vocabulary, flow leaves, and tuning path.
- `describe runtime` now distinguishes snapshots, generations, and trails as
  separate history concepts.
- Unknown-predicate errors now offer arity-aware suggestions for close schema
  matches and route non-typo misses toward `schema` and `describe convergence`.
- Markdown headings now emit structural `*span` rows with ids shaped like
  `file.md#h/heading/path` instead of graph handles. Line numbers remain on the
  span for sorting, excerpts, and retrieval.
- Retrieval now lands on heading spans: `search` and `context` return
  `heading_path` metadata for span hits, and `anneal read <H> --span-id <ID>`
  reads a matched heading span directly.
- Markdown body code references such as `lib/app.ex:10-20` now become
  external handles connected by ordinary `Cites` edges. Target path/range data
  lives in `external_class`, `target_path`, `target_start_line`, and
  `target_end_line` metadata, keeping `*handle.file`/`line` as discovery
  location fields.
- `config code_path_root { root(["web", "bin"]). }` adds project-specific code
  roots to the default `crates`, `lib`, `src`, `app`, `test`, `priv`, and
  `native` body-reference scanner.
- `config search_boost { status("authoritative", 0.08). hub(0.01). }`
  tunes search ranking boosts by lifecycle status and hub degree.
- `describe '*meta'` now teaches the three metadata key categories: standard,
  source, and frontmatter. `describe external_class` and the `target_*`
  metadata keys document code-reference target metadata.

### Changed

- Behavior change: the default `freshness_decay` potential weight is now `1`
  instead of `2`. In the calibration corpus, this sharpens the energy>=3 work
  pool from 37 handles to 3, keeping old-but-legitimate reference material from
  drowning stronger correctness and lifecycle signals. Projects that want
  freshness to pull harder can override it with `config potential_weight`.
- `potential(h, energy)` is the canonical raw-energy predicate.
  `work_candidate(h, energy)` remains as a deprecated alias through v0.14 and
  is scheduled for retirement in v0.15.
- `blocker(h, energy, source)` describe cards now teach the
  `primary_entropy(h, source)` join for one blocker row per handle.
- `changed_within(h, days)` describe cards now teach the
  `*handle{kind: "file"}` join when agents want file-granular recent changes.
- Behavior change: markdown headings no longer appear as
  `*handle{kind: "section"}` rows. Querying the retired section kind returns
  zero rows with a recovery warning pointing at `*span`, and section-shaped
  handle lookups recover toward a span query.
- Behavior change: when heading body spans exist for a file, broad body search
  suppresses the synthetic full-file body span so the ranked result points at
  the matching section rather than the whole document.
- Behavior change: search ranking now includes status and hub boosts after
  lexical scoring. Authoritative/current/stable handles rank above active or
  draft matches by default, and highly cited handles get a bounded
  incoming-edge boost.
- Behavior change: `anneal context --format=json` now emits NDJSON event rows
  (`goal`, `hit`, `span`, `neighbor`) instead of one top-level JSON object, so
  machine consumers can parse it like `search`, `status`, and `eval`.
- Behavior change: `anneal context` no longer inlines matched span bodies by
  default. It returns ranked hits, span metadata, and neighborhood first; pass
  `--read-spans` when inline matched bodies are worth the extra output.
- `anneal handle <H> --impact` and `impact("H", affected, depth)` now use the
  same configured reverse-dependency traversal, so direct handle-impact rows
  match `impact("H", _, 1)`.
- `anneal handle` groups outgoing/incoming edges by kind and renders in-repo
  code references in a dedicated `Code references` section.
- The `anneal status` Convergence header now renders `open=N` instead of
  `work=N`. The label was the odd grammar in a header that mixes state
  (`broken`/`blocked`/`open`) and motion (`advancing`/`holding`/`drifting`).
- Retirement recovery messages for `top_work`, `blocked_row`, `recent`,
  `broken`, and the v0.13 hidden runtime nouns now ship working query examples
  with bound output variables, instead of syntactically valid queries that
  return empty `{}` rows.
- Empty-binding query results now emit a hint pointing at how to extract
  values. Queries that produce rows but no output columns no longer render as a
  silent wall of `{}` lines.
- When snapshot history is not yet populated, `anneal status` renders flow
  signals as `-` and emits a note that the baseline accumulates on the next
  status run. This replaces the silent `holding=0` that looked like a
  real-but-empty signal.
- README, AGENTS.md, CLAUDE.md, the bundled `anneal` skill, top-level help,
  and `help eval` now teach the v0.14 surface, the context/grep/eval retrieval
  split, and schema discovery through helpful errors.
- `describe` text output now keeps contributor source paths out of the default
  teaching cards. The same data remains queryable with `source_of(name, file,
  lines)`.

### Removed

- Retired the v0.13 deprecated predicate aliases `top_work(h, energy)`,
  `blocked_row(h, energy, source)`, and `recent(h, days)`. Static analysis now
  fails loudly with replacements: `frontier`, `blocker`, and `changed_within`.

## v0.13.1 - 2026-05-28

### Fixed

- README and the bundled agent briefing no longer teach `anneal context
  --limit`; `context` uses `--hits` for search winners.
- `area_error_count(area, errors)` now emits zero-error rows, so
  `area_health(area, grade, files, errors, cross_edges)` returns every
  observed area instead of only areas with error diagnostics.
- `missing_frontmatter_file(h, dir, file)` now matches the W003 diagnostic
  rule by filtering to high-adoption directories, rather than returning every
  raw file with missing status frontmatter.
- Snapshot-history `pipeline_stall(...)` rows now preserve `next_status` in
  S003 evidence instead of emitting `null`.

## v0.13.0 - 2026-05-28

anneal becomes the language it has always claimed to be.

This release completes the v0.13.0 simplification arc: the visible CLI narrows
to the nine-command Code Mode surface, convergence history becomes automatic,
recent-change recovery becomes compositional, and retired workflows fail loudly
with exact replacement queries instead of silently preserving parallel command
dialects.

### Added

- `anneal status` now records automatic bounded snapshot history for
  convergence tracking. Repeated unchanged status reads coalesce instead of
  growing history, and legacy aggregate history rows are preserved during the
  transition.
- Runtime queries can use `git_mtime(file, instant)` and
  `changed_within(h, days)` to compose session-recovery workflows without
  adding a global `--since` flag.
- `work_candidate(h, energy)` is now the canonical raw-energy predicate,
  `frontier(h, energy)` is the global top projection, and
  `blocker(h, energy, source)` explains stalled handles.
- Diagnostic codes such as `E001`, `W001`, and `S001` are first-class
  `describe` targets that route to diagnostic-rule predicates and Common join
  examples.
- Diagnostic-rule predicates have deeper `describe` cards, including
  `broken_reference`, `undischarged_obligation`, `stale_reference`,
  `confidence_gap`, `missing_frontmatter_file`, `implausible_ref`,
  `orphaned_handle`, `pipeline_stall`, `abandoned_namespace`, and `top_pair`.
- `anneal handle <HANDLE> --impact` shows direct and indirect reverse
  dependencies from the same traversal policy as the legacy `impact` command.
- `anneal help agent` is now the canonical bundled agent briefing surface.
  The hidden `anneal prime` alias still emits the same briefing for installed
  skill loaders and existing muscle memory.
- `describe` examples and Common joins now show projected `Output: <columns>`
  hints so agents can see row shape before running a query.
- CR-D102 adds the Surface Evolution Framework as the durable methodology for
  future command, predicate, annotation, and top-level feature changes.

### Changed

- Default `anneal --help` now teaches the compact Code Mode surface:
  `status`, `context`, `schema`, `describe`, `eval`, `search`, `read`,
  `handle`, and `init`, plus standard `help`.
- Top-level help no longer advertises the hidden compatibility filter/render
  flags as a "Compatibility options" section.
- `describe runtime` now carries the compact command map and vocabulary query
  recipes. `describe NAME` remains the place to learn examples and Common
  joins before writing `anneal -e` queries.
- `describe` examples and Common joins now show projected output columns so
  agents can see row shape before running a query.
- `anneal help eval` teaches `at("snapshot:last")` composition again since
  automatic snapshots make it honest. Git-ref temporal references such as
  `at("HEAD~N")` remain unsupported and are not in the grammar tour.
- README and the bundled `anneal` skill now teach Code Mode composition in
  the "Work The Convergence Frontier" section instead of listing hidden
  runtime nouns. The retired `H` alias mention and the legacy "hidden
  compatibility commands" guidance are removed.
- All retired commands return teaching recovery messages naming the eval-form
  or runtime-verb replacement.

### Deprecated

- `top_work(h, energy)`, `blocked_row(h, energy, source)`, and
  `recent(h, days)` remain callable through v0.13 as compatibility aliases,
  but their describe cards point agents to `frontier`, `blocker`, and
  `changed_within`.

### Removed

- Removed the cookbook cluster: the `@cookbook(...)` annotation, the
  `cookbook(...)` primitive, the `anneal cookbook` command, CLI cookbook
  rendering, and the bundled prelude recipes.
- Removed the prelude command wrappers for `anneal vocab`, `anneal verbs`, and
  `anneal examples`. The underlying introspection data remains queryable
  through `schema`, `describe`, `examples(...)`, and `verbs(...)`.
- Removed the `anneal save` write path. Reusable project moves are now ordinary
  `@verb(...)` declarations edited directly in `anneal.dl`.
- Retired the remaining v0.10 compatibility commands: `impact`, `find`, `get`,
  `map`, `health`, `diff`, `obligations`, `garden`, `orient`, `query`, and
  `explain`. Each now fails with a teaching recovery message pointing to
  `handle --impact`, `status`, `search`/`read`/`handle`, or a specific
  `anneal -e` composition.
- Retired the compatibility filter/render flag dialects (`--pretty`, `--area`,
  `--recent`, `--since`, `--plain`, `--minimal`, and `--no-color`) from the
  runtime surface. Use `--format`, `--json`, `changed_within(h, days)`, and
  `git_mtime(file, instant)` instead.
- Removed the "Compatibility options" section from top-level help.
- Retired the hidden runtime command nouns `work`, `blocked`, `diagnostics`,
  `broken`, `areas`, `trend`, and `sources`. Their workflows now live as
  explicit `anneal -e` compositions over `frontier`, `blocker`, `diagnostic`,
  `area_health`, `area_frontier`, snapshot time blocks, and `sources`.

### Migration

- No config migration is required from v0.12.x.
- Every retired command emits a teaching recovery message naming the eval or
  runtime-verb replacement. Scripts using retired commands fail loudly with the
  replacement printed; there is no silent breakage.
- The nine-command surface is the same set advanced agents already used:
  legacy nouns drift into the language instead of remaining parallel commands.

### Known Limitations

- `at("snapshot:last")` and snapshot-based time blocks work via automatic
  snapshots, including status-change queries between the latest snapshot and
  current graph state.
- `at("HEAD~N")` and other git-ref temporal forms remain unsupported until
  temporal resolution learns to materialize historical git refs.

Older entries below describe the behavior shipped in that release. For current
workflow guidance, prefer the v0.13.0 section above and the README.

## v0.12.0 - 2026-05-21

This release makes the language loop complete: agents can discover the corpus
vocabulary, compose precise Datalog questions, study worked recipes, and save a
successful query as a project verb for the next session.

### Added

- Relation-pattern calls for derived predicates and primitives. Queries can now
  use the same partial-field shape as stored relations:
  `diagnostic{code: "E001", file: "x.md"}`. Omitted fields act as hidden
  wildcards, so agents can filter by the fields they care about without
  spelling every positional column.
- A predicate signature registry shared by analysis and introspection. Names
  shown by `schema` and `describe` are now the same names accepted by the
  analyzer, including constant-headed predicates such as `diagnostic`.
- `anneal diagnostics` as the canonical full diagnostic stream. `broken`
  remains the error-only view, and `check` remains callable as a hidden
  CI-friendly gate alias.
- `describe` cards now include Common joins for primary predicates, so agents
  can learn how to combine relations such as `diagnostic` with `area_of` or
  `top_work` with `blocked`.
- `anneal cookbook` lists worked recipes by question shape. The bundled prelude
  ships recipes for diagnostics by file, diagnostics by area, blocked work by
  area, open obligations, citation lookups, stale work, and broken-reference
  review.
- `anneal save` promotes a working eval query into a project `@verb` in
  `anneal.dl`. Saved verbs are callable by name, listed by `anneal verbs`,
  documented by `anneal describe` and `anneal help`, and support the same typed
  argument dispatch as built-in verbs.

### Changed

- `anneal help eval` now teaches relation-pattern syntax, the cookbook loop,
  and the save-as-verb workflow.
- `anneal help save`, README, and the bundled skill document the recovery path
  for a bad saved query: edit `anneal.dl` or rerun `anneal save ... --force`.
- Gate semantics for diagnostics are independent of display flags, so
  `anneal diagnostics --gate --rows=0` still exits nonzero when errors exist.

### Migration

- No config migration is required from v0.11.2.
- Existing positional calls and exact named calls continue to work. Brace-style
  relation-pattern calls are additive.
- `anneal check` remains callable for CI and pre-commit muscle memory, but it is
  intentionally hidden from the default command ladder in favor of
  `diagnostics` and `broken`.

### Known Limitations

- Comparisons inside relation-pattern braces are not supported. Write
  comparisons as normal body atoms, for example:
  `? freshness(h, days), days > 30, active(h).`

## v0.11.2 - 2026-05-21

This release makes anneal more self-teaching. The runtime now explains itself
more clearly, rejects misleading flag combinations with recovery hints, and
gives agents a cleaner convergence landing before they decide what to read or
fix next.

### Added

- `anneal areas` is now a runtime verb over `area_health` and `area_frontier`.
  It gives agents a per-area drill-down from `status`: health grades for each
  area plus the local work frontier.
- `anneal help eval` now teaches the Datalog dialect directly, including stored
  relations, derived predicates, local rules, negation, aggregation, `at(...)`
  time blocks, and stratification.
- `anneal examples NAME` now covers stored relations and primitives as well as
  saved verbs, so agents can copy and modify real query shapes.

### Fixed

- `anneal status` now renders as a convergence landing: one row per handle,
  disjoint `Blocked` and `Other work` sections, deterministic reason priority,
  and a compact `Convergence` summary header.
- `anneal describe NAME` now renders teaching cards by default, with kind,
  signature, relationships, preconditions, examples, cross-references, and
  source labels. Collision cases such as `search` show both the verb and the
  primitive relationship instead of hiding one behind the other.
- Runtime and compatibility flag dialects now fail loudly when mixed. For
  example, runtime verbs reject compatibility filters such as `--area` and
  point agents toward the equivalent Datalog query shape.
- Surface bugs from the audit are fixed: `prime --json` emits JSON, `diff`
  validates refs, `find --kind` validates enum values, section-owned labels are
  discoverable with `find --namespace`, unknown `describe` names teach recovery
  commands, and `map --render=text` works.
- Datalog parse errors now suggest the stored-relation `*` prefix when an agent
  writes `handle(...)` where `*handle{...}` was intended.
- Root-level markdown files now share the `(root)` area instead of each becoming
  a one-file area.

### Migration

- No config migration is required from v0.11.1. The visible behavior change is
  stricter flag handling: harnesses that passed compatibility flags to runtime
  verbs will now receive a diagnostic with the intended replacement.

## v0.11.1 - 2026-05-20

### Changed

- Reframed `anneal --help`, README, and the bundled skill around the
  language-first ladder: arrive with `status`/`context`, discover the runtime
  with `schema`/`describe`/`verbs`/`vocab`, retrieve evidence with
  `search`/`read`/`handle`, and use `anneal -e` for precise corpus questions.
  Compatibility-era commands remain callable but no longer appear as peer nouns
  in default help.
- Expanded `anneal help eval` with syntax notes, introspection guidance,
  stored-relation examples, primitive examples, stdin usage, and bounded
  exploration guidance.
- Project `@verb` declarations are now callable by name from the CLI with
  typed arguments. Required arguments can be passed positionally in declaration
  order or as named flags; defaults and bool flags are read from the
  `@verb(args: [...])` contract.
- Standard-library verbs now dispatch through the same registry path as project
  verbs, so prelude and project verbs share help, argument handling, and
  `--explain` behavior.
- Search and context ranking now prefer canonical source documents over
  synthetic label-index or broad-hub hits when clustered child evidence points
  back to a parent file.
- Runtime configuration declarations now live in one typed schema. Project
  loading, `anneal init`, unified `anneal.dl` parsing, and the markdown
  transition bridge consume the same schema-lowered config facts.

### Added

- `anneal -e` / `anneal eval` now accepts `--limit N` to cap broad exploratory
  query output after evaluation.

### Fixed

- Label namespaces are inferred from corpus evidence instead of maintained as a
  manual inventory. Legacy `confirmed` namespace config is dropped during
  conversion, and stale unified config can be repaired with `anneal init
  --dry-run` followed by `anneal init --force`.
- Public docs, CLI help, and the bundled skill now describe the shipped
  language-first surface consistently, including Home Manager boundaries and
  `anneal.dl` configuration.
- Runtime verb UX is more predictable in agent harnesses: TTY/text rendering is
  available through `--format=text`, empty result sets report clearly, typoed
  stored-relation fields produce diagnostics, and broad `--explain` output is
  capped by default.

## v0.11.0 - 2026-05-16

### Added

- Programmable Corpus Runtime: a Datalog substrate for corpora.
  - Typed IR with stratified negation, safety checks, and aggregation.
  - Engine-derived primitives for graph reach, citation counts, and snapshots.
  - Stored relations exposed through a `Source` trait; markdown ships.
  - Standard-library preludes for graph, convergence, checks, and ranking.
  - Project `anneal.dl` files for corpus config, adapter discovery, project
    rules, and `@verb` declarations.
  - Trail/provenance capture and capability/policy gates on every query.
- New additive runtime command surface in the same `anneal` binary: `status`,
  `context`, `search`, `read`, `handle`, `work`, `blocked`, `broken`, `trend`,
  `describe`, `sources`, `schema`, `verbs`, and `eval` (also `-e`).
  `anneal --help`, `anneal <command> --help`, and
  `anneal help <command>` expose the surface.
- `anneal context GOAL` composes search hits, bounded read spans, and graph
  neighborhood into one response. Its `--budget` derives a per-hit read cap
  that is applied independently to each winning hit, so a strong long-form
  result is not dropped merely because several hits were selected.
- `--explain` derivation traces for raw Datalog queries and runtime verbs,
  including rule/provenance paths back through the prelude and project layers.
- Cargo workspace split into `anneal-core`, `anneal-md`, `anneal-cli`,
  `anneal-mcp`, `anneal-lang`, and `anneal-legacy`. `anneal-lang` remains a
  private crate (`publish = false`) until the syntax boundary has a second
  consumer and pinned public semantics.

### Changed

- Existing corpus-health workflows remain available. The runtime commands ship
  in the same installed binary.
- `trend` now degrades cleanly on corpora without snapshot history by emitting
  zero rows instead of failing.
- `anneal status` is the named runtime status command, and bare `anneal` also
  routes to that status view. The older compatibility health report is
  available as `anneal health`.
- README and the bundled `anneal` skill lead with the programmable runtime
  while still documenting compatibility commands (`health`, `check`, `get`,
  `find`, `map`, `impact`, `diff`, `obligations`, `init`, and `prime`).
- `anneal init` now scaffolds unified `anneal.dl`. It is non-destructive by
  default; with `--force`, it writes `anneal.dl` from the loaded repo config
  and moves an older `anneal.toml` to `anneal.toml.legacy`.

### Migration from 0.10.x

- `anneal status` is the runtime work-prioritization view. Use
  `anneal health` for the corpus-health overview that previously lived at
  `anneal status`.
- New runtime commands are additive; existing `check`, `get`, `find`, `map`,
  `impact`, `diff`, `obligations`, `init`, and `prime` workflows remain in the
  same binary.
- Existing `anneal.toml` files are upgrade input. Runtime commands use
  `anneal.dl`; use `anneal init --dry-run` to preview the unified form, then
  `anneal init --force` to write `anneal.dl` and move the old TOML file to
  `anneal.toml.legacy`.

### Known Limitations

- `anneal-mcp` ships as a crate/library surface for this release. The installed
  root binary does not expose `anneal --mcp` or `anneal mcp`; source checkouts
  can inspect the crate-level tool catalog with
  `cargo run -p anneal-mcp -- --tools`.
- The default ranker is still lexical retrieval. It includes light stemming and
  a small built-in abbreviation table for common planning terms, but broader
  domain synonyms still require explicit wording or a custom search/ranking
  provider.
- Long-range follow-up work is tracked in `bd`, including multi-corpus
  federation, section-parent query ergonomics, and profile evaluation at larger
  corpus scales.

## 0.10.1 - 2026-04-23

### Fixed

- `OutputStyle::tone(Heading)` previously emitted ANSI bold even when
  `color: false`, intending to preserve scannability in monochrome
  terminals and log files. In practice this broke pipe/grep
  cleanliness and violated the `NO_COLOR` intent ("no added
  ANSI"), and made plain-mode tests fail in the Nix build sandbox
  (seen as `\u{1b}[1m` wrappers around headings). Plain mode now
  emits no ANSI at all, including bold. Rich / TTY output unchanged.

## 0.10.0 - 2026-04-23

### Changed

- `anneal orient` redesigned around two tiers: **Frontier** (where
  work is now) and **Foundation** (stable hubs the frontier still
  cites). Cross-corpus testing exposed that the previous algorithm —
  single score from edge centrality × recency × status — surfaced old
  stable hubs correctly but missed the current frontier and the curated
  entry points maintainers wrote on purpose.

  **Foundation** scores each incoming citation by the *citer's*
  recency, so a March hub cited by twenty April docs ranks highly
  while a March hub cited by fifty February docs (pre-frontier)
  decays. Curated hubs (`README`, `CHANGELOG`, `DESIGN-GOALS`,
  `OPEN-QUESTIONS`, `LABELS`, `INDEX`, `ROADMAP`, `OVERVIEW`,
  `GLOSSARY` by basename, plus files with `status: living` or a
  `purpose:` line matching "entry point" / "read first" / "overview" /
  "map" / "orientation") receive an explicit bonus — human-curated
  signals outrank graph-centrality guesses (`KB-P9`).

  **Frontier** picks per-area newest file with a Frontier-eligible
  status. When the corpus declares `config convergence { ordering([...]). }`
  in `anneal.dl`, a status is Frontier-eligible if it appears in
  that ordering — so off-pipeline alive statuses like `reference` or
  `stable` stay out of Frontier and flow to Foundation where they
  belong. Without an ordering, any non-terminal declared status
  qualifies. Curated hubs and files in archive-style directories
  (`archive/`, `archives/`, `archived/`, `old/`, `legacy/`) are
  never Frontier. In `--area=X` mode, all area files by date. Flat
  corpora fall back to top-5 globally by date.

  **Hard filters** replace the previous soft content-size penalty.
  Terminal status (per the corpus lattice — tool-wide, not an
  orient-specific list), `superseded-by:` frontmatter pointer,
  archive-style directories, and files below `[orient].stub_bytes`
  (default 1000) that aren't curated hubs are excluded entirely —
  not demoted. Stubs and redirect aliases never consume orient
  budget.

  **Overflow** sub-block catches oversized candidates. Files whose
  token cost exceeds the remaining budget appear as `path  size`
  rows (no snippet; capped at 5 per tier). Agents re-run with a
  wider `--budget` to pull specific ones in.

  **`--json` breaking change:** the `entry_point` tier variant is
  replaced by two — `frontier` and `foundation`. Downstream JSON
  consumers must update the tier-name dispatch.

  **New config:** `[orient].stub_bytes` (default 1000) and
  `[orient].curated_hub_weight` (default 10.0). See `anneal orient
  --help` for the full contract; `skills/anneal/SKILL.md` and the
  README's orient section for the annotation vocabulary.

- Terminal status vocabulary unified across the tool. orient
  previously maintained its own list of terminal tokens that
  diverged from the lattice heuristic used by every other surface,
  so a corpus with `status: closed` got one answer from `orient` and
  another from `check`. Every surface now reads from the lattice;
  the heuristic family absorbed the orient-only tokens (`historical`,
  `prior`, `incorporated`, `digested`). One canon, one contract:
  "anneal respects your lattice."

- S001 (orphaned handle) no longer fires on "solo" version handles —
  a Version generated from a filename like `2026-04-17-audit-v3.md`
  where no `-v1` sibling exists. Detection: no outgoing `Supersedes`
  edge means no older sibling, so the version is filename hygiene,
  not a disconnected version chain. Real version chains (v17 of
  formal-model) still flag when genuinely orphaned.

- CLI output tightened across every command. The `·` glyph is
  retired from inline separators — commas carry that role now, and
  whitespace + indentation carry list grouping. Garden leads with a
  blast-first header row (`1  HIGH  [FIX]  5 broken refs in
  implementation/`) with a stable left-column layout regardless of
  title length, a Maintenance-tasks heading with `showing N of M`
  when truncated, and a unified `… (N more)` detail truncation.
  Check emits a `Diagnostics (N)` heading unconditionally and
  separates severity groups with a blank line. Get's default view
  grew a Try hint block that points agents to `--context` and
  `--full`; `--context` drops the duplicated Snippet KV row. Map
  summary and by-area both right-pad their count columns, snapshot's
  convergence detail always renders obligation delta with an
  explicit sign, and orient/find share one `SNIPPET_MAX = 120` with
  an explicit `…` when cut.

- `query`, `explain`, and `map --around` output flow through the
  same Printer primitives as the other 12 commands. Each subcommand
  leads with a heading that names both the kind and the count —
  `Handles (5)`, `Diagnostics (2)`, `Convergence drifting`,
  `Neighborhood anneal-spec.md depth 1`. Query results use the
  shared table primitive so `kind`/`status`/`incoming`/`outgoing`
  columns align regardless of content width, and
  diagnostics/suggestions share the `severity[CODE]  message /
  at path:line` shape with `anneal check`. Pagination is a `Try`
  hint block with action descriptions (`--offset 5  next page`,
  `--full  all results`) rather than a flat `next` footer. Explain
  surfaces fold facts into a `Facts (N)` heading with aligned
  `fact_type key value` rows; impact's direct and indirect sections
  carry counts and render `(none)` in dim when empty. Map's focused
  neighborhood gets a `Files (N)` / `Namespaces (N)` /
  `Focus edges (N)` / `Other neighborhood edges (N)` structure with
  `… N more` truncation markers. Explicit `--render=text` and
  `--render=dot` on `map` remain byte-stable passthroughs for
  pipelines.

### Internal

No user-visible behavior change, but the codebase got meaningfully
simpler before the cut:

- `Printer<W: Write>` is no longer generic. The struct owns a
  `Box<dyn Write>`; the `Render` trait and ~45 free render helpers
  dropped their `<W: Write>` parameter. Construction uses
  `BufWriter::new(io::stdout())` (owned, 'static, buffered). Binary
  is slightly smaller (monomorphization collapse).
- Three near-duplicate output helpers (`emit_output`,
  `emit_full_output`, `emit_explanation`) collapsed into a single
  `emit_rendered`. Call sites pass the `OutputMeta` level they want
  explicitly; `Printer` is constructed in exactly one place.
- orient's `is_curated_hub` was called three times per file during
  scoring + tier assignment. Result now memoized on `FileEntry` —
  noticeable on larger corpora. Path widths similarly cached on
  `OrientEntry` so `measure_text_width` isn't called twice per entry
  during render.
- orient's `status_bonus` hardcoded a token list that ignored the
  corpus lattice. Now reads `lattice.active` — a corpus that
  declares `wip` as active gets the right bonus for its own work.
- `Handle::is_terminal(lattice)` adopted at several sites that
  open-coded `lattice.terminal.contains(s)`. Same result, consistent
  idiom.
- Audit-reference narration comments (`R2`, `F23`, `F32`, etc.)
  scrubbed from the codebase. The features are landed; the refs
  were dead weight.

## 0.9.2 - 2026-04-17

### Changed

- Orient's edge and label scoring now uses `ln(count + 1)` instead of raw counts, and weights incoming edges twice as heavily as outgoing. The two changes fix a common failure mode where a recently-authored, status=active spec lost to an older label-anchor file whose lead came entirely from sheer inbound mass. Log-scaling gives diminishing returns: a file cited 100× isn't 10× more useful as an entry point than one cited 10×, and treating it as such drowned out recency and status bonuses. Weighting inbound higher than outbound separates "others cite this" (real centrality) from "this cites a lot" (long reference tables — weaker signal). A file authored recently and marked active now outranks an older reference-catalog doc in the same area, matching how a reader would actually want to approach the material.

## 0.9.1 - 2026-04-17

### Changed

- Orient's recency ranking now uses exponential decay anchored at today with a configurable half-life, replacing the linear normalization across the corpus's full date span. The old formula measured "recency" as a file's position between the oldest and newest dates in the corpus, so a single ancient reference could recalibrate every score — and a file's recency barely nudged its rank because the `recency_weight` default was 0.5 against edge/label scores that routinely hit 10+. The new formula gives a file dated today a full bonus, halves every `recency_half_life_days` (default 90) of age, and defaults `recency_weight = 5.0` so recent work actually shows up in rankings. Agents asking "orient me on this area" now get recently-touched files floating to the top instead of being dragged down by highly-linked historical aliases.

### Added

- `[orient] recency_half_life_days` config field (default 90). Shorter half-life for corpora where only the last few weeks matter; longer for slower-moving reference material.

### Fixed

- Release workflow now opts into the Node.js 24 runtime early (via `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24=true`) so breakage in `actions/upload-artifact@v4` / `actions/download-artifact@v4` under Node 24 surfaces on routine pushes rather than on a release day. GitHub forces the switch on 2026-06-02.

## 0.9.0 - 2026-04-17

### Added

- `anneal prime`: print the agent skill briefing (first moves, command map, agent rules). The content is baked into the binary via `include_str!("../skills/anneal/SKILL.md")` at build time, so the skill file and the `prime` output stay in sync from a single source. Runs without building the graph or reading config — pure output, always succeeds. Intended for onboarding a fresh agent that doesn't have the skill preloaded, or recovering context after a session restart.
- `anneal orient`: context-budgeted reading list for agents. Scores every file by edge centrality, label density, recency, and status, then tiers the result as pinned → area entry points → upstream context → downstream consumers. Tiers fill greedily until the token budget is exhausted. Flags: `--area=X`, `--budget=Nk`, `--file=X`, `--paths-only`, `--json`. The `--file=X` variant walks upstream dependencies — the before-edit complement to `impact`. `--file` and `--paths-only` compose.
- `anneal garden`: ranked maintenance tasks with `fix:`, `context:`, and `verify:` hints so an agent can close the garden → orient → fix → check loop without guidance. Six categories: `fix` (E001/E002), `tidy` (S001 orphans), `link` (island areas), `stale` (old files), `meta` (W003), `drift` (cross-area namespace dispersion). Flags: `--area=X`, `--category=X`, `--limit=N`, `--json`.
- `anneal map --by-area`: area-level topology graph. Nodes are areas, edges are aggregated cross-area connection counts, islands are listed separately. Flags: `--by-area`, `--min-edges=N`, `--include-terminal`, `--render=text|dot`.
- `anneal diff --by-area`: per-area convergence deltas with Δ errors, Δ orphans, Δ connectivity, and a trend column (improving/holding/degrading/new/removed). Grade changes render as `[B→C]` inline. Falls back to a current-state view when no snapshot history exists.
- Batch handle lookup on `get`: `anneal get a.md b.md c.md` emits a compact one-line-per-handle table. `--status-only` trims to identity + status; `--context` adds the purpose/note summary. JSON emits an array. Single-handle `get` retains the detailed view.
- `--scope=active|all` on `check`: unified convergence scope flag, mirrors `query --scope`. The legacy `--active-only` / `--include-terminal` booleans remain as deprecated aliases.
- Pipeline semantics in `explain convergence`: active/terminal partition and ordering are always shown; optional `[convergence.descriptions]` TOML table attaches a human-readable description to each status. Agents encountering unfamiliar status values can now read the operational meaning directly from the command.
- `--area=<name>` global flag: scopes `status`, `check`, `map`, `impact`, `find`, `query`, and the new `orient`/`garden` commands to one area (directory or concern group).
- `--recent` / `--since=Nd` global flags: temporal scoping for files whose resolved date falls inside the window.
- `--sort=date` on `find` and `query handles`: chronological view without needing a standalone `recent` command. `find`'s positional query is now optional when any filter is present.
- `--context` enrichment on `find` and `query handles`: adds a `purpose:` / `note:` (or body snippet) column to the output table.
- `map --around --upstream` / `--downstream`: directed tree traversal. `orient --file=X` and `impact X` share the same upstream/downstream infrastructure.
- Obligation remediation in `explain obligation`: outstanding obligations now include the exact `discharges: [...]` frontmatter syntax needed to remediate, plus candidate discharger files ranked by graph proximity.
- `[orient]`, `[temporal]`, `[convergence.descriptions]` config sections.

### Changed

- Snapshot schema gained an optional per-area summary (files, handles, errors, orphans, cross-links, connectivity, grade). Old snapshots without this field still parse.
- Command count in docs went from 12 to 15 (`orient`, `garden`, `prime`).
- README, skill file, and `--help` examples reorganized around three explicit loops: orientation, narrowing, gardening. Command output cross-references (e.g. `status` → `check`, `areas` → `garden`, `check` → `explain obligation`) so an agent following hints reaches the right next command without consulting documentation.

### Fixed

- `AreaGrade` now round-trips through serde, so per-area snapshot data keeps its type-safe shape across the write/read boundary.
- `compute_areas` no longer treats out-of-corpus edge targets as implicitly belonging to the `(root)` area.
- `[orient] exclude` honors the same split-by-glob-sigil grammar as the top-level `exclude`. Plain entries like `"archive"` now exclude the whole top-level directory; glob patterns like `"**/CHANGELOG.md"` match path-wise. Previously orient ran every entry through `Glob::new` unconditionally, so plain names silently matched nothing.
- `orient --file=<path> --paths-only` now composes. An earlier clap constraint prevented these flags from being combined even though they're the natural combination for piping a single-file upstream reading list into another tool.

### Internal

- Extracted `area_of_handle` and `area_of_diagnostic` in `src/area.rs` as the single source of "what area does this handle/diagnostic belong to?". Replaces open-coded dispatch in `compute_areas`, `cmd_map_by_area`, and three `garden` collector paths.
- Promoted `parse::build_exclude_sets` to `pub(crate)` so `orient` reuses the same dir-name-vs-glob split that the graph walker uses. New `ExcludeMatcher` in `orient.rs` wraps it.
- Extracted `resolve_previous_snapshot` helper shared by `cmd_diff` and `cmd_diff_by_area` — one place owns the three-mode reference resolution (git_ref → days → latest).
- Unified `QueryScope` and the former `ConvergenceScope` into a single `query::Scope` enum; `check --scope` and `query --scope` now share one type.
- `MapOutput.format` and `MapByAreaOutput.format` moved from `String` to the typed `MapRender` enum; garden's `blast` is derived from `blast_score` via `GardenBlast::from_score` so the two fields can no longer drift.
- `BatchGetOptions { status_only, context }` (which accepted `{true, true}`) replaced by `BatchGetMode { Default, StatusOnly, Context }`.
- `Display` impls on `GardenCategory`, `GardenBlast`, `OrientTier`, `AreaTrend` collapse four drifting naming conventions (`short`, `short_label`, `as_str`) into one.
- Handle constructors gained `size_bytes: Option<u32>` (populated during `build_graph`); consumed by `orient`'s token budget estimation.
- Structured `Evidence::Suggestion::OrphanedHandle` replaces regex message parsing in `garden`'s S001 extraction.
- `around_subgraph` in `src/cli/map.rs` is now `pub(super)` so `orient --file=X` can share the same BFS infrastructure as `map --around`.

## 0.8.0 - 2026-04-15

### Added

- `anneal areas` command: per-directory health profiles with grades (A-D), connectivity, cross-links, orphan counts, and signal summaries. Auto-detects areas from top-level directory structure. Flags: `--sort=files|grade|conn|name`, `--include-terminal`, `--json`.
- Temporal awareness: file handles now carry a resolved date from `updated:` frontmatter > `date:` frontmatter > `YYYY-MM-DD` filename prefix. Foundation for upcoming `--recent`, `--since`, and `orient` features.
- `[areas]` config section with `orphan_threshold` for tuning grade sensitivity.
- `[temporal]` config section with `recent_days` for the upcoming `--recent` flag.
- Design notes for areas/orient/garden feature set and CLI UX audit.

### Changed

- `check` human output now sorts diagnostics by severity (errors first). Previously sorted by code, which buried errors under suggestions in large corpora.
- Human output now says "terminal" instead of "frozen" to match project terminology consistently.
- Handle construction uses five named constructors (`Handle::file`, `::section`, `::label`, `::version`, `::external`) instead of raw struct literals. Adding a field to Handle is now a one-file change.

### Fixed

- Body-text edge kind inference is now per-line instead of per-block. DependsOn keywords on one line no longer promote references on other lines within the same paragraph.
- Removed "based on" from DependsOn keyword list (too common in prose).
- Implausible markdown link destinations (single characters, bare uppercase tokens like `T` from `Stream[r](T)`) are now rejected instead of creating E001 diagnostics.
- File glob patterns in `exclude` config now work (`**/README.md` prevents matched files from entering the graph).
- Heading-defined labels take ownership priority over table cell and inline references.

### Internal

- Deduplicated 7 test factory definitions into canonical `Handle::test_file`, `Handle::test_label`, `Lattice::test_empty`, `Lattice::test_new`, and `Lattice::test_with_ordering`.
- Area module takes `&Lattice` for correct active/terminal counts (not the approximation from initial implementation).

## 0.7.4 - 2026-04-12

### Fixed

- Body-text edge kind inference is now per-line instead of per-block. A DependsOn keyword (e.g. "incorporates") on one line no longer promotes references on other lines within the same paragraph to DependsOn. Fixes false-positive W001 warnings from prose that happened to share a paragraph with a structural keyword.
- Removed "based on" from the DependsOn keyword list — too common in normal prose, causing false structural dependencies.
- Implausible markdown link destinations (single characters, bare uppercase tokens like `T` from `Stream[r](T)`) are now rejected instead of creating broken-reference E001 diagnostics. Fixes false positives in corpora with formal math notation.

## 0.7.3 - 2026-04-08

### Added

- File glob patterns in `exclude` config: entries like `**/README.md` now prevent matched files from entering the graph entirely. Plain directory names continue to work as before. Useful for structural index files that should not trigger W003 or S003 diagnostics.

### Fixed

- Heading-defined labels now take ownership priority over table cell and inline references. Fixes incorrect `file` attribution when the same label appears in both a heading definition and a reference table elsewhere in the corpus.

## 0.7.2 - 2026-04-08

### Fixed

- Labels defined in markdown table cells are now extracted (requires `ENABLE_TABLES` in the cmark parser). Fixes false-positive E001 broken references for corpora that define labels in tables.
- Compound hyphenated prefixes (e.g. `ST-OQ` from `ST-OQ-1`) are now captured as a single prefix instead of only the last segment (`OQ`). Fixes resolution failures for namespaces with compound prefixes.

## 0.7.1 - 2026-04-08

### Fixed

- CLI help text for `anneal impact` now documents `[impact] traverse` config instead of describing a hardcoded traversal set.
- Spec §12.7 and README impact section updated to match.

## 0.7.0 - 2026-04-08

### Added

- Configurable impact traversal: `[impact] traverse` in `anneal.toml` controls which edge kinds `anneal impact` follows. Corpora using custom edge kinds (Synthesizes, Implements, Reconciles) now get accurate blast radius analysis. Defaults to the previous behavior (DependsOn, Supersedes, Verifies) when absent.

## 0.6.1 - 2026-04-08

### Fixed

- Off-by-one in frontmatter line count: body-text line numbers in diagnostics were reported 1 too high for files with frontmatter.
- `Severity` serialization now consistently produces lowercase (`"error"`, `"warning"`) instead of PascalCase in JSON.
- Diagnostics with unknown line numbers now report `line: null` instead of the misleading sentinel `line: 1`.
- Evidence serialization in identity computation uses graceful fallback instead of `expect()`.

### Changed

- `resolved_file` returns `Option<&Utf8Path>` instead of allocating `Option<String>` on every call.
- `run_checks` takes a `CheckInput` struct instead of 9 positional parameters.
- `read_latest_snapshot` reads the history file backwards, parsing only the last line instead of all lines.
- `try_version_stem` uses a pre-built `VersionStemIndex` for O(1) lookup instead of scanning all node keys.
- `classify_frontmatter_value` results are cached across frontmatter processing loops.
- `check_confidence_gap` builds a `HashMap` for state level lookups instead of linear scanning.
- `is_terminal_by_heuristic` moved from `parse.rs` to `lattice.rs` (fixes layering inversion).
- `parse_frontmatter` returns a `FrontmatterParseResult` struct instead of a 4-tuple.
- `EdgeKind::from_name` uses case-insensitive matching for well-known kinds.
- `EdgeKind::Custom` uses `Box<str>` instead of `String` (8 bytes smaller per edge).
- Diagnostic codes promoted from `&'static str` to `DiagnosticCode` enum for exhaustive matching.
- `ImplausibleReason` promoted from `String` to a four-variant enum.
- `HashMap<String, usize>` in `summarize_extractions` changed to `HashMap<&'static str, usize>`.
- `cli.rs` (4459 lines) split into `src/cli/` module directory with 11 focused submodules.
- Malformed YAML frontmatter and non-UTF-8 filenames are now tracked in `BuildResult` for future reporting.

### Removed

- Dead code: `ConvergenceState`, `classify_status`, `Resolution` enum, `node_mut`, `Explanation` wrapper enum.
- Stale Phase 2 comments and unjustified `#[allow(dead_code)]` annotations.
- Duplicate `fnv1a_64` implementation in `snapshot.rs` (now imports from `identity.rs`).

### Added

- 34 new tests for `lattice.rs` (12), `graph.rs` (8), `obligations.rs` (8), and `split_frontmatter` (6) — covering all four previously untested modules.

## 0.6.0 - 2026-04-08

### Added

- Custom edge kinds: any `edge_kind` string in `anneal.toml` that doesn't match a well-known kind (Cites, DependsOn, Supersedes, Verifies, Discharges) is now accepted as a `Custom` edge kind — indexed in the graph and queryable via `anneal query edges --kind=<name>`, with no built-in diagnostic behavior.
- The `--kind` filter on `anneal query edges` now accepts any string, not just the five well-known kinds.

### Changed

- W001 (stale reference) now fires only on `DependsOn` edges. Cites and custom edges from active to terminal handles no longer trigger staleness warnings.

## 0.5.0 - 2026-04-07

### Added

- Added the `anneal query` command family for bounded structural selection across handles, edges, diagnostics, obligations, and suggestions.
- Added the `anneal explain` command family for provenance-oriented explanations of diagnostics, impact results, convergence signals, obligations, and suggestions.
- Added stable diagnostic and suggestion identities so `check`, `query`, and `explain` compose through explicit IDs.
- Added structured suggestion evidence for `S001` through `S005`, enabling typed suggestion explanation and selector matching instead of message-text heuristics.

### Changed

- Simplified the internal query/explain analysis pipeline by factoring shared analysis, obligation, identity, and selector logic into dedicated modules.
- Tightened query/explain defaults around bounded output, active-scope filtering, and check-compatible diagnostic derivation.
- Updated the README, canonical docs, CLI help, and bundled anneal skill so the new query/explain workflows are documented consistently.

## 0.4.3 - 2026-04-02

### Changed

- Made `install.sh --skill-target` stage the bundled skill once per install and fan it out to each requested target instead of re-downloading the same bundle for every target.

### Fixed

- Made installer smoke coverage compare the installed skill directory against the bundled `skills/anneal` tree, removing duplicated file-list assumptions from CI.

## 0.4.2 - 2026-04-02

### Added

- Added optional `install.sh --skill-target ...` support so the curl installer can populate one or more agent skill directories with the bundled anneal skill.

### Changed

- Clarified README installer guidance so binary-only installs and installer-managed skill targets are documented together.

### Fixed

- Added installer smoke coverage for bundled skill installation so the curl install path and documented skill targets stay verified together.

## 0.4.1 - 2026-04-02

### Added

- Added optional Home Manager skill installation so anneal can declaratively link its bundled skill into agent-specific locations such as `.agents/skills/anneal` and `.codex/skills/anneal`.

### Changed

- Hardened Home Manager skill target handling by rejecting non-home-relative paths and duplicate targets.
- Simplified the Home Manager smoke harness so configured and bare cases share one evaluator instead of duplicating module stubs.

### Fixed

- Fixed the Home Manager smoke test to match anneal's text-based config emission and keep CI green after the config output refactor.
- Updated GitHub Actions checkout steps to `actions/checkout@v5`, removing the Node 20 deprecation warning from CI.

## 0.4.0 - 2026-04-02

### Added

- Added an exported Home Manager module so Nix users can install `anneal` and manage its XDG user config declaratively.
- Added smoke coverage for the Home Manager integration path, including CI coverage and a repo-local smoke test helper.

### Changed

- Redesigned `check`, `find`, `get`, and `map` around progressive disclosure so risky JSON output is bounded by default and expands explicitly.
- Polished human progressive-disclosure output on hub handles so `get --context` is easier to scan and `map --around` summarizes large neighborhoods instead of dumping them by default.
- Clarified README installation guidance for Nix profile installs versus Home Manager-managed configuration.

### Fixed

- Removed self-corpus check noise caused by absolute repo-local references in redesign docs.
- Fixed the Home Manager module so it works in a real Home Manager / nix-darwin configuration without recursive module evaluation.

## 0.3.1 - 2026-03-31

### Changed

- Tightened the anneal skill defaults so broad orientation uses compact health checks instead of raw diagnostic JSON dumps.
- Replaced brittle skill examples with commands that work in anneal's own corpus or clearly use placeholders where the argument must come from the active corpus.
- Made the release helper scaffold a changelog entry on version bump and require a non-placeholder release entry during release verification.

## 0.3.0 - 2026-03-31

### Added

- Added a release changelog and started tracking release-facing changes in one place.
- Added installer UX improvements including `--help`, `--dry-run`, `--print-target`, `--install-dir`, and `--tag`.
- Added automated release verification covering version alignment, release-target alignment, installer checks, release builds, and public-repo safety checks.
- Added broken-file `did you mean ...` suggestions for unresolved bare filename references.

### Changed

- Moved anneal snapshot history to machine-local XDG state by default, while keeping explicit repo-local history mode and legacy history compatibility.
- Made `anneal check` default to active-file diagnostics, with `--include-terminal` for the full corpus view.
- Reused parse-time snippet data for `anneal get`, avoiding extra file reads on the common path.
- Tightened snapshot history APIs so latest-snapshot reads and full-history reads are explicit.
- Promoted `install.sh` to a first-class release surface in CI and docs.
- Refined CLI help and the anneal skill so they teach convergence, settledness, and disconnected-intelligence workflows more clearly.

### Fixed

- Hardened XDG history handling so repo config cannot direct writes to arbitrary machine-local paths.
- Made malformed user config warn and fall back to defaults instead of breaking the CLI.
- Made no-`HOME` / no-`XDG_STATE_HOME` environments degrade gracefully while still reading legacy repo-local history when available.
- Normalized zero-padded label lookups such as `OQ-064` in direct handle lookup.

### Internal

- Simplified the analysis pipeline and recent lookup helpers.
- Reconciled backlog residue from the completed v1.1 milestone and closed stale tracked work.
