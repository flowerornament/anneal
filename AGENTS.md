# anneal

Convergence assistant for knowledge corpora.

## Key Files

- `.design/anneal-spec.md` — authoritative product and implementation spec
- `.design/anneal.toml` — self-check config for anneal's own spec corpus
- `.design/.anneal/` — derived local history/state; ignored
- `.beads/config.yaml` — tracked repo config only; keep federation settings local

## Build & Quality

Use `just` for routine work:

```bash
just check          # fmt + clippy + test
just build          # cargo build --release
just release-verify # release readiness checks
```

Toolchain is pinned in `rust-toolchain.toml` to Rust 1.94.0 with `rustfmt` and `clippy`.
Workspace lint policy denies Clippy `all` and `pedantic` with a small set of targeted allows in `Cargo.toml`.

## Release Flow

Release automation is local-first and tag-driven:

```bash
just release-bump 0.2.1
just release-verify
git add -A && git commit -m "release: prepare v0.2.1"
git push origin master
just release-tag 0.2.1
```

`just release-verify` checks:
- version alignment across `Cargo.toml`, `Cargo.lock`, and `flake.nix`
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

## Task Tracking

This project uses beads:

```bash
bd ready
bd show <id>
bd update <id> --claim
bd close <id>
bd dolt push
```

Set machine-specific federation config locally with `BD_FEDERATION_REMOTE`.

## Session Completion

Work is not complete until both `bd dolt push` and `git push` succeed.
