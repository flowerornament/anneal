# anneal

Convergence assistant for knowledge corpora.

`anneal` reads a directory of markdown files, computes a typed knowledge graph, checks it for local consistency, and tracks convergence over time. It is built for disconnected intelligences — agents across sessions with no shared memory — to orient in a shared body of knowledge and push it toward settledness.

## Project Context

- anneal's own specs are tracked as markdown in this repository. Inspect and maintain them with `anneal`: `anneal status`, `anneal check`, `anneal handle <file> --impact`.
- Design/spec docs under `.design/` carry the date in the H1 title as `# <Title> — YYYY-MM-DD`, matching the dated filename / frontmatter `date:`.
- The v2.0 master spec is the authoritative Programmable Corpus Runtime reference.
- The v1.x spec is superseded but retained as historical record of the shipped shape.
- The engine-viability artifacts gate v2.0 architecture decisions. Do not casually claim "SP-R1 cleared" — Ascent unsafe is accepted as bounded dependency risk, not eliminated.
- Repo-local anneal state is optional and should only be used when explicitly configured.
- `.planning/ROADMAP.md` and `.planning/STATE.md` own the roadmap spine and current state. Treat `bd` as the source of truth for in-progress work; `.planning/STATE.md` is updated at phase boundaries.
- `install.sh` ships with releases — treat installer correctness as release-critical.
- `.beads/config.yaml` is tracked repo config only; keep machine-specific federation settings local to your shell environment.
- Store state in specs sparingly. Use `bd` for state.
- On the maintainer's machine, `main` runs as `anneal-dev` beside release `anneal`, sharing `~/.config/anneal/config.toml` + history state. Keep config/history formats backward-compatible or migrate, else the release binary breaks.

For orientation in the v2.0 master spec (`2026-05-13-corpus-runtime.md`):
- Part I (`§1-§3`) for framing and the cold-agent test
- Part II (`§4-§8`) for architecture (substrate / adapters / surfaces, Source trait, engine-replaceability)
- Part III (`§9-§16`) for substrate primitives (identity, stored relations, engine-derived predicates, trails, capabilities)
- Part IV (`§17-§20`) for the language (grammar, types, aggregation, stratification)
- Part VII (`§29-§37`) for CLI + MCP surfaces
- Part X (`§47-§48`) for files and layout
- Part XV for `CR-*` label conventions (CR-D decisions, CR-R rules, CR-Q questions, CR-Fw forward-looking)

CR-* labels are referenced from bd issues and commit messages — search the spec for the exact label to find the governing definition.

For low-context corpus orientation, prefer:
- `anneal context "<goal>"`
- `anneal status` (~16 KB; run once on arrival, not per turn)
- `anneal search "<text>" --limit 25`
- `anneal read <handle> --budget 4000`
- `anneal handle <handle> --impact`

Use `anneal context "X"` for "find the section that defines X" work,
`grep -rn "X"` for exhaustive literal occurrences with line numbers, and
`anneal -e '? ...'` for structural graph predicates.

Avoid broad default dumps like raw `check --json`, empty search queries, or full graph queries unless you are intentionally expanding with `schema`, `describe`, and narrow `anneal -e` predicates.

## Rust Toolchain

- `rust-toolchain.toml` pins Rust 1.94.0 with `rustfmt` and `clippy`.
- Use `just`. `just check` is the default gate (fmt + `install.sh` syntax + clippy + test, each step timed). Inspect `justfile` or run `just --list` for the full command surface.
- `just build` for a release binary; `just release-verify` for release-readiness gates.
- `just audit` = architecture fitness functions: `cargo-machete` (unused deps) + `cargo-deny` (advisories/bans/licenses/sources, configured in `deny.toml`). `just check` runs the offline subset (machete + deny bans/licenses/sources), guarded to skip if the tools aren't installed.
- Do NOT run `just check`/the test suite inside a git worktree. anneal-re9h (fixed) once wrote `core.bare=true` into a shared `.git/config`; recover with `git config core.bare false`. jj workspaces are safe (see Version Control).
- Add dependencies with `cargo add`; never hand-write version strings.
- `ast-grep run -p 'PATTERN' -l rust` for AST-aware code search (no config needed). Useful patterns: `$X.unwrap()`, `todo!($$$)`, `#[allow($$$)]`. Add `-r 'REPLACEMENT'` for structural refactoring; `--json` for machine-readable output.

## Module Boundaries

**v2.0 target shape (Phase 1 in flight — `bd anneal-xu2`):** a cargo workspace.

- `crates/anneal-core` — substrate: Datalog runtime, dynamic IR, engine-derived primitives (Ascent-backed), convergence stdlib, provenance, trail capture. No source-specific code.
- `crates/anneal-md` — markdown adapter; implements `Source`.
- `crates/anneal-cli` — the binary; links core + md.
- `crates/anneal-mcp` — MCP server; links core + md.
- Adapters beyond markdown (`anneal-mdx`, `anneal-code`, `anneal-host`) are sibling crates added v2.1+.

`anneal-core` is the only crate other anneal crates depend on. Engine choice is internal to `anneal-core` — see master spec §8 final paragraph. Do not expose Ascent as a general runtime `.dl` loader; the dynamic IR owns prelude/project/inline rules.

**v1.x current shape (still operational until Phase 10 lands):** single-crate binary; modules in `src/` own distinct layers.

- Durable pipeline shape: `parse` → `extraction` → `graph` / `resolve` → `analysis` / `lattice` → `checks` → `output` / `cli`.
- `graph` owns the arena; `handle` owns identity types (`Handle`, `NodeId`, `HandleKind`, `EdgeKind`). Don't leak arena internals across module boundaries.
- `lattice` owns terminal/active classification. Do not reintroduce terminal-status heuristics into `orient`, `checks`, or CLI layers — they belong in the lattice.
- `cli/` and `output/` compose analyses into user-facing commands. Analysis logic belongs in its analysis module, not the CLI seam.
- The shipped surfaces are the CLI binary, the tag-driven GitHub release assets, and `install.sh`.

## Verify Before Asserting

Claims about this system's own behavior get checked against the system, not recalled.
Error strings, version state, predicate membership, counts, and file paths are quoted
from a command or a `file:line`, or they are labelled `UNVERIFIED:` with the command
that would settle them.

Three traps, each of which has produced a wrong claim here:

- **Scope is part of the claim.** What a tool walks, what the repo tracks, what config
  mounts, and what a query emits are four different sets. `lifecycle_status_classification`
  carries project origin, so it differs per corpus — measuring it here says nothing about
  murail. Measure the set the claim is about.
- **Truncated evidence reads as complete evidence.** `head -N` on a grep, an aborted
  build, a `--limit` on a query: the output looks whole and nothing marks the cut. Do not
  pipe through `head` when the result is about to become a conclusion.
- **A passing check proves nothing if its target is absent.** A probe against a path that
  does not exist, a fixture missing the case it guards, an assertion on a truncated list —
  all pass while testing nothing.

## Rust Conventions

- `unsafe_code` is denied workspace-wide. Any exception must use the narrowest `#[allow(unsafe_code)]` with documented invariants. Run `unsafe-checker` on every `unsafe` block before commit.
- Workspace Clippy policy denies `all` and `pedantic` with a small allow-list in `Cargo.toml`. Don't grow the allow-list without justification.
- No bare `unwrap()` in production code. Use `expect("reason")`, `?`, or `unwrap_or(...)` with a sensible default.
- Prefer index arenas (`Vec<Node>` + `NodeId(u32)`) over lifetime-threaded trees — this is already how `graph.rs` works.
- Use semantically distinct newtypes at identity and arena boundaries (`Handle`, `NodeId`, identity types).
- Prefer phase-local error types inside modules; `anyhow::Error` is fine at the CLI boundary.
- When changing handle/graph/lattice types, reach for `m05-type-driven`; for optimization, `m10-performance`; for new modules, `m15-anti-pattern` before treating them as done. Load `rust-skills` for general patterns.

## Task Tracking (bd)

```bash
# orient
bd show --current --short
bd show <id> --short          # plain `bd show` prints the whole comment thread — only when you need it
bd query "status=in_progress"
bd ready --explain

# work
bd update <id> --claim
bd note <id> "context"
bd close <id> --suggest-next

# capture
bd todo add "quick thought"
bd create --title="..." --type=task --priority=2
```

Prefer plain-text views for orientation. Avoid raw `bd --json` by default. Full reference: `bd prime`.

## Release Flow

Release automation is local-first and tag-driven:

- day-to-day work lands on `master` through `just land`
- release prep is an ordinary landed change
- release tags are cut from `master` in the colocated checkout `~/code/anneal`

Before bumping, verify all shipped features are reflected in docs. CLI help strings are authoritative, but these must match:

- `README.md` — command sections, output examples, config blocks, architecture listing
- `skills/anneal/SKILL.md` — first moves, command map, agent rules
- `CHANGELOG.md` — entry for the target version (scaffolded by `release-bump`)

Write docs as if they were always correct — no "added" or "updated" language.

```bash
# in your jj workspace
just release-bump 0.2.1
just release-verify
jj describe -m "release: prepare v0.2.1" && just land
# then in ~/code/anneal, the colocated checkout
jj git fetch && jj new master
just release-tag 0.2.1
```

`just release-verify` checks version alignment across `Cargo.toml`, `Cargo.lock`, and `flake.nix`; CHANGELOG entry presence without `TODO`/`TBD` placeholders; release target alignment across `release.yml`, `install.sh`, and `README.md`; public-repo safety for `.beads/config.yaml`; then runs `just check`, `just build`, `anneal --version`, and the corpus consistency check.

Every push to `master` runs the native Nix publication matrix, uploads each
runtime closure to the public Cachix cache, pins the last three revisions per
system, and proves substitution from a fresh tokenless runner with local builds
disabled.

`just release-tag` first verifies that every advertised Nix package output is
present in the public cache. Only then does it push the annotated tag and
force-update the `release` branch (`--force-with-lease`) so downstream flake
consumers can track `?ref=refs/heads/release` and resolve to the latest released
commit via `nix flake update`.

Pushing `vX.Y.Z` triggers `.github/workflows/release.yml`, publishes binaries,
then smoke-tests the installer against that exact published tag for:
- `aarch64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

## Test Corpus

Primary smoke corpus: a real-world external markdown corpus, when available locally. Useful for smoke-checking `status`, `context`, `search`, `read`, `handle --impact`, `check`, and focused `anneal -e` predicates. Integration tests may skip if the external corpus is unavailable.

## Version Control

anneal uses jj, colocated with git. All agents share one commit graph, and
`master` is the only published bookmark. Each writing agent works in its own
jj workspace; jj runs with stock configuration and records edits continuously,
so there is nothing to stage.

```text
~/code/anneal      colocated checkout: .git + .jj, the bd home, releases.
                   No agent edits code here.
~/code/anneal-1a   jj workspace, coordinator (claude)
~/code/anneal-1b   jj workspace, implementer (codex)
```

- Work: `jj new master`, edit, `jj describe -m "area: subject (anneal-xxxx)"`.
  Pick up others' landings with `jj rebase -d master`.
- Publish: `just land` runs `just check` on exactly the described change, then
  moves `master` to it and pushes. It refuses undescribed work, a stale
  parent, and any edit made while the check ran. Then `bd dolt push`. The
  implementer lands after the coordinator's GO. A raw `jj git push` skips the
  gate; never run one. jj runs no git hooks, so nothing gates a commit except
  `just land`.
- Every description gains `Jj-Workspace:`, `Agent:` and `Session:` trailers
  from `scripts/jj-identity`, which a SessionStart hook runs. Do not type or
  strip them.
- Git runs only in `~/code/anneal`. A jj workspace has no `.git`, and `~/.git`
  exists, so git in a workspace silently answers about the home directory.
  Never run a mutating git command, with one exception: releases run from
  `~/code/anneal` after `jj git fetch && jj new master` (see Release Flow;
  `scripts/release.py` tags and pushes with git there).
- anneal run inside a jj workspace reports Git-derived recency, W006 and
  assertion provenance as unavailable until anneal-qao9 lands. Run
  provenance-dependent checks from `~/code/anneal`.
- The git-fixture tests (anneal-re9h, fixed) strip `GIT_DIR` and work in their
  own tempdir repositories, so `just check` is safe in a jj workspace.
- New jj workspace: `jj workspace add --name anneal-<pair><letter>
  ../anneal-<pair><letter>`, write `../anneal/.beads` into its
  `.beads/redirect`, and link Claude's memory:
  `ln -s ~/.claude/projects/-Users-morgan-code-anneal/memory
  ~/.claude/projects/-Users-morgan-code-anneal-<x>/memory` (create the parent
  directory first). `jj workspace list` is the roster.
- Staleness: when another agent rewrites a commit under your `@`, every jj
  command refuses until `jj workspace update-stale`. Never `jj edit` a change
  that is an ancestor of another workspace's `@`.
- Recovery starts at `jj op log`, `jj undo` and `jj op restore`, before any
  destructive file operation. The `jj-ops` skill holds the jj model.

## Completion

- Before ending a session:
  1. `jj describe -m "area: subject (anneal-xxxx)"`, then `just land` (it runs
     `just check`).
  2. `bd dolt push`.
- Work is not complete until `just land` succeeds.

## Reminders

- Low context in, high signal out.
- Write it as if it was always right.
- Design it like you got it right the first time.
