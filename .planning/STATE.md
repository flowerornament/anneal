---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: Programmable Corpus Runtime
status: planning
stopped_at: Master spec at .design/2026-05-13-corpus-runtime.md. Phase 1 closure work (anneal-apa) ready.
last_updated: "2026-05-13T00:00:00Z"
last_activity: 2026-05-13
progress:
  total_phases: 4
  completed_phases: 0
  total_plans: 0
  completed_plans: 0
  percent: 0
---

# State: anneal

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-31)

**Core value:** An arriving agent can immediately understand what's settled, what's drifting, what's connected, and where to push next.
**Current focus:** v2.0 Programmable Corpus Runtime — substrate (Datalog + convergence stdlib) decoupled from sources (markdown today; mdx, code, host-embed v2.1+). The same agent skills work across every corpus the substrate can ingest. Master spec: .design/2026-05-13-corpus-runtime.md.

## Current Position

Milestone: v2.0 Programmable Corpus Runtime
Phase: 1 closure — READY TO START (parity-runner, dynamic-IR bench, fixtures snapshot, unsafe audit)
Status: Master spec landed 2026-05-13. Engine-viability question decided (ascent for primitives + dynamic IR for rules).
Last activity: 2026-05-13

Progress: [          ] 0% (0/4 phases complete)

**v2.0 phase decomposition (matches master spec layering):**

| Phase | Issue | What | Blocks |
|---|---|---|---|
| 0 closure | `anneal-apa` (P1) | parity-runner, fixtures snapshot, unsafe audit, dynamic-IR bench | Phase 1 |
| 1 Foundation | `anneal-xu2` (P1) | workspace, Source trait, stored relations, generation tracking, anneal-md | Phase 2 |
| 2 Runtime | `anneal-jqh` (P1) | parser, IR, fixpoint, stratification, NDJSON output | Phase 3 |
| 3 Primitives | `anneal-f2b` (P1) | graph, lifecycle, obligations, aggregation, time travel | Phase 4, 5, 9 |
| 4 Content | `anneal-9yl` (P1) | `*content`/`*span`, search + Ranker, read, match | Phase 6, 8 |
| 5 Self-description | `anneal-1gy` (P1) | schema, predicates, verbs, describe, source_of | Phase 6 |
| 6 Standard library | `anneal-1xb` (P1) | graph.dl, convergence.dl, checks.dl, ranking.dl, views.dl | Phase 7, 10 |
| 7 Project extension | `anneal-7it` (P1) | anneal.dl loader, @verb, adapter-qualified discovery | Phase 10 |
| 8 Trails | `anneal-t10` (P1) | `*trail` capture, TrailSummarizer, persistence, --explain | Phase 10 |
| 9 Capability/Policy | `anneal-m08` (P1) | ActorContext, capability gating, Policy trait | Phase 10 |
| 10 Surfaces | `anneal-toe` (P1) | anneal-cli, anneal-mcp, anneal init | Phase 11 |
| 11 Migration | `anneal-px9` (P2) | parity, fixtures, dual-CLI, docs | (epic close) |

**Epic:** `anneal-rsx` (v2.0 — Programmable Corpus Runtime)

**Supporting issues kept open:**
- `anneal-10c` (P2): SP-Q literal-query conformance — gates Phase 11
- `anneal-bmq` (P2): I001 misclassification — becomes a checks.dl rule fix in Phase 6
- `anneal-aj8` (P3): test_large-corpus_corpus drift — subsumed by `apa` fixture snapshot
- `anneal-6uy` (P3, in_progress): newtype wrappers — orthogonal Rust internals
- LR-OQs that survive: `anneal-23w` (TakeUntil semantics), `anneal-kys` (multi-corpus federation, v2.2), `anneal-nty` (section parent_file), `anneal-s74` (perf ceiling)

**Issues closed in the reframe:** anneal-9pg/wq6/tu3/7gi (old 4-phase decomposition), anneal-2gf/7t8/35s (agent ergonomics — folded into master scope), anneal-d6r (naming clash with context verb), anneal-46t/qz7 (resolved by spec refinements), anneal-pqj/4bt (placeholder; absorbed into Phase 1/8), anneal-0he (Phase 0 viability decided).

## Decisions

Recent decisions affecting current work (full log in PROJECT.md):

- [v2.0 reframe, 2026-05-13]: Product story reframed from "collapse 14 commands into Datalog + 7 verbs" to "programmable knowledge-corpus runtime for agents: searchable content, typed relations, explainable views, and saved verbs" (.design/2026-05-13-primitives-first-corpus-vm.md). Architecture from 2026-05-03 unchanged; product framing, verb model, onboarding default, and SP-DR1 capstone gate updated.
- [v2.0 reframe]: `*content`, `*span`, `*search_hit`, `search`, `read`, `schema`, `describe`, `source`, `top_k` join the engine primitives layer (CV-D2, CV-D3). Agent-ergonomics epic anneal-2gf folds into v2.0; search and MCP promote from P3 to P1.
- [v2.0 reframe]: Verbs become saved templates under Steele's criterion (CV-R1) — project verbs syntactically indistinguishable from prelude verbs. The "seven verbs" target is demoted to "starter verbs the prelude happens to ship."
- [v2.0 reframe]: SP-DR1 capstone gate becomes workflow-completion (CV-R2) — cold agent reaches answer in ≤2 tool calls on the large-corpus conformance task — not MVS coverage alone.
- [v2.0 reframe]: Onboarding defaults to lattice-on (CV-S2) — `anneal init` always scaffolds a minimal lattice rather than landing in "graph mode" until the user configures one.
- [v2.0]: Engine architecture revised per engine-spike findings — ascent for engine-derived primitives only; a dynamic IR owns the rule layer (prelude + project + inline together). See .design/2026-05-13-engine-spike-results.md §SR-A.
- [v2.0]: Language-first redesign — original spec at .design/2026-05-03-language-redesign.md merged via PR #2 on 2026-05-03. Architecture (engine/prelude/project, Datalog grammar, convergence vocabulary, diagnostic ID rules) remains authoritative; product framing superseded 2026-05-13.
- [v2.0]: Specs `2026-04-02-cli-output-audit`, `2026-04-02-progressive-disclosure-output-spec`, `2026-04-02-query-explain-spec`, `2026-04-15-areas-orient-garden`, `2026-04-15-cli-ux-audit`, `2026-04-17-cli-ux-audit-v2`, `2026-04-21-orient-frontier-foundation` marked superseded — their CLI-surface concerns moot or absorbed into the language redesign
- [v2.0]: Spec `2026-04-30-agent-ergonomics-retrieval-layer` retained as draft — search/context-annotations/MCP are orthogonal to the datalog redesign and tracked under the agent-ergonomics epic
- [v2.0]: Eleven CLI-surface bd issues (`anneal-2o4`, `t78`+children, `86k`, `33o`, `7i8`, `b2h`, `djb`, `hr9`, `xdu`) closed as superseded; `anneal-bmq` retained because the I001 fix becomes a rule edit in checks.dl
- [v1.1]: pulldown-cmark 0.13 replaces regex body scanner (research validated)
- [v1.1]: Do NOT enable ENABLE_YAML_STYLE_METADATA_BLOCKS (conflicts with split_frontmatter)
- [v1.1]: Deterministic structural transforms only for resolution, no fuzzy edit distance
- [post-v1.1 backlog cleanup]: `anneal check` now defaults to active-file diagnostics; `--include-terminal` opts back into the full picture
- [v1.1]: JSON schema changes additive-only (new nullable fields, no type changes)
- [Phase 04-types-plausibility]: Compound label regex with optional hyphen supports KB-D1 style prefixes
- [Phase 04-types-plausibility]: Classification cascade: comma-list before prose check to prevent misclassification
- [Phase 04-types-plausibility]: ImplausibleRef/ExternalRef as parse.rs structs to avoid circular dependency with checks.rs
- [Phase 04]: Dual-pass over field_edges: existing PendingEdge flow untouched, new DiscoveredRef flow added in parallel
- [Phase 04]: RefSource::Frontmatter field from EdgeKind::as_str() since FrontmatterEdge lacks field name
- [Phase 05]: classify_body_ref as separate simpler classifier for body references from regex/link events
- [Phase 05]: scan_file retained pub(crate) for parallel-run comparison tests
- [Phase 05]: FileExtraction construction moved after scan_file_cmark to include body refs
- [Phase 06]: ScanResult file_refs/section_refs carry (String, u32) tuples for line numbers
- [Phase 06]: CheckConfig.default_filter is Option<String> for future-proofing
- [Phase 06]: COMPOUND_LABEL_RE regex for zero-pad normalization of compound label prefixes
- [Phase 06]: Root-prefix strip creates graph edges; version-stem and zero-pad produce candidates only
- [Phase 06]: Evidence uses serde(tag=type) for internally-tagged JSON serialization
- [Phase 06]: E001 always gets Evidence::BrokenRef even with empty candidates for JSON consistency
- [Phase 06]: Line 1 for all frontmatter-sourced diagnostics; Version handles resolve file_path through artifact field
- [Phase 07]: Suppressions are applied after run_checks and before snapshot generation so human output and recorded diagnostics stay aligned. — Filtering once after diagnostics are assembled avoids duplicating suppression logic across individual check rules and keeps snapshot counts consistent with displayed output.
- [Phase 07]: External URLs reuse one graph node per URL identity while each source file still emits its own Cites edge. — Deduplicating URL nodes preserves handle identity uniqueness and prevents ambiguous lookups when multiple files cite the same external reference.
- [Phase 07]: Snippets are read on demand from source files so the graph stays lean and get output can add context without storing document bodies.
- [Phase 07]: Obligation reporting groups IDs by configured linear namespace and still returns a valid empty JSON payload when no linear namespaces are configured.
- [Phase 07]: File scoping stays after suppressions so displayed diagnostics and snapshots are filtered the same way.
- [Phase 07]: Temporal S003 compares the current status population with the latest snapshot and falls back to static edge analysis when no history exists.
- [Phase 07]: Used a targeted E001 suppress rule in .design/anneal.toml instead of editing anneal-spec.md because synthesis/v17.md is an illustrative prose example, not corpus truth.

## Performance Metrics

| Phase | Plan | Duration | Tasks | Files |
| ----- | ---- | -------- | ----- | ----- |

| Phase 07 P01 | 6 min | 2 tasks | 7 files |
| Phase 07 P02 | 9 min | 2 tasks | 2 files |
| Phase 07 P03 | 4 min | 2 tasks | 5 files |
| Phase 07 P04 | 2 min | 1 task | 1 file |

## Session Continuity

Last session: 2026-03-31T02:57:56Z
Stopped at: Completed 07-VERIFICATION.md
Resume file: None

---
*Last updated: 2026-03-31 after Phase 7 verification*
