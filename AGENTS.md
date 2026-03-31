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

## Task Tracking

This project uses beads for issue tracking:

```bash
bd ready
bd show <id>
bd update <id> --claim
bd close <id>
bd dolt push
```

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

## Session Completion

Work is not complete until both `bd dolt push` and `git push` succeed.
