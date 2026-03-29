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

- [ ] Check five local consistency rules: existence, staleness, confidence gap, linearity, convention adoption
- [ ] Track obligations as linear handles (must be discharged exactly once)
- [ ] Compute impact analysis: reverse graph traversal from a changed handle
- [ ] Track convergence via append-only JSONL snapshots with summary (advancing/holding/drifting)
- [ ] Compute graph diffs between snapshots
- [ ] Eight CLI commands: check, get, find, status, map, init, impact, diff
- [ ] All commands support --json output for agent consumption
- [ ] Suggestions: detect patterns and propose structural improvements from graph analysis

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
- **Dependencies**: Minimal — 10 crates (anyhow, clap, serde, serde_json, serde_yaml_ng, toml, regex, walkdir, camino, chrono)
- **No heavy deps**: Hand-roll graph (~135 lines), frontmatter split (~15 lines), JSONL (~30 lines) instead of petgraph, gray_matter, jsonl crates
- **Build time**: Target <10s clean build
- **Performance**: <100ms for full pipeline on ~300 files

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Local checks over global propagation | Knowledge graphs are shallow (~3 hops); local checks catch the same issues without cascade false positives | — Pending |
| Hand-roll graph instead of petgraph | Only need traversal + toposort + reachability; 135 lines vs 1.5s compile cost for 5% of petgraph surface | Validated Phase 1 (131 lines) |
| Cites vs DependsOn edge distinction | Not all references are dependencies; formal model citing OQ-64 shouldn't have its grade affected by OQ status | — Pending |
| serde_yaml_ng for frontmatter | Maintained fork of archived serde_yaml; manual ---/--- split is ~15 lines | Validated Phase 1 |
| Convergence tracking via JSONL snapshots | Append-only, derived, deletable; enables status --history and diff without persistent database | — Pending |
| anneal.toml optional with inference-first | Zero-config must work (existence lattice); config only overrides inference | Validated Phase 1 |

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

## Current State

Phase 1 complete — graph foundation is working end-to-end. Running `cargo run -- --root ~/code/murail/.design/` produces 9788 handles, 6408 edges, 22 namespaces from 259 files. The pipeline: parse files → infer namespaces → resolve labels/versions/edges → infer lattice → print stats. Quality gate (`just check`) passes.

---
*Last updated: 2026-03-29 after Phase 1 completion*
