# Phase 1: Graph Foundation - Context

**Gathered:** 2026-03-28
**Status:** Ready for planning

<domain>
## Phase Boundary

Parse a directory of markdown files, build the knowledge graph with handles (File, Section, Label, Version) and typed edges (Cites, DependsOn, Supersedes, Verifies, Discharges), resolve handles across namespaces, infer convergence lattice from frontmatter, and provide a minimal CLI that prints graph stats. No persistent state beyond optional anneal.toml. No consistency checks (Phase 2). No convergence tracking (Phase 3).

</domain>

<decisions>
## Implementation Decisions

### Edge Kind Inference
- **D-01:** Implement full keyword-based edge kind inference in Phase 1, not deferred to Phase 2. Both frontmatter fields (`superseded-by:` -> Supersedes, `discharges:` -> Discharges, `verifies:` -> Verifies, `depends-on:` -> DependsOn) AND body-text context keywords (`incorporates`, `builds on`, `extends`, `based on` -> DependsOn; `see also`, `cf.`, `related` -> Cites) are implemented. Default edge kind for unmatched references is Cites.
- **D-02:** This ensures Phase 2 checks have real DependsOn edges to work with immediately, rather than needing to add inference and checks simultaneously.

### Claude's Discretion
- Keyword proximity rule for body text inference (same-line vs same-paragraph) — Claude should choose based on what works best during implementation and testing against the Murail corpus.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Specification
- `.design/anneal-spec.md` — Authoritative specification (933 lines, 66 labels). Read sections:
  - Part I (sections 1-3) for orientation and design principles
  - Section 4 (Handle) for handle kinds, resolution, namespace inference
  - Section 5 (Graph) for edge kinds, graph construction, regex patterns
  - Section 6 (Convergence Lattice) for lattice definitions, active/terminal partition
  - Section 15 for implementation patterns and dependency rationale

### Requirements
- `.planning/REQUIREMENTS.md` — Phase 1 requirements: GRAPH-01..06, HANDLE-01..06, LATTICE-01..04, CONFIG-01..02

### Test Corpus
- `~/code/murail/.design/` — Primary test corpus (265 markdown files, 15 label namespaces, ~25 status values). Integration tests point here by path.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- None — fresh project with only skeleton `main.rs`

### Established Patterns
- Cargo.toml already declares all 10 dependencies with correct versions and features
- Clippy configured with all + pedantic denied, targeted allows for noisy lints
- `unsafe_code = "deny"` workspace-wide
- Edition 2024, Rust 1.94.0 stable

### Integration Points
- `main.rs` — entry point, currently a placeholder println
- `just check` — quality gate (fmt + clippy + test) via pre-commit hook
- Module structure specified in ROADMAP: handle.rs, graph.rs, lattice.rs, parse.rs, resolve.rs, config.rs, cli.rs

</code_context>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches. The spec is highly prescriptive; follow it closely.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 01-graph-foundation*
*Context gathered: 2026-03-28*
