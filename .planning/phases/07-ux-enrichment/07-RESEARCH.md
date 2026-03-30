# Phase 7: UX Enrichment - Research

**Researched:** 2026-03-30
**Domain:** CLI UX, configuration, content extraction, obligation tracking
**Confidence:** HIGH

## Summary

Phase 7 addresses nine requirements spanning five areas: content snippets in `anneal get` (UX-02), smarter terminal inference for `anneal init` (UX-03), default depth reduction for `anneal map --around` (UX-04), file-scoped `anneal check` (UX-05), a new `anneal obligations` command (UX-06), false-positive suppression config (CONFIG-01), external URL handle kind (CONFIG-02), self-check passing on anneal's own `.design/` (QUALITY-02), and temporal signal for S003 pipeline stall diagnostics (QUALITY-03).

The codebase is well-structured for all of these changes. The graph, parse, checks, cli, config, and snapshot modules provide clean extension points. No new dependencies are needed -- all work is pure Rust code changes within existing modules. The main risk areas are: (1) content snippet extraction requiring file I/O during `anneal get` (currently the graph doesn't store file content), (2) the suppress config requiring a new TOML section and filter pass in checks, and (3) self-check (QUALITY-02) requiring either fixing the spec or suppressing its false positive.

**Primary recommendation:** Implement in 3-4 plans: (1) config extensions + simple CLI changes (UX-04, CONFIG-01, CONFIG-02), (2) content snippets + obligations command (UX-02, UX-06), (3) file-scoped check + smarter init + temporal S003 (UX-05, UX-03, QUALITY-03), (4) self-check closure (QUALITY-02).

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| UX-02 | Content snippet in `anneal get` output | Requires reading file content at get-time; graph stores `file_path` but not content. Two modes: first paragraph for File handles, heading context for Label handles (grep label in file, return surrounding heading + paragraph). |
| UX-03 | Smarter `anneal init` terminal inference from status name heuristics | Currently `init` only uses directory convention (`archive/`, `history/`, `prior/`). Add heuristic: status names matching `superseded`, `archived`, `retired`, `deprecated`, `obsolete`, `withdrawn`, `cancelled` should be classified as terminal. |
| UX-04 | Default `--depth=1` for `anneal map --around` | Trivial: change `default_value = "2"` to `default_value = "1"` in clap arg definition in `main.rs` line 304. |
| UX-05 | `--file=<path>` filter for `anneal check` | Add `--file` arg to Check command, filter diagnostics by file path before output. Simple post-filter on `all_diagnostics` in `main.rs`. |
| UX-06 | `anneal obligations` command showing linear namespace status | New subcommand. Obligation counts already computed in `snapshot::build_snapshot`. Need per-namespace breakdown: iterate linear namespaces, count outstanding/discharged/mooted per namespace. |
| CONFIG-01 | `[suppress]` section in anneal.toml for false positive suppression | New config section with pattern-based suppression rules. Applied as a filter pass after `run_checks()`. |
| CONFIG-02 | `HandleKind::External` for URL references | Add variant to `HandleKind`, create External nodes from `ExternalRef` data already collected in `parse::build_graph`. Skip convergence tracking for External handles. |
| QUALITY-02 | Self-check: `anneal --root .design/ check` passes cleanly | Currently fails with E001 for `synthesis/v17.md` (inline prose reference in spec). Fix: either suppress it via CONFIG-01, or fix the reference in the spec. Suppression is the correct approach since this is a prose example, not a real reference. |
| QUALITY-03 | S003 pipeline stall uses temporal signal from snapshot history | Current `suggest_pipeline_stalls` uses static edge counting. Change to compare current pipeline distribution against previous snapshot -- a stall is when a level's population hasn't decreased over time, not when it lacks outflow edges. |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- **Rust project** -- use `just` for all build operations
- **Quality gate:** `just check` (fmt + clippy + test) must pass before commit
- **No `unwrap()` in production** -- use `expect("reason")` or `?`
- **`--json` on every command** -- new `obligations` command must support `--json`
- **Edition 2024** with Rust 1.94.0, `unsafe` denied
- **Clippy**: all + pedantic denied with targeted allows
- **Hand-roll simple things** per spec philosophy (don't add dependencies for simple tasks)
- **JSON schema additive-only** (DIAG-04) -- new fields allowed, existing fields must not change type
- **Pre-commit hook** runs `just check`

## Architecture Patterns

### Current Module Structure (Relevant)

```
src/
  main.rs         # CLI arg parsing, command dispatch, graph construction pipeline
  cli.rs          # Command implementations (cmd_get, cmd_check, cmd_init, etc.)
  checks.rs       # 5 check rules + 5 suggestion rules, Diagnostic type
  config.rs       # AnnealConfig with TOML deserialization
  snapshot.rs     # Snapshot types, obligation counting, convergence summary
  parse.rs        # File scanning, frontmatter parsing, build_graph
  extraction.rs   # RefHint, DiscoveredRef, FileExtraction types
  handle.rs       # Handle, HandleKind enum, HandleMetadata
  graph.rs        # DiGraph with dual adjacency lists
  lattice.rs      # Lattice, convergence state classification
  resolve.rs      # Resolution cascade
  impact.rs       # Reverse BFS
```

### Extension Points per Requirement

**UX-02 (content snippets):** The `cmd_get` function in `cli.rs` (line 282) receives `graph` and `node_index`. To get content, it needs access to the root path and must read the file. Two approaches:
1. Pass `root` to `cmd_get` and read file on demand (preferred -- keeps graph lean)
2. Store content in Handle (wastes memory for rarely-used feature)

For File handles: read file, extract first non-frontmatter paragraph.
For Label handles: read the file where the label is defined (`handle.file_path`), grep for the label string, return the heading + following paragraph.

**UX-05 (file-scoped check):** Add `--file` to Check variant in main.rs. In the check command handler, filter `all_diagnostics` to retain only those whose `file` field matches the provided path. Also need to match path normalization (strip root prefix if provided).

**CONFIG-01 (suppress):** Add `SuppressConfig` to `config.rs`:
```rust
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct SuppressConfig {
    /// Diagnostic codes to suppress globally (e.g., ["I001"]).
    pub(crate) codes: Vec<String>,
    /// Specific identity + code pairs to suppress.
    pub(crate) rules: Vec<SuppressRule>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct SuppressRule {
    pub(crate) code: String,
    pub(crate) target: String,  // e.g., "synthesis/v17.md"
}
```
TOML syntax:
```toml
[suppress]
codes = ["I001"]

[[suppress.rules]]
code = "E001"
target = "synthesis/v17.md"
```

**CONFIG-02 (HandleKind::External):** Add variant to HandleKind:
```rust
pub(crate) enum HandleKind {
    File(Utf8PathBuf),
    Section { parent: NodeId, heading: String },
    Label { prefix: String, number: u32 },
    Version { artifact: NodeId, version: u32 },
    External { url: String },
}
```
External handles participate in the graph (for navigation) but skip convergence tracking (never terminal, never counted in pipeline). The `ExternalRef` data is already collected in `parse::build_graph` (line 826) but not wired into graph nodes.

**UX-06 (obligations command):** New subcommand. The obligation counting logic already exists in `snapshot::build_snapshot` (lines 143-161). Extract into a shared function and add per-namespace detail:
```rust
pub(crate) struct ObligationDetail {
    pub(crate) namespace: String,
    pub(crate) outstanding: Vec<String>,  // handle IDs
    pub(crate) discharged: Vec<String>,
    pub(crate) mooted: Vec<String>,
}
```

**QUALITY-03 (temporal S003):** The `suggest_pipeline_stalls` function (checks.rs line 588) currently checks for DependsOn edges to the next level. Change to: read snapshot history, compare population at each level between current and previous snapshot. A stall is when a level's count hasn't decreased (or has grown) between snapshots while it has >= 3 members. This requires `suggest_pipeline_stalls` to accept a `&[Snapshot]` history parameter, or the previous snapshot's `states` map.

### Anti-Patterns to Avoid

- **Don't store file content in the graph.** Handles are metadata-only. Read files on demand for `anneal get` content snippets. The graph is rebuilt every invocation -- storing content would waste memory.
- **Don't make suppress rules too complex.** Simple code + target matching is sufficient. Regex patterns, glob matching, or other complexity is overkill for v1.1.
- **Don't break JSON schema.** All new fields must be additive. `GetOutput` gets a new `snippet: Option<String>` field (nullable). New `obligations` command gets its own output type.
- **Don't change default `--active-only` behavior** (per existing decision).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Content extraction for snippets | Custom markdown parser | Read file, split frontmatter, take first paragraph as plain text | pulldown-cmark available but overkill for "first paragraph" extraction |
| Status name heuristics | ML classifier | Static match list: `superseded`, `archived`, `retired`, `deprecated`, `obsolete`, `withdrawn`, `cancelled` | Deterministic, covers all common conventions |

## Common Pitfalls

### Pitfall 1: Content Snippet Performance
**What goes wrong:** Reading files during `anneal get` adds I/O to a command that was previously pure graph lookup.
**Why it happens:** The graph doesn't store content -- it's rebuilt from files every invocation but content is discarded after scanning.
**How to avoid:** Accept the I/O cost. `anneal get` targets a single handle, so reading one file is negligible. Don't pre-load all content.
**Warning signs:** If someone tries to add content to every handle in the graph.

### Pitfall 2: Path Normalization for --file
**What goes wrong:** User provides `./foo.md` or `subdir/foo.md` or absolute path, but diagnostics store paths relative to root.
**Why it happens:** Path comparison is string-based.
**How to avoid:** Normalize the `--file` path relative to root before comparison. Strip leading `./`, strip root prefix if absolute.
**Warning signs:** `--file=foo.md` works but `--file=./foo.md` doesn't.

### Pitfall 3: Config deny_unknown_fields
**What goes wrong:** Adding `[suppress]` to AnnealConfig fails because `deny_unknown_fields` rejects the new section.
**Why it happens:** `AnnealConfig` uses `#[serde(default, deny_unknown_fields)]` (config.rs line 89).
**How to avoid:** Add the `suppress` field to `AnnealConfig` struct BEFORE any config parsing changes. The field must exist in the struct for `deny_unknown_fields` to accept it.
**Warning signs:** Config parse errors when adding suppress section to anneal.toml.

### Pitfall 4: HandleKind::External Serialization
**What goes wrong:** Adding a new variant to `HandleKind` changes `as_str()` return value patterns, may break downstream JSON consumers.
**Why it happens:** New enum variant in a serialized type.
**How to avoid:** Add `External { url: String }` variant, add `"external"` to `as_str()`. Since JSON changes are additive-only and no existing variant changes, this is safe.
**Warning signs:** Existing tests failing after adding variant.

### Pitfall 5: Temporal S003 Without History
**What goes wrong:** No previous snapshot available (first run) -- temporal signal can't be computed.
**Why it happens:** New corpora have no `.anneal/history.jsonl`.
**How to avoid:** Fall back to the current static analysis when no history exists. Only use temporal signal when >= 2 snapshots are available.
**Warning signs:** S003 disappearing on first runs.

### Pitfall 6: Self-Check Circular Dependency
**What goes wrong:** Adding suppress rules to make self-check pass requires an anneal.toml in `.design/`, but `.design/` currently has no config.
**Why it happens:** The self-check target (`.design/anneal-spec.md`) has a prose reference to `synthesis/v17.md` that resolves as a broken reference.
**How to avoid:** Two options: (1) create `.design/anneal.toml` with suppress rule for `synthesis/v17.md`, or (2) change the spec prose to not look like a file reference. Option (1) is cleaner -- it also demonstrates the suppress feature.
**Warning signs:** None -- straightforward.

## Code Examples

### Content Snippet Extraction (UX-02)

```rust
// For File handles: read file, extract first paragraph after frontmatter
fn extract_snippet(root: &Utf8Path, file_path: &Utf8Path) -> Option<String> {
    let full_path = root.join(file_path);
    let content = std::fs::read_to_string(full_path.as_std_path()).ok()?;
    let (_, body) = crate::parse::split_frontmatter(&content);

    // First non-empty paragraph (lines until blank line)
    let mut lines = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() && !lines.is_empty() {
            break;
        }
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            lines.push(trimmed);
        }
    }
    if lines.is_empty() { None } else { Some(lines.join(" ")) }
}

// For Label handles: find label definition context in its file
fn extract_label_context(root: &Utf8Path, file_path: &Utf8Path, label_id: &str) -> Option<String> {
    let full_path = root.join(file_path);
    let content = std::fs::read_to_string(full_path.as_std_path()).ok()?;

    // Find the line containing this label
    let mut heading = String::new();
    for line in content.lines() {
        if line.starts_with('#') {
            heading = line.trim_start_matches('#').trim().to_string();
        }
        if line.contains(label_id) {
            // Return heading + the line containing the label
            let context = if heading.is_empty() {
                line.trim().to_string()
            } else {
                format!("{heading}: {}", line.trim())
            };
            return Some(truncate(&context, 200));
        }
    }
    None
}
```

### Suppress Config Filter (CONFIG-01)

```rust
fn apply_suppressions(diagnostics: &mut Vec<Diagnostic>, config: &SuppressConfig) {
    diagnostics.retain(|d| {
        // Check global code suppression
        if config.codes.contains(&d.code.to_string()) {
            return false;
        }
        // Check specific rules
        for rule in &config.rules {
            if d.code == rule.code {
                if d.message.contains(&rule.target) {
                    return false;
                }
            }
        }
        true
    });
}
```

### Terminal Status Heuristics (UX-03)

```rust
const TERMINAL_STATUS_HEURISTICS: &[&str] = &[
    "superseded", "archived", "retired", "deprecated",
    "obsolete", "withdrawn", "cancelled", "canceled",
    "closed", "resolved", "done", "completed",
];

fn is_terminal_by_heuristic(status: &str) -> bool {
    let lower = status.to_lowercase();
    TERMINAL_STATUS_HEURISTICS.iter().any(|h| lower.contains(h))
}
```

## Implementation Order Analysis

Based on dependency analysis:

1. **CONFIG-01 (suppress) + CONFIG-02 (External) + UX-04 (depth default)** -- Config changes first since QUALITY-02 depends on suppress being available. UX-04 is trivial (one line change). These are independent and can be a single plan.

2. **UX-02 (content snippets) + UX-06 (obligations command)** -- Both are new CLI features with no cross-dependencies. Can be done in parallel within one plan.

3. **UX-05 (file-scoped check) + UX-03 (smarter init) + QUALITY-03 (temporal S003)** -- Three moderate changes touching different modules. UX-05 is a filter pass, UX-03 is heuristic addition, QUALITY-03 needs snapshot history access in suggestions.

4. **QUALITY-02 (self-check)** -- Depends on CONFIG-01 being implemented (need suppress rules to handle the spec's false positive). Create `.design/anneal.toml` with appropriate suppression, verify `anneal --root .design/ check` passes.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Static edge counting for S003 | Should use temporal snapshot comparison | Phase 7 | More accurate stall detection; avoids false positives when edges exist but handles aren't progressing |
| Bare `anneal get` output (metadata only) | Should include content snippet | Phase 7 | Agents can understand what a handle IS without opening the file |
| Directory-only terminal inference | Should add status name heuristics | Phase 7 | Better zero-config experience for corpora without terminal directories |

## Open Questions

1. **Content snippet length limit**
   - What we know: First paragraph extraction is straightforward. Need a character/line limit to keep output concise.
   - What's unclear: Ideal snippet length (100 chars? 200? 500?).
   - Recommendation: 200 characters with ellipsis truncation. Sufficient for orientation without overwhelming terminal output.

2. **Suppress rule matching granularity**
   - What we know: Need at minimum code + target identity matching.
   - What's unclear: Should we support file-scoped suppression (suppress E001 for all refs in a specific file)? Or glob patterns?
   - Recommendation: Start simple: global code suppression + specific (code, target) pairs. Extend later if needed.

3. **HandleKind::External graph participation**
   - What we know: External refs are collected but not wired into the graph. CONFIG-02 says they should "participate in graph, skip convergence tracking."
   - What's unclear: Should External handles appear in `anneal map`? In `anneal find`? In diagnostic counts?
   - Recommendation: Add to graph for navigation (appear in `anneal get` edges, `anneal map`). Exclude from convergence pipeline (no status, no staleness checks, no obligation tracking). Treat like a Label with no status.

## Sources

### Primary (HIGH confidence)
- Source code analysis of all 12 modules in `src/`
- `.design/anneal-spec.md` -- spec sections on obligations (section 8), checks (section 7), commands (section 12), configuration (section 13)
- `.planning/REQUIREMENTS.md` -- all 9 Phase 7 requirements with descriptions
- Live `anneal --root .design/ check` output showing current self-check failure

### Secondary (MEDIUM confidence)
- Existing test patterns in checks.rs, snapshot.rs, parse.rs for test structure conventions
- Existing config.rs `deny_unknown_fields` pattern for new config section design

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all Rust code changes
- Architecture: HIGH -- clear extension points identified in each module
- Pitfalls: HIGH -- verified through code analysis and live testing
- Implementation order: HIGH -- dependency chain is clear (CONFIG-01 before QUALITY-02)

**Research date:** 2026-03-30
**Valid until:** 2026-04-30 (stable codebase, no external dependencies changing)
