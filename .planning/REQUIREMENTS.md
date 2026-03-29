# Requirements: anneal

**Defined:** 2026-03-28
**Core Value:** An arriving agent can immediately understand what's settled, what's drifting, what's connected, and where to push next.

## v1.0 Requirements (validated)

All 48 v1.0 requirements validated across 3 phases. See git history for full list.

### Summary

- Graph construction: 6 requirements (GRAPH-01..06) — Complete Phase 1
- Handle resolution: 6 requirements (HANDLE-01..06) — Complete Phase 1
- Convergence lattice: 4 requirements (LATTICE-01..04) — Complete Phase 1
- Local checks: 6 requirements (CHECK-01..06) — Complete Phase 2
- Impact analysis: 3 requirements (IMPACT-01..03) — Complete Phase 2
- Convergence tracking: 5 requirements (CONVERGE-01..05) — Complete Phase 3
- CLI commands: 10 requirements (CLI-01..10) — Complete Phases 2-3
- Configuration: 4 requirements (CONFIG-01..04) — Complete Phases 1-2
- Suggestions: 5 requirements (SUGGEST-01..05) — Complete Phase 3

## v1.1 Requirements

### Extraction Pipeline

- [ ] **EXTRACT-01**: Introduce `FileExtraction` type as uniform extraction output from both frontmatter and body scanning
- [ ] **EXTRACT-02**: Introduce `DiscoveredRef` with `RefHint` enum replacing `PendingEdge`, `LabelCandidate`, `FrontmatterEdge`, `ScanResult`
- [ ] **EXTRACT-03**: Introduce `SourceSpan` with mandatory line numbers on all discovered references
- [ ] **EXTRACT-04**: Build `LineIndex` from full file content for O(log n) byte-to-line lookup with frontmatter offset adjustment
- [ ] **EXTRACT-05**: Plausibility filter rejects absolute paths, freeform prose, and wildcard patterns from frontmatter edge targets
- [ ] **EXTRACT-06**: URLs in frontmatter classified as `RefHint::External` (not silently skipped)
- [ ] **EXTRACT-07**: Replace regex body scanner with pulldown-cmark 0.13 event walker (ENABLE_HEADING_ATTRIBUTES + ENABLE_WIKILINKS)
- [ ] **EXTRACT-08**: Concatenate text events per block element before applying regex patterns (label, section ref, version)
- [ ] **EXTRACT-09**: Extract markdown links and wiki-links as `DiscoveredRef` from pulldown-cmark Link events
- [ ] **EXTRACT-10**: Skip extraction inside code blocks (structural via pulldown-cmark) and inline code spans
- [ ] **EXTRACT-11**: Scan HTML block content with regex patterns (pragmatic: anneal's job is finding references)

### Resolution

- [ ] **RESOLVE-01**: Introduce `Resolution` enum (Exact / Fuzzy / Unresolved) with candidate collection
- [ ] **RESOLVE-02**: Resolution cascade: exact match -> root-prefix strip -> bare filename -> version stem -> zero-pad normalize
- [ ] **RESOLVE-03**: Root-prefix resolution (`.design/foo.md` -> `foo.md`) for corpus-relative path mismatches
- [ ] **RESOLVE-04**: Version stem resolution (`formal-model-v11.md` -> suggest `formal-model-v17.md` if latest exists)
- [ ] **RESOLVE-05**: Zero-pad label normalization (`OQ-01` resolves to `OQ-1`)
- [ ] **RESOLVE-06**: Unresolved references carry candidate list for diagnostic enrichment (structural transforms only, no fuzzy edit distance)

### Diagnostics

- [ ] **DIAG-01**: All diagnostics carry mandatory `SourceSpan` (file + line number, never null)
- [ ] **DIAG-02**: Introduce `Evidence` enum on `Diagnostic` for structured check results
- [ ] **DIAG-03**: E001 diagnostics include resolution candidates ("similar handle exists: subdir/foo.md")
- [ ] **DIAG-04**: JSON output changes are additive-only (new fields allowed, existing fields preserve type and presence)
- [ ] **DIAG-05**: Human output stays terse by default (line number is the only new addition to default format)

### CLI & UX

- [ ] **UX-01**: `--active-only` available as config opt-in via `[check] default_filter = "active-only"` in anneal.toml
- [ ] **UX-02**: Content snippet in `anneal get` output (first paragraph for files, heading context for labels)
- [ ] **UX-03**: Smarter `anneal init` terminal inference from status name heuristics (superseded, archived, retired, etc.)
- [ ] **UX-04**: Default `--depth=1` for `anneal map --around` (currently 2, too verbose on large corpora)
- [ ] **UX-05**: `--file=<path>` filter for `anneal check` to scope diagnostics to one file
- [ ] **UX-06**: `anneal obligations` command showing linear namespace status (outstanding/discharged/mooted)

### Configuration

- [ ] **CONFIG-01**: `[suppress]` section in anneal.toml for false positive suppression (patterns + identities)
- [ ] **CONFIG-02**: `HandleKind::External` for URL references (participate in graph, skip convergence tracking)

### Quality

- [ ] **QUALITY-01**: Parallel-run comparison of old regex scanner vs new pulldown-cmark scanner on Murail and Herald corpora before removing regex
- [ ] **QUALITY-02**: Self-check: `anneal --root .design/ check` on anneal's own spec passes cleanly
- [ ] **QUALITY-03**: S003 pipeline stall diagnostic refined to use temporal signal (snapshot history) rather than static edge counting

## v2 Requirements

### Search

- **SEARCH-01**: Vector/semantic search backend for `anneal find`
- **SEARCH-02**: Ranked results by relevance + convergence state

### Integration

- **INTEG-01**: MCP server wrapping all commands as tools
- **INTEG-02**: Pre-commit hook integration via just target

### Extended Scanning

- **SCAN-01**: Comment scanning in non-markdown files (Agda, Lean, Rust)
- **SCAN-02**: Extractor trait for multi-format support (KB-OQ5)

### Extended Analysis

- **ANAL-01**: Full Kleene propagation as opt-in mode
- **ANAL-02**: Coherence proxy metrics (decision stability, session orientation speed)

## Out of Scope

| Feature | Reason |
|---------|--------|
| Content heuristics for suggestions | Fragile, language-dependent; structural analysis is reliable |
| ML models for semantic search | Adds 2GB+ dependencies; full-text is v1, vector search is v2 |
| Database storage for graph | Graph is computed from files; statelessness is a design principle (KB-P1) |
| Document creation/editing | anneal reads and reports; it doesn't write content documents |
| Process enforcement | anneal reports state; it doesn't gate transitions (KB-P4) |
| Heavy diagnostic crates (codespan, miette) | Over-engineered for single-location diagnostics; hand-roll SourceSpan |
| Generic fuzzy matching (strsim/Levenshtein) | Deterministic structural transforms are more reliable for handle IDs |
| Changing --active-only to default | Breaks CI scripts and poisons convergence snapshots; config opt-in instead |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| EXTRACT-01 | Phase 4 | Pending |
| EXTRACT-02 | Phase 4 | Pending |
| EXTRACT-03 | Phase 5 | Pending |
| EXTRACT-04 | Phase 5 | Pending |
| EXTRACT-05 | Phase 4 | Pending |
| EXTRACT-06 | Phase 4 | Pending |
| EXTRACT-07 | Phase 5 | Pending |
| EXTRACT-08 | Phase 5 | Pending |
| EXTRACT-09 | Phase 5 | Pending |
| EXTRACT-10 | Phase 5 | Pending |
| EXTRACT-11 | Phase 5 | Pending |
| RESOLVE-01 | Phase 4 | Pending |
| RESOLVE-02 | Phase 6 | Pending |
| RESOLVE-03 | Phase 6 | Pending |
| RESOLVE-04 | Phase 6 | Pending |
| RESOLVE-05 | Phase 6 | Pending |
| RESOLVE-06 | Phase 6 | Pending |
| DIAG-01 | Phase 6 | Pending |
| DIAG-02 | Phase 6 | Pending |
| DIAG-03 | Phase 6 | Pending |
| DIAG-04 | Phase 6 | Pending |
| DIAG-05 | Phase 6 | Pending |
| UX-01 | Phase 6 | Pending |
| UX-02 | Phase 7 | Pending |
| UX-03 | Phase 7 | Pending |
| UX-04 | Phase 7 | Pending |
| UX-05 | Phase 7 | Pending |
| UX-06 | Phase 7 | Pending |
| CONFIG-01 | Phase 7 | Pending |
| CONFIG-02 | Phase 7 | Pending |
| QUALITY-01 | Phase 5 | Pending |
| QUALITY-02 | Phase 7 | Pending |
| QUALITY-03 | Phase 7 | Pending |

**Coverage:**
- v1.1 requirements: 33 total
- Mapped to phases: 33/33
- Unmapped: 0

---
*Requirements defined: 2026-03-29*
*Last updated: 2026-03-29 after v1.1 roadmap creation*
