# Changelog

All notable changes to `anneal` are documented in this file.

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
