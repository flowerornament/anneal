# anneal

Convergence assistant for knowledge corpora.

`anneal` reads a directory of markdown files, computes a typed knowledge graph, checks it for local consistency, and tracks convergence over time. It is built for disconnected intelligences — agents across sessions with no shared memory — to orient in a shared body of knowledge and push it toward settledness.

## Design Corpus

- `.design/` is anneal's own design corpus. Inspect and maintain it with `anneal` under `.design/anneal.toml`: `anneal status`, `anneal check --active-only`, `anneal impact <file>`.
- `.design/anneal-spec.md` is the authoritative product and implementation spec.
- `.design/.anneal/` is optional repo-local anneal state when explicitly configured.
- `.planning/ROADMAP.md` and `.planning/STATE.md` own the roadmap spine and current state.
- `install.sh` ships with releases — treat installer correctness as release-critical.
- `.beads/config.yaml` is tracked repo config only; keep machine-specific federation settings local to your shell environment.
- Store state in specs sparingly. Use `bd` for state.

For orientation in the spec:
- `§1-§3` for model and motivation
- `§12` for CLI surface
- `§15` for implementation patterns and dependencies

For low-context corpus orientation, prefer:
- `anneal status --json --compact`
- `anneal check --active-only`
- `anneal get <handle> --context`
- `anneal find <text> --limit 25`
- `anneal map --around=<handle>`

Avoid broad default dumps like raw `check --json`, empty-query `find --json`, or full-graph renders unless you are intentionally expanding with flags like `--diagnostics`, `--refs`, `--nodes`, or `--full`.

## Rust Toolchain

- `rust-toolchain.toml` pins Rust 1.94.0 with `rustfmt` and `clippy`.
- Use `just`. `just check` is the default gate (fmt + `install.sh` syntax + clippy + test, each step timed). Inspect `justfile` or run `just --list` for the full command surface.
- `just build` for a release binary; `just release-verify` for release-readiness gates.
- Add dependencies with `cargo add`; never hand-write version strings.
- `ast-grep run -p 'PATTERN' -l rust` for AST-aware code search (no config needed). Useful patterns: `$X.unwrap()`, `todo!($$$)`, `#[allow($$$)]`. Add `-r 'REPLACEMENT'` for structural refactoring; `--json` for machine-readable output.

## Module Boundaries

anneal is a single-crate binary; modules in `src/` own distinct layers.

- Durable pipeline shape: `parse` → `extraction` → `graph` / `resolve` → `analysis` / `lattice` → `checks` → `output` / `cli`.
- `graph` owns the arena; `handle` owns identity types (`Handle`, `NodeId`, `HandleKind`, `EdgeKind`). Don't leak arena internals across module boundaries.
- `lattice` owns terminal/active classification. Do not reintroduce terminal-status heuristics into `orient`, `checks`, or CLI layers — they belong in the lattice.
- `cli/` and `output/` compose analyses into user-facing commands. Analysis logic belongs in its analysis module, not the CLI seam.
- The shipped surfaces are the CLI binary, the tag-driven GitHub release assets, and `install.sh`.

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

- day-to-day work lands on `dev`
- merge `dev` into `master` for release prep
- cut and push release tags from `master`

Before bumping, verify all shipped features are reflected in docs. CLI help strings are authoritative, but these must match:

- `README.md` — command sections, output examples, config blocks, architecture listing
- `skills/anneal/SKILL.md` — first moves, command map, agent rules
- `.design/anneal-spec.md` — §12 command count and entries, §14 architecture diagram, §15 crate structure
- `CHANGELOG.md` — entry for the target version (scaffolded by `release-bump`)

Write docs as if they were always correct — no "added" or "updated" language.

```bash
just release-bump 0.2.1
just release-verify
git add -A && git commit -m "release: prepare v0.2.1"
git push origin master
just release-tag 0.2.1
```

`just release-verify` checks version alignment across `Cargo.toml`, `Cargo.lock`, and `flake.nix`; CHANGELOG entry presence without `TODO`/`TBD` placeholders; release target alignment across `release.yml`, `install.sh`, and `README.md`; public-repo safety for `.beads/config.yaml`; then runs `just check`, `just build`, `anneal --version`, and `anneal --root .design check`.

Pushing `vX.Y.Z` triggers `.github/workflows/release.yml` and publishes binaries for:
- `aarch64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

## Test Corpus

Primary real-world corpus: `~/code/murail/.design/`. Useful for smoke-checking `status`, `get`, `check --file`, `map`, `impact`, and `obligations`. Integration tests may skip if the external corpus is unavailable.

## Hooks And Completion

- `.git/hooks/pre-commit` runs `just check` after the beads integration block.
- Before ending a session:
  1. Run `just check` if code changed.
  2. Commit with a clear message.
  3. `bd dolt push`.
  4. `git push`.
- Work is not complete until `git push` succeeds.

## Reminders

- Low context in, high signal out.
- Write it as if it was always right.
- Design it like you got it right the first time.
