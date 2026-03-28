# Requirements: anneal

**Defined:** 2026-03-28
**Core Value:** An arriving agent can immediately understand what's settled, what's drifting, what's connected, and where to push next.

## v1 Requirements

### Graph Construction

- [ ] **GRAPH-01**: Scan directory tree for .md files and create File handles
- [ ] **GRAPH-02**: Parse YAML frontmatter between `---` fences, extract `status:` and metadata
- [ ] **GRAPH-03**: Parse markdown headings (`#{1,6}`) to create Section handles within files
- [ ] **GRAPH-04**: Scan content with RegexSet for labels, section refs, file paths, version refs
- [ ] **GRAPH-05**: Build directed graph with typed edges (Cites, DependsOn, Supersedes, Verifies, Discharges)
- [ ] **GRAPH-06**: Graph is computed from files on every invocation, never stored

### Handle Resolution

- [ ] **HANDLE-01**: Resolve File handles by filesystem path
- [ ] **HANDLE-02**: Resolve Section handles to heading ranges within parent files
- [ ] **HANDLE-03**: Resolve Label handles by scanning confirmed namespaces across all files
- [ ] **HANDLE-04**: Resolve Version handles by matching versioned artifact naming conventions
- [ ] **HANDLE-05**: Infer handle namespaces by sequential cardinality (N >= 3 members, M >= 2 files)
- [ ] **HANDLE-06**: Only labels in confirmed namespaces generate broken-reference errors

### Convergence Lattice

- [ ] **LATTICE-01**: Support two-element existence lattice {exists, missing} as zero-config baseline
- [ ] **LATTICE-02**: Infer confidence lattice from observed frontmatter status values
- [ ] **LATTICE-03**: Partition status values into active and terminal sets (by directory convention + config)
- [ ] **LATTICE-04**: Compute freshness from file mtime or `updated:` frontmatter field

### Local Checks

- [ ] **CHECK-01**: Existence check — every edge target must resolve (error if not)
- [ ] **CHECK-02**: Staleness check — active handle referencing terminal handle (warning)
- [ ] **CHECK-03**: Confidence gap check — DependsOn edge where source state > target state (warning)
- [ ] **CHECK-04**: Linearity check — linear handles discharged exactly once (error if zero, info if multiple)
- [ ] **CHECK-05**: Convention adoption check — warn about missing frontmatter only when >50% of siblings have it
- [ ] **CHECK-06**: Diagnostics use compiler-style format with error codes (E001, W001, I001)

### Impact Analysis

- [ ] **IMPACT-01**: Compute impact set by reverse traversal over DependsOn, Supersedes, Verifies edges
- [ ] **IMPACT-02**: Handle cycles via visited-set detection
- [ ] **IMPACT-03**: Show direct and indirect affected handles

### Convergence Tracking

- [ ] **CONVERGE-01**: Append snapshot to `.anneal/history.jsonl` after check/status runs
- [ ] **CONVERGE-02**: Snapshot includes handle counts, edge counts, state histogram, obligation status, diagnostic counts, namespace stats
- [ ] **CONVERGE-03**: Compute convergence summary: advancing, holding, or drifting (from snapshot delta)
- [ ] **CONVERGE-04**: Compute graph diff between current state and previous snapshot
- [ ] **CONVERGE-05**: Graceful handling of missing/corrupted history file (skip bad lines, return empty on missing)

### CLI Commands

- [ ] **CLI-01**: `anneal check` — run local checks, report diagnostics, exit non-zero on errors
- [ ] **CLI-02**: `anneal get <handle>` — resolve any handle, show content + state + context
- [ ] **CLI-03**: `anneal find <query>` — full-text search filtered by convergence state
- [ ] **CLI-04**: `anneal status` — dashboard with graph stats, pipeline state, convergence summary
- [ ] **CLI-05**: `anneal map` — render knowledge graph, with --concern and --around flags
- [ ] **CLI-06**: `anneal init` — save inferred coloring as anneal.toml
- [ ] **CLI-07**: `anneal impact <handle>` — show what's affected if handle changes
- [ ] **CLI-08**: `anneal diff [ref]` — graph-level changes since reference point
- [ ] **CLI-09**: All commands support `--json` output via global flag
- [ ] **CLI-10**: Human-readable output as default, via CommandOutput trait with print_human()

### Configuration

- [ ] **CONFIG-01**: Parse anneal.toml with all-optional fields via `#[serde(default, deny_unknown_fields)]`
- [ ] **CONFIG-02**: Zero-config is valid — tool works with no anneal.toml (existence lattice only)
- [ ] **CONFIG-03**: Config supports: root, convergence (active/terminal/ordering), handles (confirmed/rejected), linear namespaces, freshness thresholds, concern groups
- [ ] **CONFIG-04**: `anneal init` generates anneal.toml from inferred structure

### Suggestions

- [ ] **SUGGEST-01**: Detect orphaned handles (no incoming edges)
- [ ] **SUGGEST-02**: Detect candidate handle namespaces (recurring regex patterns)
- [ ] **SUGGEST-03**: Detect pipeline stalls (state levels with high population, no outflow)
- [ ] **SUGGEST-04**: Detect abandoned namespaces (all members frozen >N days)
- [ ] **SUGGEST-05**: Suggest concern groups from label co-occurrence

## v2 Requirements

### Search

- **SEARCH-01**: Vector/semantic search backend for `anneal find`
- **SEARCH-02**: Ranked results by relevance + convergence state

### Integration

- **INTEG-01**: MCP server wrapping all commands as tools
- **INTEG-02**: Pre-commit hook integration via just target

### Extended Scanning

- **SCAN-01**: Comment scanning in non-markdown files (Agda, Lean, Rust)
- **SCAN-02**: Edge kind inference from context keywords (incorporates, see also, cf.)

### Extended Analysis

- **ANAL-01**: Full Kleene propagation as opt-in mode
- **ANAL-02**: Coherence proxy metrics (decision stability, session orientation speed)

## Out of Scope

| Feature | Reason |
|---------|--------|
| Markdown AST parsing | Five regexes + YAML parser is sufficient; AST adds complexity without value |
| Content heuristics for suggestions | Fragile, language-dependent; structural analysis is reliable |
| ML models for semantic search | Adds 2GB+ dependencies; full-text is v1, vector search is v2 |
| Database storage for graph | Graph is computed from files; statelessness is a design principle (KB-P1) |
| Document creation/editing | anneal reads and reports; it doesn't write content documents |
| Process enforcement | anneal reports state; it doesn't gate transitions (KB-P4) |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| GRAPH-01 | Phase 1 | Pending |
| GRAPH-02 | Phase 1 | Pending |
| GRAPH-03 | Phase 1 | Pending |
| GRAPH-04 | Phase 1 | Pending |
| GRAPH-05 | Phase 1 | Pending |
| GRAPH-06 | Phase 1 | Pending |
| HANDLE-01 | Phase 1 | Pending |
| HANDLE-02 | Phase 1 | Pending |
| HANDLE-03 | Phase 1 | Pending |
| HANDLE-04 | Phase 1 | Pending |
| HANDLE-05 | Phase 1 | Pending |
| HANDLE-06 | Phase 1 | Pending |
| LATTICE-01 | Phase 1 | Pending |
| LATTICE-02 | Phase 1 | Pending |
| LATTICE-03 | Phase 1 | Pending |
| LATTICE-04 | Phase 1 | Pending |
| CHECK-01 | Phase 2 | Pending |
| CHECK-02 | Phase 2 | Pending |
| CHECK-03 | Phase 2 | Pending |
| CHECK-04 | Phase 2 | Pending |
| CHECK-05 | Phase 2 | Pending |
| CHECK-06 | Phase 2 | Pending |
| IMPACT-01 | Phase 2 | Pending |
| IMPACT-02 | Phase 2 | Pending |
| IMPACT-03 | Phase 2 | Pending |
| CONVERGE-01 | Phase 3 | Pending |
| CONVERGE-02 | Phase 3 | Pending |
| CONVERGE-03 | Phase 3 | Pending |
| CONVERGE-04 | Phase 3 | Pending |
| CONVERGE-05 | Phase 3 | Pending |
| CLI-01 | Phase 2 | Pending |
| CLI-02 | Phase 2 | Pending |
| CLI-03 | Phase 2 | Pending |
| CLI-04 | Phase 3 | Pending |
| CLI-05 | Phase 3 | Pending |
| CLI-06 | Phase 2 | Pending |
| CLI-07 | Phase 2 | Pending |
| CLI-08 | Phase 3 | Pending |
| CLI-09 | Phase 2 | Pending |
| CLI-10 | Phase 2 | Pending |
| CONFIG-01 | Phase 1 | Pending |
| CONFIG-02 | Phase 1 | Pending |
| CONFIG-03 | Phase 2 | Pending |
| CONFIG-04 | Phase 2 | Pending |
| SUGGEST-01 | Phase 3 | Pending |
| SUGGEST-02 | Phase 3 | Pending |
| SUGGEST-03 | Phase 3 | Pending |
| SUGGEST-04 | Phase 3 | Pending |
| SUGGEST-05 | Phase 3 | Pending |

**Coverage:**
- v1 requirements: 48 total
- Mapped to phases: 48
- Unmapped: 0

---
*Requirements defined: 2026-03-28*
*Last updated: 2026-03-28 after initialization*
