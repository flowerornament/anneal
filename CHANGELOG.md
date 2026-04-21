# Changelog

All notable changes to `anneal` are documented in this file.

## Unreleased

### Changed

- CLI output tightened across every command (Round 2 UX audit). The
  `·` glyph is retired from inline separators — commas carry that role
  now, and whitespace + indentation carry list grouping. Garden gets a
  blast-first header row (`1  HIGH  [FIX]  5 broken refs in
  implementation/`) with a stable left-column layout regardless of
  title length, a Maintenance-tasks heading with `showing N of M` when
  truncated, and a unified `… (N more)` detail truncation. Check emits
  a `Diagnostics (N)` heading unconditionally and separates severity
  groups with a blank line. Get's default view grew a Try hint block
  that points agents to `--context` and `--full`; `--context` drops
  the duplicated Snippet KV row. Map summary and by-area both right-
  pad their count columns, snapshot's convergence detail always renders
  obligation delta with an explicit sign, and orient/find share one
  `SNIPPET_MAX = 120` with an explicit `…` when cut. See
  `.design/2026-04-17-cli-ux-audit-v2.md` for the findings table.

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
- Command count in docs and spec went from 12 to 15 (`orient`, `garden`, `prime`).
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
- Design specs for areas/orient/garden feature set and CLI UX audit.

### Changed

- `check` human output now sorts diagnostics by severity (errors first). Previously sorted by code, which buried errors under suggestions in large corpora.
- Human output now says "terminal" instead of "frozen" to match spec terminology consistently.
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
- Updated the README, canonical spec, CLI help, and bundled anneal skill so the new query/explain workflows are documented consistently.

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
