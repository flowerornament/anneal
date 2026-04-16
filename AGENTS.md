# anneal

Convergence assistant for knowledge corpora.

`anneal` reads a markdown corpus, computes a typed knowledge graph, checks local consistency, and tracks convergence over time.

## Key Files

- `.design/anneal-spec.md` — authoritative product and implementation spec
- `.design/anneal.toml` — self-check config for anneal's own spec corpus
- `.design/.anneal/` — optional repo-local anneal state when explicitly configured
- `.planning/ROADMAP.md` — roadmap and milestone spine
- `.planning/STATE.md` — current project state
- `install.sh` — shipped installer for release binaries
- `.beads/config.yaml` — tracked repo config only; keep federation settings local

For orientation in the spec, read:
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

# query
bd query "type=bug AND priority<=1 AND updated>7d"
bd search "keyword"
bd count "status=open"
bd graph --compact <id>

# state
bd kv set/get key [value]
bd find-duplicates
```

Full ref: `bd prime`

Keep machine-specific federation settings out of the repo. For anneal, the local remote should come from your shell environment rather than tracked `.beads/config.yaml`.

## Build & Quality

Use `just` for routine work:

```bash
just check          # fmt + installer syntax + clippy + test
just build          # cargo build --release
just release-verify # release readiness checks
```

Toolchain is pinned in `rust-toolchain.toml` to Rust 1.94.0 with `rustfmt` and `clippy`.
Workspace lint policy denies Clippy `all` and `pedantic` with a small set of targeted allows in `Cargo.toml`.

The shipped surfaces are the CLI binary, the tag-driven GitHub release assets, and `install.sh`. Treat installer correctness as release-critical.

## Release Flow

Release automation is local-first and tag-driven:

- day-to-day work lands on `dev`
- merge `dev` into `master` for release prep
- cut and push release tags from `master`

Before bumping, verify that all shipped features are reflected in docs — CLI help strings are authoritative, but these must match:

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

`just release-bump` also scaffolds a `CHANGELOG.md` entry for the target version.

`just release-verify` checks:
- version alignment across `Cargo.toml`, `Cargo.lock`, and `flake.nix`
- release entry present in `CHANGELOG.md` with at least one bullet and no `TODO`/`TBD` placeholders
- release target alignment across `release.yml`, `install.sh`, and `README.md`
- public-repo safety for `.beads/config.yaml`
- `just check`
- `just build`
- `./target/release/anneal --version`
- `./target/release/anneal --root .design check`

Pushing `vX.Y.Z` triggers `.github/workflows/release.yml`, which publishes binaries for:
- `aarch64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

## Test Corpus

Primary real-world corpus: `~/code/murail/.design/`

- useful for smoke-checking `status`, `get`, `check --file`, `map`, `impact`, and `obligations`
- integration tests may skip if the external corpus is unavailable

## Completion

Before ending a session:
1. Run `just check` if code changed.
2. Commit with a clear message.
3. `bd dolt push && git push`

Work is not complete until `git push` succeeds.
