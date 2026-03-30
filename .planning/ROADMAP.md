# Roadmap: anneal

## Milestones

- [x] **v1.0 MVP** - Phases 1-3 (shipped 2026-03-29)
- [ ] **v1.1 Parser Hardening & UX Polish** - Phases 4-7 (in progress)

## Phases

<details>
<summary>v1.0 MVP (Phases 1-3) - SHIPPED 2026-03-29</summary>

- [x] **Phase 1: Graph Foundation** - Parse markdown files, build knowledge graph with handles and edges, resolve across namespaces
- [x] **Phase 2: Checks & CLI** - Five local consistency rules, impact analysis, core CLI commands
- [x] **Phase 3: Convergence & Polish** - Convergence tracking via JSONL snapshots, suggestions, remaining commands

### Phase 1: Graph Foundation
**Goal**: Parse a directory of markdown files, build the knowledge graph with handles and edges, resolve handles across namespaces.
**Requirements**: GRAPH-01..06, HANDLE-01..06, LATTICE-01..04, CONFIG-01..02
**Plans**: 3/3 plans complete

Plans:
- [x] 01-01-PLAN.md -- Types & Config
- [x] 01-02-PLAN.md -- Parse & Lattice
- [x] 01-03-PLAN.md -- Resolution & Wiring

### Phase 2: Checks & CLI
**Goal**: Implement the five local consistency rules, impact analysis, and the core CLI commands that agents need.
**Requirements**: CHECK-01..06, IMPACT-01..03, CLI-01..03, CLI-06..07, CLI-09..10, CONFIG-03..04
**Plans**: 3/3 plans complete

Plans:
- [x] 02-01-PLAN.md -- Foundation repairs + config extensibility
- [x] 02-02-PLAN.md -- Check rules + impact analysis
- [x] 02-03-PLAN.md -- CLI subcommands

### Phase 3: Convergence & Polish
**Goal**: Add convergence tracking, suggestions, and remaining commands.
**Requirements**: CONVERGE-01..05, CLI-04..05, CLI-08, SUGGEST-01..05
**Plans**: 5/5 plans complete

Plans:
- [x] 03-01-PLAN.md -- Snapshot infrastructure
- [x] 03-02-PLAN.md -- Suggestion engine
- [x] 03-03-PLAN.md -- Map command
- [x] 03-04-PLAN.md -- Status command
- [x] 03-05-PLAN.md -- Diff command

</details>

### v1.1 Parser Hardening & UX Polish (In Progress)

**Milestone Goal:** Make `anneal check` output trustworthy and actionable by introducing typed extraction/resolution/diagnostic pipeline, replacing the regex body scanner with pulldown-cmark, and enriching orientation commands.

- [ ] **Phase 4: Types & Plausibility** - Typed extraction pipeline with plausibility filtering and external URL classification
- [ ] **Phase 5: pulldown-cmark Migration** - Replace regex body scanner with pulldown-cmark event walker, line number tracking
- [ ] **Phase 6: Resolution Cascade** - Deterministic resolution strategies, enriched diagnostics with evidence, active-only config
- [ ] **Phase 7: UX Enrichment** - Content snippets, smarter init, file-scoped checks, obligations command, config suppression, self-check

## Phase Details

### Phase 4: Types & Plausibility
**Goal**: Extraction pipeline produces typed, plausibility-filtered output — frontmatter references are classified not silently skipped, and the extraction boundary is clean enough to swap internals behind it
**Depends on**: Phase 3 (v1.0 complete)
**Requirements**: EXTRACT-01, EXTRACT-02, EXTRACT-05, EXTRACT-06, RESOLVE-01
**Success Criteria** (what must be TRUE):
  1. `anneal check --json` output includes `DiscoveredRef` with `RefHint` classification for every reference (frontmatter and body)
  2. URLs in frontmatter edges appear as `RefHint::External` in extraction output instead of being silently dropped
  3. Absolute paths, freeform prose, and wildcard patterns in frontmatter are rejected with a plausibility diagnostic instead of creating false positive broken-reference errors
  4. All existing tests pass — `just check` green with no behavior change in final diagnostic output
**Plans**: 3 plans

Plans:
- [x] 04-01-PLAN.md -- Extraction types & classify function
- [x] 04-02-PLAN.md -- Plausibility filter wiring & W004 diagnostic
- [x] 04-03-PLAN.md -- Gap closure: wire FileExtraction/DiscoveredRef into production & JSON output

### Phase 5: pulldown-cmark Migration
**Goal**: Body scanning uses pulldown-cmark's structural AST instead of line-by-line regex, giving accurate line numbers and structural code block skipping
**Depends on**: Phase 4 (typed extraction boundary must exist)
**Requirements**: EXTRACT-03, EXTRACT-04, EXTRACT-07, EXTRACT-08, EXTRACT-09, EXTRACT-10, EXTRACT-11, QUALITY-01
**Success Criteria** (what must be TRUE):
  1. `anneal check` on Murail (262 files) and Herald (89 files) produces equal or fewer false positives compared to the regex scanner (parallel-run comparison documented)
  2. Every diagnostic in `anneal check --json` output carries a non-null line number
  3. References inside fenced code blocks and inline code spans are not extracted (structural skip, not regex toggle)
  4. Wiki-links (`[[target]]`) and standard markdown links are extracted as typed references from pulldown-cmark events
  5. HTML block content is scanned for references (pragmatic: anneal's job is finding all references)
**Plans**: 3 plans

Plans:
- [x] 05-01-PLAN.md -- SourceSpan & LineIndex types + pulldown-cmark dependency
- [x] 05-02-PLAN.md -- pulldown-cmark event walker (scan_file_cmark)
- [x] 05-03-PLAN.md -- Production wiring + parallel-run comparison

### Phase 6: Resolution Cascade
**Goal**: Unresolved references get deterministic "did you mean?" candidates, and all diagnostics carry structured evidence with mandatory source locations
**Depends on**: Phase 5 (extraction must produce DiscoveredRef with RefHint and line numbers)
**Requirements**: RESOLVE-02, RESOLVE-03, RESOLVE-04, RESOLVE-05, RESOLVE-06, DIAG-01, DIAG-02, DIAG-03, DIAG-04, DIAG-05, UX-01
**Success Criteria** (what must be TRUE):
  1. `anneal check` on a corpus with path mismatches shows "similar handle exists: subdir/foo.md" instead of bare E001
  2. Resolution cascade resolves root-prefix paths (`.design/foo.md` -> `foo.md`), version stems (`formal-model-v11.md` -> suggest v17), and zero-padded labels (`OQ-01` -> `OQ-1`)
  3. Every diagnostic in `--json` output carries a `SourceSpan` (file + line), never null
  4. JSON output changes are additive-only — existing fields preserve type and presence, new fields are nullable
  5. `--active-only` is configurable via `[check] default_filter = "active-only"` in anneal.toml (no default behavior change)
**Plans**: 3 plans

Plans:
- [x] 06-01-PLAN.md -- PendingEdge line threading & CheckConfig for active-only
- [x] 06-02-PLAN.md -- Resolution cascade strategies (root-prefix, version-stem, zero-pad)
- [x] 06-03-PLAN.md -- Evidence enum & diagnostic enrichment with candidates

### Phase 7: UX Enrichment
**Goal**: Orientation commands are richer and more actionable — content snippets, obligations tracking, file-scoped checks, smarter init, and false positive suppression
**Depends on**: Phase 6 (diagnostics and resolution must be enriched first)
**Requirements**: UX-02, UX-03, UX-04, UX-05, UX-06, CONFIG-01, CONFIG-02, QUALITY-02, QUALITY-03
**Success Criteria** (what must be TRUE):
  1. `anneal get OQ-64` shows a content snippet (first paragraph for files, heading context for labels) in addition to metadata
  2. `anneal obligations` shows linear namespace status: outstanding, discharged, and mooted counts
  3. `anneal check --file=path.md` scopes diagnostics to a single file
  4. `anneal --root .design/ check` on anneal's own spec directory passes cleanly (self-check)
  5. S003 pipeline stall suggestion uses temporal signal from snapshot history rather than static edge counting
**Plans**: TBD

## Progress

**Execution Order:** Phases 4 -> 5 -> 6 -> 7

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Graph Foundation | v1.0 | 3/3 | Complete | 2026-03-29 |
| 2. Checks & CLI | v1.0 | 3/3 | Complete | 2026-03-29 |
| 3. Convergence & Polish | v1.0 | 5/5 | Complete | 2026-03-29 |
| 4. Types & Plausibility | v1.1 | 3/3 | Complete | 2026-03-30 |
| 5. pulldown-cmark Migration | v1.1 | 3/3 | Complete | 2026-03-30 |
| 6. Resolution Cascade | v1.1 | 2/3 | In progress | - |
| 7. UX Enrichment | v1.1 | 0/0 | Not started | - |

---
*Roadmap created: 2026-03-28*
*v1.1 phases added: 2026-03-29*
