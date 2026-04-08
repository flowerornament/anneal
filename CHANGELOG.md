# Changelog

All notable changes to `anneal` are documented in this file.

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
