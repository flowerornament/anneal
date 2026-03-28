# anneal

Convergence assistant for knowledge corpora. Reads markdown files, computes a typed knowledge graph, checks local consistency, and tracks convergence over time.

## `.planning/` — GSD Execution Tracking

| Path              | Description                                        |
| ----------------- | -------------------------------------------------- |
| `ROADMAP.md`      | Canonical execution spine. **Planning authority.**  |
| `PROJECT.md`      | Project definition and context.                     |
| `REQUIREMENTS.md` | 48 v1 requirements across 9 categories.             |
| `STATE.md`        | Current GSD execution state.                        |
| `config.json`     | GSD configuration.                                  |

## `.design/` — Design Documents

| Path              | Description                                        |
| ----------------- | -------------------------------------------------- |
| `anneal-spec.md`  | Full specification (933 lines, 66 labels). **Authoritative.** |

The spec derives from theory (Part I) through core model (Part II) through CLI surface (Part IV) through implementation (Part VI). Read §1-§3 for orientation, §12 for commands, §15 for implementation patterns.

### Key Labels

| Prefix | Namespace | Count | Description |
| ------ | --------- | ----- | ----------- |
| KB-F   | Foundations | 5 | Theoretical lineage (One Loop, graded types, propagators, linear logic, coloring book) |
| KB-P   | Principles | 8 | Design principles (files are truth, everything is a handle, inference first, ...) |
| KB-D   | Definitions | 20 | Core model definitions (Handle, Graph, Lattice, Checks, ...) |
| KB-R   | Rules | 5 | Local consistency check rules |
| KB-E   | Emergent | 10 | Derived capabilities (reference checking, staleness, pipeline tracking, ...) |
| KB-C   | Commands | 8 | CLI commands (check, get, find, status, map, init, impact, diff) |
| KB-OQ  | Open Questions | 8 | Unresolved design questions |

## Task Tracking

This project uses **GSD + beads**: GSD for roadmap planning, `bd` for issue execution tracking.

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

## Build & Quality

This is a **Rust** project. Use `just` for all build operations.

```bash
just              # List available commands
just check        # fmt + clippy + test (the quality gate)
just fmt          # cargo fmt
just lint         # cargo clippy --all-targets
just test         # cargo test
just build        # cargo build --release
```

### Toolchain

Pinned to Rust 1.94.0 stable, edition 2024 via `rust-toolchain.toml`. Components: rustfmt, clippy.

### Quality Gate

A pre-commit hook runs `just check` before every `git commit`. If fmt, clippy, or tests fail, the commit is blocked.

### Dependencies

10 crates, ~10s clean build. See `.design/anneal-spec.md` §15.1 for the full list with rationale.

Key decisions:
- Hand-roll graph (~135 lines) instead of petgraph
- Hand-roll frontmatter split (~15 lines) instead of gray_matter
- Hand-roll JSONL (~30 lines) instead of jsonl crate
- `serde_yaml_ng` (maintained fork) instead of `serde_yaml` (archived)

### Rust Code Style

- **Edition 2024** with Rust 1.94.0 stable
- **`unsafe` is denied** workspace-wide
- **Clippy**: all + pedantic denied, with targeted allows for noisy lints
- **No `unwrap()` in production** — use `expect("reason")` or propagate with `?`
- **`--json` on every command** — `CommandOutput` trait: `Serialize` + `print_human()`

## Session Workflow

### Starting
```bash
bd prime              # Load context
bd ready              # See available work
anneal status         # (once built) Check corpus state
```

### Ending
```
[ ] just check                (quality gate)
[ ] git add <files> && git commit
[ ] bd dolt push
[ ] git push
```

**Work is NOT complete until `git push` succeeds.**

## Test Corpus

The primary test corpus is Murail's `.design/` directory at `~/code/murail/.design/`:
- 265 markdown files
- 15 label namespaces (OQ, FM, A, SR, DG, ...)
- ~25 status values
- Machine-checked proofs in Agda/Lean

Integration tests point there via path: `anneal --root ~/code/murail/.design/ check`

## Structure

```
anneal/
  src/
    handle.rs       # Handle, HandleKind, HandleId
    graph.rs        # DiGraph with dual adjacency lists
    lattice.rs      # Lattice trait, convergence states
    checks.rs       # Five local check rules (KB-R1..R5)
    linear.rs       # Obligation lifecycle
    impact.rs       # Reverse graph traversal
    snapshot.rs     # JSONL history, convergence summary, diff
    parse.rs        # Frontmatter + RegexSet scanning
    resolve.rs      # Handle resolution
    config.rs       # anneal.toml parsing + inference
    cli.rs          # Eight commands + --json
    main.rs         # Entry point
  .design/
    anneal-spec.md  # Authoritative specification
  .planning/        # GSD execution tracking
```

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->
