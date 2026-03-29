# Roadmap: anneal

**Created:** 2026-03-28
**Phases:** 3 (coarse granularity)
**Requirements:** 48 v1 requirements mapped

## Phase Overview

| # | Phase | Goal | Requirements | Success Criteria |
|---|-------|------|--------------|------------------|
| 1 | Graph Foundation | 3/3 | Complete   | 2026-03-29 |
| 2 | Checks & CLI | 3/3 | Complete | 2026-03-29 |
| 3 | Convergence & Polish | Convergence tracking, suggestions, remaining commands | 13 (CONVERGE-*, CLI-04/05/08, SUGGEST-*) | 1/5 in progress |

## Phase Details

### Phase 1: Graph Foundation

**Goal:** Parse a directory of markdown files, build the knowledge graph with handles and edges, resolve handles across namespaces.

**Requirements:** GRAPH-01, GRAPH-02, GRAPH-03, GRAPH-04, GRAPH-05, GRAPH-06, HANDLE-01, HANDLE-02, HANDLE-03, HANDLE-04, HANDLE-05, HANDLE-06, LATTICE-01, LATTICE-02, LATTICE-03, LATTICE-04, CONFIG-01, CONFIG-02

**Plans:** 3/3 plans complete

Plans:
- [x] 01-01-PLAN.md -- Types & Config: Handle, AnnealConfig, DiGraph foundational types
- [x] 01-02-PLAN.md -- Parse & Lattice: Frontmatter split, RegexSet scanner, edge kind inference, convergence lattice
- [x] 01-03-PLAN.md -- Resolution & Wiring: Namespace inference, handle resolution, main.rs pipeline

**Success Criteria:**
1. Running anneal on Murail's .design/ directory (265 files) produces a graph with ~500 handles and ~2000 edges in <100ms
2. Label handles in confirmed namespaces (OQ, FM, A, SR, etc.) resolve correctly; false positives (SHA-256, GPT-2) are excluded
3. Frontmatter status values are parsed and partitioned into active/terminal sets
4. The graph is ephemeral -- no persistent state created beyond the optional anneal.toml

**Dependencies:** None -- this is the foundation.

**Key implementation:**
- `handle.rs`: Handle, HandleKind, HandleId types
- `graph.rs`: DiGraph with dual adjacency lists (fwd + rev), Edge, EdgeKind
- `lattice.rs`: Lattice trait, ConvergenceState, active/terminal partition
- `parse.rs`: Frontmatter split + RegexSet scanning (5 patterns)
- `resolve.rs`: Handle resolution across namespaces
- `config.rs`: anneal.toml with #[serde(default, deny_unknown_fields)]
- `main.rs`: Minimal CLI skeleton (cargo run -- parses and prints graph stats)

---

### Phase 2: Checks & CLI

**Goal:** Implement the five local consistency rules, impact analysis, and the core CLI commands that agents need.

**Requirements:** CHECK-01, CHECK-02, CHECK-03, CHECK-04, CHECK-05, CHECK-06, IMPACT-01, IMPACT-02, IMPACT-03, CLI-01, CLI-02, CLI-03, CLI-06, CLI-07, CLI-09, CLI-10, CONFIG-03, CONFIG-04

**Plans:** 3/3 plans complete

Plans:
- [x] 02-01-PLAN.md -- Foundation repairs + config extensibility: fix resolution gaps, extensible frontmatter mapping, terminal status wiring
- [x] 02-02-PLAN.md -- Check rules + impact analysis: five local consistency rules (KB-R1..R5), diagnostics, reverse graph traversal
- [x] 02-03-PLAN.md -- CLI subcommands: check, get, find, init, impact with --json support and CommandOutput pattern

**Success Criteria:**
1. `anneal check` on Murail's .design/ reports real broken references and stale references with compiler-style diagnostics
2. `anneal get OQ-64` resolves the label, shows its definition, state, and references
3. `anneal impact formal-model/v17.md` shows the files that depend on the formal model
4. `anneal init` generates an anneal.toml from inferred structure that matches Murail's conventions
5. All commands produce valid JSON with `--json` flag

**Dependencies:** Phase 1 (graph must exist to check it).

**Key implementation:**
- `checks.rs`: Five rules (KB-R1 through KB-R5), diagnostic types with error codes
- `impact.rs`: Reverse graph traversal with cycle detection
- `cli.rs`: clap derive with 5 subcommands, global --json flag, CommandOutput pattern
- `config.rs`: Extensible frontmatter field mapping (FrontmatterConfig)
- `parse.rs`: Code block label skip, directory convention analysis, table-driven frontmatter
- `resolve.rs`: Bare filename resolution wired

---

### Phase 3: Convergence & Polish

**Goal:** Add convergence tracking (the feature that makes anneal more than a linter), suggestions, and remaining commands.

**Requirements:** CONVERGE-01, CONVERGE-02, CONVERGE-03, CONVERGE-04, CONVERGE-05, CLI-04, CLI-05, CLI-08, SUGGEST-01, SUGGEST-02, SUGGEST-03, SUGGEST-04, SUGGEST-05

**Plans:** 5 plans

Plans:
- [x] 03-01-PLAN.md -- Snapshot infrastructure: Snapshot type, JSONL I/O, convergence summary, Severity::Suggestion
- [ ] 03-02-PLAN.md -- Suggestion engine: five suggestion rules (S001-S005) + check command filter flags
- [ ] 03-03-PLAN.md -- Map command: text and DOT rendering, --concern and --around subgraph extraction
- [ ] 03-04-PLAN.md -- Status command: dashboard with pipeline histogram, convergence summary, snapshot append
- [ ] 03-05-PLAN.md -- Diff command: snapshot-based and git-aware graph diff, three reference modes

**Success Criteria:**
1. `anneal status` shows pipeline histogram, convergence summary (advancing/holding/drifting), and top suggestions
2. `anneal diff` shows what changed in the knowledge graph since last session
3. `anneal map` renders the active knowledge graph (at least text format; --format=dot for graphviz)
4. Suggestions detect at least: orphaned handles, pipeline stalls, and abandoned namespaces

**Dependencies:** Phase 2 (checks must work for convergence tracking to be meaningful).

**Key implementation:**
- `snapshot.rs`: JSONL append/read, convergence summary computation, graph diff
- Enhance `cli.rs`: status, map, diff commands
- Suggestion engine in `checks.rs` or new `suggest.rs`

---

## Verification

- **Phase 1 verification:** Build graph from Murail's .design/, assert handle/edge counts, benchmark <100ms
- **Phase 2 verification:** Run checks on .design/, compare results against known issues (stale v16 references, etc.)
- **Phase 3 verification:** Run status across multiple sessions, verify convergence summary reflects actual changes

---
*Roadmap created: 2026-03-28*
*Last updated: 2026-03-29 after Plan 03-01 completion*
