# anneal

## What This Is

A convergence assistant for knowledge corpora. anneal reads a directory of markdown files, computes a typed knowledge graph from handles (files, sections, labels, versions), checks it for local consistency, and tracks convergence over time. It helps disconnected intelligences — agents across sessions with no shared memory — orient in a body of knowledge and push it toward settledness.

## Core Value

An arriving agent can immediately understand what's settled, what's drifting, what's connected to what, and where to push next — without reading every README.

## Requirements

### Validated

- [x] Compute a knowledge graph from markdown files (handles as nodes, references as edges) — Phase 1
- [x] Resolve handles uniformly: files, sections (headings), labels (OQ-64), versions (v17) — Phase 1
- [x] Infer handle namespaces from sequential cardinality (OQ-1..OQ-69 is a namespace; SHA-256 is not) — Phase 1
- [x] Parse YAML frontmatter for convergence state (status field) — Phase 1
- [x] Infer active/terminal partition from directory conventions and observed status values — Phase 1
- [x] Optional anneal.toml config; zero-config is valid (existence lattice = reference checking only) — Phase 1

### Active

No active milestone requirements. v1.1 is shipped; see REQUIREMENTS.md for the validated traceability record.

### Validated (Phase 2)

- [x] Check five local consistency rules: existence, staleness, confidence gap, linearity, convention adoption — Phase 2
- [x] Track obligations as linear handles (must be discharged exactly once) — Phase 2
- [x] Compute impact analysis: reverse graph traversal from a changed handle — Phase 2
- [x] Five CLI commands: check, get, find, init, impact with --json — Phase 2

### Validated (Phase 3)

- [x] Track convergence via append-only JSONL snapshots with summary (advancing/holding/drifting) — Phase 3
- [x] Compute graph diffs between snapshots — Phase 3
- [x] Three remaining CLI commands: status, map, diff with --json — Phase 3
- [x] Suggestions: detect patterns and propose structural improvements from graph analysis (S001-S005) — Phase 3

### Validated (Phase 7)

- [x] Enrich orientation commands with content snippets, obligations reporting, and file-scoped checks — Phase 7
- [x] Add smarter terminal-state inference, suppression-backed self-checking, and external URL graph handles — Phase 7
- [x] Use snapshot history to detect pipeline stalls temporally instead of relying only on static edge shape — Phase 7

### Out of Scope

- Semantic/vector search — full-text only in v1; interface designed for future extension (KB-OQ3)
- MCP server — future extension once CLI proves useful (KB-OQ4)
- Non-markdown file parsing — markdown primary, optional comment scanning later (KB-OQ5)
- Full Kleene propagation — local checks sufficient; lattice structure supports it as extension (KB-OQ1)
- Content heuristics for suggestions — structural analysis only, no NLP

## Context

anneal was designed through a deep design conversation exploring the formal analogy between graded type systems (TensorQTT from Murail's formal model) and knowledge management. The core insight: a document's convergence state has the same algebraic structure as a program's resource grade — both are values in bounded lattices that compose through meet/join operations.

The design follows Herald's coloring book principle (C-10): the kernel defines an abstract space (handles, graph, convergence lattice, local checks, linearity); each project is a coloring (which handles exist, which states are valid, which namespaces are linear).

The first corpus to test against is Murail's own .design/ directory: 265 markdown files, 15 label namespaces, ~25 status values, machine-checked proofs in Agda/Lean.

Detailed specification: `.design/anneal-spec.md` (933 lines, 66 labels, 8 namespaces).

The primary test corpus is Murail's `.design/` directory at `~/code/murail/.design/` (265 markdown files, 15 label namespaces). Integration tests point there via path, not structural coupling.

## Constraints

- **Language**: Rust (1.94 stable, edition 2024)
- **Stateless**: Graph computed from files on every invocation; no .anneal/ database
- **One piece of state**: `.anneal/history.jsonl` — append-only convergence snapshots (derived, deletable)
- **Dependencies**: Minimal — 11 crates (anyhow, clap, serde, serde_json, serde_yaml_ng, toml, regex, walkdir, camino, chrono, pulldown-cmark)
- **No heavy deps**: Hand-roll graph (~135 lines), frontmatter split (~15 lines), JSONL (~30 lines) instead of petgraph, gray_matter, jsonl crates
- **Build time**: Target <10s clean build
- **Performance**: <100ms for full pipeline on ~300 files

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Local checks over global propagation | Knowledge graphs are shallow (~3 hops); local checks catch the same issues without cascade false positives | Validated Phase 2 |
| Hand-roll graph instead of petgraph | Only need traversal + toposort + reachability; 135 lines vs 1.5s compile cost for 5% of petgraph surface | Validated Phase 1 (131 lines) |
| Cites vs DependsOn edge distinction | Not all references are dependencies; formal model citing OQ-64 shouldn't have its grade affected by OQ status | Validated Phase 2 |
| serde_yaml_ng for frontmatter | Maintained fork of archived serde_yaml; manual ---/--- split is ~15 lines | Validated Phase 1 |
| Convergence tracking via JSONL snapshots | Append-only, derived, deletable; enables status --history and diff without persistent database | Validated Phase 3 |
| anneal.toml optional with inference-first | Zero-config must work (existence lattice); config only overrides inference | Validated Phase 1 |
| pulldown-cmark over regex for body scanning | Structural markdown parsing avoids false positives in code blocks, handles wiki-links/markdown links natively, provides SourceSpan line numbers | Validated Phase 5 |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition:**
1. Requirements invalidated? Move to Out of Scope with reason
2. Requirements validated? Move to Validated with phase reference
3. New requirements emerged? Add to Active
4. Decisions to log? Add to Key Decisions
5. "What This Is" still accurate? Update if drifted

**After each milestone:**
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

## Current Milestone: v1.1 Parser Hardening & UX Polish (Shipped)

**Goal:** Make `anneal check` output trustworthy and actionable by introducing three missing intermediate types (Extraction, Resolution, Diagnostic enrichment), replacing the regex body scanner with pulldown-cmark, and enriching orientation commands.

**Target features:**
- Typed extraction pipeline with plausibility filtering (URLs, prose, wildcards classified not skipped)
- Resolution cascade with "did you mean?" candidates (root-prefix, bare filename, version stem, zero-pad)
- pulldown-cmark body scanner replacing regex (native markdown/wiki-links, section ref resolution, line numbers)
- Rich diagnostics with mandatory source locations and structured evidence
- Active-only check default, content snippets in `get`, obligations command, smarter init
- External URL handles (HandleKind::External), false positive suppression config

**Architecture evolution:**
- `DiscoveredRef` + `RefHint` replaces `PendingEdge`, `LabelCandidate`, `FrontmatterEdge`, `ScanResult` (4 types → 1)
- `Resolution` enum (Exact/Fuzzy/Unresolved) makes resolution cascade explicit
- `Evidence` enum on Diagnostic makes line numbers, candidates, snippets structurally mandatory
- Extractor function signature clean enough to become a trait when KB-OQ5 arrives

## Current State

v1.1 is shipped. anneal now has a typed extraction/resolution/diagnostic pipeline, pulldown-cmark-based body scanning, deterministic resolution candidates, snippet-enriched orientation commands, obligations reporting, file-scoped checks, smarter init heuristics, suppression-backed self-checking, and temporal S003 pipeline analysis.

Quality is green on the shipped state: `just check` passes, `cargo test` passes with 152 tests in the suite (150 passing, 2 external-corpus smoke tests ignored), and `cargo run --quiet -- --root .design check` exits 0 with only the informational section-notation note.

**Known issues from real-world testing (Murail 262 files, Herald 89 files):**
- Murail: 186 errors → 9 with --active-only (95% are terminal-file noise)
- Herald: 89 errors → 50 with --active-only (remaining are frontmatter URLs/prose false positives)
- S003 pipeline stall fires for every level (needs temporal heuristic via snapshots)
- 255 section references unresolvable in Herald (KB-OQ2)

---
*Last updated: 2026-03-31 after Phase 7 completion*
