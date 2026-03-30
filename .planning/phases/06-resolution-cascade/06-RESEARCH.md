# Phase 6: Resolution Cascade - Research

**Researched:** 2026-03-29
**Domain:** Handle resolution strategies, diagnostic enrichment, JSON schema evolution
**Confidence:** HIGH

## Summary

Phase 6 transforms anneal's resolution pipeline from a binary "found or not found" system into a multi-strategy cascade that produces actionable "did you mean?" candidates for unresolved references. Simultaneously, it enriches every diagnostic with mandatory `SourceSpan` (file + line) and structured `Evidence`, and adds config-driven `--active-only` filtering.

The codebase is well-prepared for this phase. Phase 4 introduced the `Resolution` enum (Exact/Fuzzy/Unresolved) and `DiscoveredRef` with `RefHint` classification. Phase 5 added `SourceSpan` and `LineIndex` for line number tracking. The key work is: (1) implementing the resolution cascade strategies inside `resolve.rs`, (2) threading `SourceSpan` from `DiscoveredRef` through `PendingEdge` into `Diagnostic`, (3) adding an `Evidence` enum to `Diagnostic` for structured results, and (4) wiring `[check]` config section for `default_filter`.

**Primary recommendation:** Build the cascade as a sequence of deterministic structural transforms applied to unresolved PendingEdges after the existing resolution pass, populating Resolution::Fuzzy candidates. Then enrich Diagnostic with SourceSpan and Evidence in a separate step to keep the diff reviewable.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| RESOLVE-02 | Resolution cascade: exact -> root-prefix strip -> bare filename -> version stem -> zero-pad normalize | Cascade architecture section below; existing `resolve_pending_edges` is the integration point |
| RESOLVE-03 | Root-prefix resolution (`.design/foo.md` -> `foo.md`) | Strip known root prefixes from target, re-lookup in node_index |
| RESOLVE-04 | Version stem resolution (`formal-model-v11.md` -> suggest `formal-model-v17.md`) | Existing `VERSION_FILENAME_RE` in resolve.rs; group by base stem, suggest latest version |
| RESOLVE-05 | Zero-pad label normalization (`OQ-01` -> `OQ-1`) | Strip leading zeros from number portion and re-lookup; label IDs are `{prefix}-{number}` with no padding |
| RESOLVE-06 | Unresolved references carry candidate list | `Resolution::Fuzzy { candidates: Vec<NodeId> }` already defined in resolve.rs |
| DIAG-01 | All diagnostics carry mandatory SourceSpan | Add `span: Option<SourceSpan>` to PendingEdge, make Diagnostic.line always populated |
| DIAG-02 | Introduce Evidence enum on Diagnostic | New enum: Evidence::BrokenRef { candidates }, Evidence::StaleRef { ... }, etc. |
| DIAG-03 | E001 includes resolution candidates | check_existence reads candidates from enriched unresolved edges |
| DIAG-04 | JSON output additive-only | New fields (`span`, `evidence`, `candidates`) are nullable; no type changes to existing fields |
| DIAG-05 | Human output stays terse (line number is only addition) | Diagnostic.print_human already prints line when Some; just ensure it is populated |
| UX-01 | `--active-only` via config `[check] default_filter` | New `CheckConfig` struct in config.rs, read in main.rs check command path |
</phase_requirements>

## Architecture Patterns

### Current Resolution Pipeline

```
main.rs::run()
  -> parse::build_graph()           // produces BuildResult with pending_edges, extractions
  -> resolve::resolve_all()         // resolves labels, versions, pending edges
  -> collect_unresolved_owned()     // filters remaining unresolved PendingEdges
  -> checks::run_checks()           // produces Diagnostic vec from unresolved edges
  -> cli::cmd_check()               // filters + formats output
```

### Target Resolution Pipeline (Phase 6)

```
main.rs::run()
  -> parse::build_graph()           // unchanged
  -> resolve::resolve_all()         // unchanged (exact resolution)
  -> resolve::cascade_unresolved()  // NEW: structural transforms on remaining unresolved
  -> collect_unresolved_owned()     // updated: includes candidates from cascade
  -> checks::run_checks()           // updated: reads candidates, produces Evidence
  -> cli::cmd_check()               // updated: reads active_only from config
```

### Resolution Cascade Order (RESOLVE-02)

The cascade applies deterministic structural transforms in order, stopping at first match:

1. **Exact match** (existing): `node_index.get(&target_identity)` -- already done in `resolve_pending_edges`
2. **Root-prefix strip** (RESOLVE-03): if target starts with a known root prefix (e.g., `.design/`, `docs/`), strip it and re-lookup
3. **Bare filename fallback** (existing): already done for `.md` targets without `/` in `resolve_pending_edges`
4. **Version stem** (RESOLVE-04): parse `{base}-v{N}.md`, find all `{base}-v{M}.md` in node_index, suggest the latest
5. **Zero-pad normalize** (RESOLVE-05): if target matches label pattern with leading zeros, strip zeros and re-lookup

Each strategy returns either a resolved NodeId (cascade stops, edge created) or candidate NodeIds (cascade continues, candidates accumulated). After all strategies run, remaining unresolved edges carry their candidate list.

### Key Design Decision: Cascade vs. Post-Pass

Two options exist for integrating the cascade:

1. **Inline in `resolve_pending_edges`**: extend the existing `.or_else()` chain
2. **Separate post-pass**: new `cascade_unresolved()` function runs after `resolve_all()`

**Recommendation: Separate post-pass.** Reasons:
- `resolve_pending_edges` mutates the graph (adds edges); cascade candidates should NOT add edges (they are suggestions, not resolutions)
- Fuzzy/candidate results need to flow to diagnostics, not the graph
- Keeps existing resolution behavior untouched (safer)
- Exception: root-prefix strip DOES produce exact matches and SHOULD create edges. It can either go in `resolve_pending_edges` as another `.or_else()` or the post-pass can add edges for exact cascaded matches.

### PendingEdge Enhancement for SourceSpan

`PendingEdge` currently has no line number. The line number exists in `DiscoveredRef.span` (via `FileExtraction`), but `PendingEdge` is constructed separately. Two options:

1. **Add `line: Option<u32>` to PendingEdge**: Populate during `build_graph` from the frontmatter line or body scan line
2. **Cross-reference PendingEdge with DiscoveredRef at diagnostic time**: Match by source NodeId + target_identity

**Recommendation: Option 1.** Adding a field to PendingEdge is simple and avoids a lookup join. The line number is available at PendingEdge construction time in `build_graph`.

### Diagnostic Enhancement

Current `Diagnostic`:
```rust
pub(crate) struct Diagnostic {
    pub(crate) severity: Severity,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
}
```

Target `Diagnostic`:
```rust
pub(crate) struct Diagnostic {
    pub(crate) severity: Severity,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
    // NEW: structured evidence for --json consumers
    pub(crate) evidence: Option<Evidence>,
}
```

The `Evidence` enum:
```rust
#[derive(Clone, Debug, Serialize)]
pub(crate) enum Evidence {
    BrokenRef {
        target: String,
        candidates: Vec<String>,
    },
    StaleRef {
        source_status: String,
        target_status: String,
    },
    ConfidenceGap {
        source_status: String,
        source_level: usize,
        target_status: String,
        target_level: usize,
    },
    UnmetObligation {
        prefix: String,
        number: u32,
    },
    // etc. for each diagnostic code
}
```

**JSON additive-only (DIAG-04):** `evidence` is `Option<Evidence>`, serialized as `null` when absent. All existing fields (`severity`, `code`, `message`, `file`, `line`) remain unchanged in type and presence.

### Config Enhancement for UX-01

Current `AnnealConfig` has no `[check]` section. Add:

```rust
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct CheckConfig {
    pub(crate) default_filter: Option<String>,
}
```

Add to `AnnealConfig`:
```rust
pub(crate) check: CheckConfig,
```

In `main.rs`, when constructing `CheckFilters`:
```rust
let active_only = active_only || config.check.default_filter.as_deref() == Some("active-only");
```

This is opt-in only. Default behavior is unchanged (all diagnostics shown).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Fuzzy string matching | Edit distance / Levenshtein | Deterministic structural transforms only | Explicit out-of-scope decision in REQUIREMENTS.md |
| Rich diagnostic rendering | codespan-reporting / miette | Hand-rolled single-line format | Explicit out-of-scope; anneal diagnostics are single-location |

**Key insight:** The REQUIREMENTS.md explicitly excludes both fuzzy matching libraries (strsim/Levenshtein) and heavy diagnostic crates (codespan, miette). All transforms are structural: prefix stripping, version extraction, zero-pad normalization. This is the correct approach for handle IDs which have predictable structure.

## Common Pitfalls

### Pitfall 1: Cascade creating false positive matches
**What goes wrong:** Root-prefix stripping matches the wrong file (e.g., `.design/foo.md` strips to `foo.md` but a different `foo.md` exists in another directory)
**Why it happens:** Corpus may have duplicate filenames in different directories
**How to avoid:** Root-prefix strip should only match if the result is unambiguous (single match in node_index). If ambiguous, add all matches as candidates instead of resolving.
**Warning signs:** Test with Murail corpus which has subdirectories.

### Pitfall 2: Version stem suggesting non-existent versions
**What goes wrong:** `formal-model-v11.md` suggests `formal-model-v17.md` which does exist, but there might be edge cases where the version regex matches incorrectly
**Why it happens:** VERSION_FILENAME_RE matches `{base}-v{N}.md`; some filenames might have `v` in the base name
**How to avoid:** Reuse the existing `VERSION_FILENAME_RE` from resolve.rs. Group by base name only among file handles.
**Warning signs:** Test with filenames like `v2-design.md`.

### Pitfall 3: Zero-pad normalization with compound labels
**What goes wrong:** `KB-D01` should normalize to `KB-D1`, but the compound prefix regex is tricky
**Why it happens:** Labels can have compound prefixes (`KB-D`, `KB-F`) where the hyphen separates prefix parts, not prefix from number
**How to avoid:** Use the extraction.rs `LABEL_RE` which already handles compound prefixes. Parse, strip leading zeros from number, rebuild identity string.
**Warning signs:** Test with `KB-D01`, `KB-F05`, `OQ-001`.

### Pitfall 4: Breaking JSON schema
**What goes wrong:** Adding `evidence` field changes serialization and breaks downstream consumers
**Why it happens:** Serde derives serialize all fields by default
**How to avoid:** `evidence: Option<Evidence>` serializes as `"evidence": null` when None. This is additive (new field) and nullable. Verify with `serde_json` that existing fields are unchanged.
**Warning signs:** Run existing JSON output tests before and after.

### Pitfall 5: PendingEdge line number gap
**What goes wrong:** Line numbers are available in DiscoveredRef (Phase 5) but not threaded through to PendingEdge, so diagnostics still have `line: null`
**Why it happens:** PendingEdge was designed before SourceSpan existed; they're populated in parallel paths
**How to avoid:** Add `line: Option<u32>` to PendingEdge. Populate from frontmatter line (YAML line number) or body scanner offset. The body scanner already has LineIndex.
**Warning signs:** Frontmatter edges may need special handling -- the YAML parser doesn't give exact line numbers per key.

### Pitfall 6: `deny_unknown_fields` on AnnealConfig blocking new `[check]` section
**What goes wrong:** Adding `check:` to anneal.toml fails because AnnealConfig uses `deny_unknown_fields`
**Why it happens:** `deny_unknown_fields` rejects any unrecognized TOML key
**How to avoid:** Add the `check` field to `AnnealConfig` struct BEFORE users need it. Since the field has `#[serde(default)]`, existing configs without `[check]` continue to work. But the field MUST be added to the struct.
**Warning signs:** Integration test with existing anneal.toml files.

## Code Examples

### Resolution cascade function signature

```rust
// resolve.rs

/// Resolution candidate from the cascade, before becoming a Diagnostic.
pub(crate) struct CascadeResult {
    /// PendingEdge index (or the edge itself)
    pub(crate) target_identity: String,
    /// Source node for the reference
    pub(crate) source: NodeId,
    /// Candidates found by structural transforms
    pub(crate) candidates: Vec<String>,
    /// If exactly one candidate with high confidence, the resolved NodeId
    pub(crate) resolved: Option<NodeId>,
}

/// Run deterministic structural transforms on unresolved pending edges.
pub(crate) fn cascade_unresolved(
    unresolved: &[PendingEdge],
    node_index: &HashMap<String, NodeId>,
    graph: &DiGraph,
    root_prefixes: &[&str],
) -> Vec<CascadeResult> {
    // ...
}
```

### Root-prefix strip strategy

```rust
fn try_root_prefix_strip(
    target: &str,
    node_index: &HashMap<String, NodeId>,
    root_prefixes: &[&str],
) -> Option<(NodeId, String)> {
    for prefix in root_prefixes {
        if let Some(stripped) = target.strip_prefix(prefix) {
            let stripped = stripped.strip_prefix('/').unwrap_or(stripped);
            if let Some(&node_id) = node_index.get(stripped) {
                return Some((node_id, stripped.to_string()));
            }
        }
    }
    None
}
```

### Version stem strategy

```rust
fn try_version_stem(
    target: &str,
    node_index: &HashMap<String, NodeId>,
) -> Vec<String> {
    // Parse target as versioned filename
    if let Some(caps) = VERSION_FILENAME_RE.captures(target) {
        let base = &caps[1];
        // Find all node_index entries matching "{base}-v{N}.md"
        let mut matches: Vec<(u32, String)> = node_index.keys()
            .filter_map(|key| {
                let filename = key.rsplit('/').next().unwrap_or(key);
                VERSION_FILENAME_RE.captures(filename)
                    .filter(|c| &c[1] == base)
                    .and_then(|c| c[2].parse::<u32>().ok().map(|v| (v, key.clone())))
            })
            .collect();
        matches.sort_by(|a, b| b.0.cmp(&a.0)); // latest first
        matches.into_iter().map(|(_, key)| key).collect()
    } else {
        vec![]
    }
}
```

### Zero-pad normalization

```rust
fn try_zero_pad_normalize(
    target: &str,
    node_index: &HashMap<String, NodeId>,
) -> Option<(NodeId, String)> {
    // Try parsing as label with possible leading zeros
    if let Some(caps) = LABEL_RE.captures(target) {
        let prefix = &caps[1];
        let number_str = &caps[2];
        // If number has leading zeros, try without
        if number_str.starts_with('0') && number_str.len() > 1 {
            if let Ok(number) = number_str.parse::<u32>() {
                let normalized = format!("{prefix}-{number}");
                if let Some(&node_id) = node_index.get(&normalized) {
                    return Some((node_id, normalized));
                }
            }
        }
    }
    None
}
```

### Evidence on E001

```rust
fn check_existence(
    graph: &DiGraph,
    unresolved_edges: &[UnresolvedEdge], // enhanced type with candidates
    section_ref_count: usize,
) -> Vec<Diagnostic> {
    for edge in unresolved_edges {
        let candidate_msg = if !edge.candidates.is_empty() {
            format!(
                "; similar handle exists: {}",
                edge.candidates.join(", ")
            )
        } else {
            String::new()
        };
        diagnostics.push(Diagnostic {
            severity: Severity::Error,
            code: "E001",
            message: format!(
                "broken reference: {} not found{}",
                edge.target_identity, candidate_msg
            ),
            file,
            line: edge.line,
            evidence: Some(Evidence::BrokenRef {
                target: edge.target_identity.clone(),
                candidates: edge.candidates.clone(),
            }),
        });
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `PendingEdge` without line | `PendingEdge` needs line added | Phase 6 | Enables DIAG-01 |
| `Diagnostic` without evidence | Add `evidence: Option<Evidence>` | Phase 6 | Enables DIAG-02, DIAG-03 |
| `Resolution` enum defined but unused | Cascade populates it | Phase 6 | Connects Phase 4 type to runtime |
| Binary resolve: found or E001 | Cascade with candidates | Phase 6 | RESOLVE-02 through RESOLVE-06 |

## Open Questions

1. **Root prefixes: hardcoded or inferred?**
   - What we know: The config has `root:` which defines the corpus root. References like `.design/foo.md` are relative to the project root, not the corpus root.
   - What's unclear: Should root prefixes be `[".design", "docs"]` hardcoded, or inferred from the `root` config value?
   - Recommendation: Infer from `config.root`. If root is `.design/`, then `.design/` is a root prefix. Also include the literal root directory name. This covers the common case without config.

2. **Should root-prefix matches create graph edges?**
   - What we know: Version stem and zero-pad produce candidates (suggestions), not edges. Root-prefix strip produces an exact match to a real node.
   - What's unclear: Is a root-prefix match an "exact resolution via transform" (should create edge) or a "fuzzy suggestion" (should only report)?
   - Recommendation: Root-prefix strip creates edges (it's a deterministic path normalization, not a guess). Version stem and zero-pad produce candidates only.

3. **Frontmatter line numbers for PendingEdge**
   - What we know: Body refs have SourceSpan from Phase 5's LineIndex. Frontmatter refs don't have per-field line numbers because serde_yaml_ng doesn't expose them.
   - What's unclear: What line number to use for frontmatter-sourced PendingEdges?
   - Recommendation: Use `1` (the frontmatter starts at line 1) as a pragmatic fallback. The file path alone is already useful. Alternatively, scan the raw YAML string for the field name to get an approximate line. The simpler approach (line 1 or even a dedicated "search for field name in raw yaml" scan of ~5 lines) is sufficient for DIAG-01 since it says "never null", not "always exact."

## Sources

### Primary (HIGH confidence)
- Source code: `src/resolve.rs` - current resolution pipeline, `Resolution` enum, `VERSION_FILENAME_RE`
- Source code: `src/extraction.rs` - `SourceSpan`, `LineIndex`, `DiscoveredRef`, `RefHint` types
- Source code: `src/checks.rs` - current `Diagnostic` struct, `check_existence`, `run_checks`
- Source code: `src/config.rs` - `AnnealConfig` with `deny_unknown_fields`, current config structure
- Source code: `src/parse.rs` - `PendingEdge`, `BuildResult`, `build_graph` pipeline
- Source code: `src/main.rs` - `collect_unresolved_owned`, check command pipeline
- `.design/anneal-spec.md` - diagnostic format (section 12.1), handle resolution (section 4.2)
- `.planning/REQUIREMENTS.md` - RESOLVE-02..06, DIAG-01..05, UX-01 definitions, out-of-scope items

### Secondary (MEDIUM confidence)
- `.planning/STATE.md` - decisions log confirming structural transforms only, additive JSON, active-only as config opt-in

## Metadata

**Confidence breakdown:**
- Resolution cascade architecture: HIGH - all types and integration points exist in code, clear requirements
- Diagnostic enrichment: HIGH - Diagnostic struct is simple, Evidence is additive
- SourceSpan threading: HIGH - SourceSpan exists, PendingEdge just needs the field added
- Config extension: HIGH - straightforward serde struct addition
- Frontmatter line numbers: MEDIUM - pragmatic approach needed since YAML parser doesn't expose per-field lines

**Research date:** 2026-03-29
**Valid until:** 2026-04-28 (stable; Rust codebase with pinned deps)
