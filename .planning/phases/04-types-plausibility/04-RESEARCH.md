# Phase 4: Types & Plausibility - Research

**Researched:** 2026-03-29
**Domain:** Rust type system design for extraction pipeline refactor
**Confidence:** HIGH

## Summary

Phase 4 introduces typed intermediate representations between parsing and resolution. Currently, four separate types (`PendingEdge`, `LabelCandidate`, `FrontmatterEdge`, `ScanResult`) carry discovered references through the pipeline with no classification. Frontmatter values like URLs (`https://modal.com/pricing`), absolute paths (`.design/SPEC.md`), freeform prose (`claude-desktop session`), and compound lists (`specs/foo.md, specs/bar.md`) all become `PendingEdge` targets that fail resolution and produce false-positive E001 "broken reference" errors.

The core work is: (1) define `FileExtraction` as the uniform output of per-file extraction, (2) define `DiscoveredRef` with a `RefHint` enum that classifies every reference at extraction time (label, file path, section ref, external URL, implausible), (3) add plausibility filtering so implausible frontmatter values produce diagnostics instead of false E001 errors, and (4) introduce the `Resolution` enum type signature that Phase 6 will populate with cascade strategies.

**Primary recommendation:** Build the new types in a new `extraction.rs` module, wire them through `build_graph` alongside the existing types, and gate behavior changes behind the classification. Existing types remain until Phase 5 completes the migration -- Phase 4 adds the new layer, not removes the old one.

## Project Constraints (from CLAUDE.md)

- **Language**: Rust 1.94.0 stable, edition 2024
- **Quality gate**: `just check` (fmt + clippy + test) must pass before every commit; pre-commit hook enforces this
- **Clippy**: all + pedantic denied, with targeted allows
- **No `unwrap()` in production** -- use `expect("reason")` or propagate with `?`
- **`unsafe` denied** workspace-wide
- **`--json` on every command** -- `CommandOutput` trait: `Serialize` + `print_human()`
- **JSON schema changes additive-only** -- new nullable fields allowed, existing fields preserve type and presence
- **Dependencies**: 10 crates, ~10s clean build; no new crates needed for this phase
- **Hand-roll preference**: No heavy diagnostic crates (codespan, miette); no generic fuzzy matching (strsim)
- **Deterministic structural transforms only** for resolution, no fuzzy edit distance

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| EXTRACT-01 | Introduce `FileExtraction` type as uniform extraction output from both frontmatter and body scanning | New struct in extraction.rs combining status, metadata, and Vec<DiscoveredRef> |
| EXTRACT-02 | Introduce `DiscoveredRef` with `RefHint` enum replacing `PendingEdge`, `LabelCandidate`, `FrontmatterEdge`, `ScanResult` | RefHint variants: Label, FilePath, SectionRef, External, Implausible; source tracking |
| EXTRACT-05 | Plausibility filter rejects absolute paths, freeform prose, and wildcard patterns from frontmatter edge targets | Regex-based classification of frontmatter values before they become pending edges |
| EXTRACT-06 | URLs in frontmatter classified as `RefHint::External` (not silently skipped) | URL detection at frontmatter parse time; External variant in RefHint |
| RESOLVE-01 | Introduce `Resolution` enum (Exact / Fuzzy / Unresolved) with candidate collection | Type definition only in Phase 4; cascade logic deferred to Phase 6 |
</phase_requirements>

## Standard Stack

### Core (no new dependencies)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde + serde_derive | 1.x | Serialization for new types | Already in Cargo.toml; all output types must derive Serialize |
| regex | 1.x | Plausibility classification patterns | Already in Cargo.toml; URL/path/prose detection |
| camino | 1.x | UTF-8 path handling | Already in Cargo.toml; Utf8PathBuf in DiscoveredRef |

No new crates required. All work uses existing dependencies.

## Architecture Patterns

### Current Pipeline (what exists)

```
split_frontmatter() -> parse_frontmatter() -> FrontmatterEdge + HandleMetadata
                                                     |
scan_file()         -> ScanResult { label_candidates, section_refs, file_refs }
                                                     |
build_graph()       -> BuildResult { pending_edges: Vec<PendingEdge>, ... }
                                                     |
resolve_all()       -> ResolveStats
                                                     |
collect_unresolved_owned() -> Vec<PendingEdge>  (unresolved only)
                                                     |
check_existence()   -> E001 diagnostics
```

**Problem points in current pipeline:**
1. `parse_frontmatter()` calls `yaml_value_to_string()` on ALL frontmatter values -- URLs, prose, paths all become raw strings
2. These strings become `PendingEdge.target_identity` in `build_graph()` at line 596
3. `resolve_pending_edges()` tries to resolve them as handle IDs or filenames
4. Anything unresolved becomes E001 in `check_existence()`
5. No classification step exists between extraction and resolution

### Recommended New Module: `extraction.rs`

```
src/
  extraction.rs     # NEW: FileExtraction, DiscoveredRef, RefHint, plausibility filter
  parse.rs          # Existing: split_frontmatter, parse_frontmatter, scan_file (unchanged)
  resolve.rs        # Existing: add Resolution enum type definition
  checks.rs         # Existing: minor change to handle classified refs
```

### Pattern: Parallel Type Introduction

Phase 4 introduces new types alongside existing ones. The existing pipeline continues working unchanged. New types are populated in parallel during `build_graph()` and used for classification, but the existing `PendingEdge` flow remains the path that produces diagnostics. This avoids a big-bang refactor.

```rust
// extraction.rs -- new module

/// Uniform extraction output from a single file.
/// Replaces the tuple return from parse_frontmatter + scan_file.
pub(crate) struct FileExtraction {
    pub(crate) status: Option<String>,
    pub(crate) metadata: HandleMetadata,
    pub(crate) refs: Vec<DiscoveredRef>,
    pub(crate) all_keys: Vec<String>,
}

/// A single discovered reference with classification.
/// Eventually replaces PendingEdge, LabelCandidate, FrontmatterEdge.
pub(crate) struct DiscoveredRef {
    pub(crate) raw: String,
    pub(crate) hint: RefHint,
    pub(crate) source: RefSource,
    pub(crate) edge_kind: EdgeKind,
    pub(crate) inverse: bool,
}

/// Classification of what a reference looks like before resolution.
pub(crate) enum RefHint {
    /// Label reference (e.g., "OQ-64", "KB-D1")
    Label { prefix: String, number: u32 },
    /// File path reference (e.g., "foo.md", "subdir/bar.md")
    FilePath,
    /// Section cross-reference (e.g., "section:4.1")
    SectionRef,
    /// External URL (e.g., "https://example.com")
    External,
    /// Failed plausibility: absolute path, prose, wildcard, etc.
    Implausible { reason: String },
}

/// Where in the file the reference was discovered.
pub(crate) enum RefSource {
    /// From a frontmatter field
    Frontmatter { field: String },
    /// From body text scanning
    Body,
}
```

### Pattern: Resolution Enum (Type Only)

```rust
// In resolve.rs -- add type definition

/// Resolution outcome for a discovered reference.
/// Phase 4: type definition only.
/// Phase 6: populated by resolution cascade.
pub(crate) enum Resolution {
    /// Exact match to a known handle
    Exact(NodeId),
    /// Fuzzy match with candidates (Phase 6)
    Fuzzy { candidates: Vec<NodeId> },
    /// No match found
    Unresolved,
}
```

### Plausibility Filter Design

The plausibility filter runs on frontmatter edge target strings BEFORE they become `PendingEdge` entries. It classifies each string and either:
- Passes it through (label, file path, section ref) -- becomes a normal PendingEdge
- Classifies it as External (URL) -- tracked but not resolved as E001
- Rejects it as Implausible -- emits a diagnostic, does NOT create a PendingEdge

**Classification rules (ordered, first match wins):**

```rust
fn classify_frontmatter_value(value: &str) -> RefHint {
    let trimmed = value.trim();

    // 1. URL detection
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return RefHint::External;
    }

    // 2. Absolute path (starts with / or ~/ or drive letter)
    if trimmed.starts_with('/') || trimmed.starts_with("~/") {
        return RefHint::Implausible {
            reason: "absolute path".into(),
        };
    }

    // 3. Wildcard patterns
    if trimmed.contains('*') || trimmed.contains('?') {
        return RefHint::Implausible {
            reason: "wildcard pattern".into(),
        };
    }

    // 4. Freeform prose: contains spaces AND no .md extension AND not a label pattern
    if trimmed.contains(' ') && !trimmed.ends_with(".md") && !LABEL_RE.is_match(trimmed) {
        return RefHint::Implausible {
            reason: "freeform prose".into(),
        };
    }

    // 5. Comma-separated lists (multiple targets in one string)
    if trimmed.contains(", ") && trimmed.len() > 40 {
        return RefHint::Implausible {
            reason: "comma-separated list".into(),
        };
    }

    // 6. Label pattern
    if let Some(caps) = LABEL_RE.captures(trimmed) {
        // ... extract prefix + number
        return RefHint::Label { prefix, number };
    }

    // 7. File path (.md extension)
    if trimmed.ends_with(".md") {
        return RefHint::FilePath;
    }

    // 8. Default: treat as potential handle identity (label without .md)
    RefHint::FilePath
}
```

### Anti-Patterns to Avoid

- **Big-bang type replacement**: Do NOT delete PendingEdge/ScanResult/etc in Phase 4. The new types run alongside; removal happens in Phase 5 after pulldown-cmark migration validates the new pipeline.
- **Behavior change in diagnostics**: Phase 4 MUST NOT change final diagnostic output. The plausibility filter prevents new false PendingEdges from being created, but existing behavior for valid references must be identical.
- **Overly aggressive filtering**: Plausibility should be conservative -- only reject things that are clearly NOT handle references. "claude-desktop" without spaces or extensions is borderline; `value.contains(' ') && !value.ends_with(".md")` catches "claude-desktop session" but not "claude-desktop".

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| URL detection | Full URL parser | Simple prefix check `starts_with("http")` | Frontmatter URLs are always absolute; no need for url crate |
| Fuzzy matching | Edit distance | Deferred to Phase 6; structural transforms only | Per REQUIREMENTS.md Out of Scope |
| Heavy diagnostics | codespan/miette integration | Existing Diagnostic struct with optional fields | Per REQUIREMENTS.md Out of Scope |

## Common Pitfalls

### Pitfall 1: Breaking JSON Schema

**What goes wrong:** Adding `DiscoveredRef` to check output changes existing JSON field types or removes fields.
**Why it happens:** Refactoring types tempts restructuring output.
**How to avoid:** JSON changes must be additive-only (DIAG-04). New nullable fields are fine. Do NOT change the shape of existing `diagnostics` array entries. The `DiscoveredRef` data should appear in a NEW field if exposed, not replace existing fields.
**Warning signs:** Existing integration tests fail; `--json` output diff shows removed/renamed fields.

### Pitfall 2: Plausibility Filter Too Aggressive

**What goes wrong:** Valid frontmatter references get classified as Implausible and stop producing E001 errors (hiding real broken references).
**Why it happens:** Overfitting to Herald/Murail false positives.
**How to avoid:** Only reject patterns with high confidence: URLs (starts with http), absolute paths (starts with / or ~/), wildcard (contains * or ?), clear prose (contains spaces, no .md, not a label). When in doubt, let it through as a normal reference.
**Warning signs:** `anneal check` on Murail shows FEWER total diagnostics than before (should show same number of real errors, just different classification for false positives).

### Pitfall 3: Circular Dependency Between extraction.rs and parse.rs

**What goes wrong:** New extraction.rs imports from parse.rs, and parse.rs needs to use the new types.
**Why it happens:** Extraction types need to reference EdgeKind from graph.rs and HandleMetadata from handle.rs.
**How to avoid:** extraction.rs depends on graph.rs and handle.rs (which are stable). parse.rs does NOT import from extraction.rs in Phase 4 -- the conversion from old types to new types happens in build_graph or a new adapter function.
**Warning signs:** Compile errors from circular module imports.

### Pitfall 4: Forgetting Inverse Edge Direction

**What goes wrong:** DiscoveredRef loses the `inverse` flag from FrontmatterEdge, causing edges to point the wrong way.
**Why it happens:** The inverse direction concept exists only in frontmatter config (e.g., "affects: X" means X DependsOn this file).
**How to avoid:** DiscoveredRef must carry the `inverse: bool` field just like PendingEdge does.
**Warning signs:** Graph traversal tests show reversed edges; impact analysis gives wrong results.

### Pitfall 5: Clippy Pedantic on New Enums

**What goes wrong:** Clippy pedantic flags on new enum variants (e.g., `enum_variant_names`, `module_name_repetitions`).
**Why it happens:** Workspace has `clippy::all` and `clippy::pedantic` denied.
**How to avoid:** Name variants without repeating the enum name (e.g., `RefHint::Label` not `RefHint::RefLabel`). If a lint fires, add a targeted `#[allow]` with a comment explaining why.
**Warning signs:** `just lint` fails on new code.

## Code Examples

### Example 1: Real False Positives from Herald Corpus

Current E001 errors that should become Implausible or External:

```
# URLs -- should be RefHint::External
E001: broken reference: https://modal.com/products/sandboxes not found
E001: broken reference: https://modal.com/docs/guide/sandboxes not found

# Freeform prose -- should be RefHint::Implausible
E001: broken reference: 20+ academic papers not found
E001: broken reference: GitHub repos, community forums, HN threads, industry reports not found
E001: broken reference: 44 services analyzed across 9 tiers not found
E001: broken reference: claude-desktop session not found

# Compound list -- should be RefHint::Implausible
E001: broken reference: specs/2026-03-21-herald-next-era.md, specs/2026-03-21-architecture-theory.md not found

# Non-markdown paths -- informational, not broken references
E001: broken reference: lib/herald/agent/ not found
E001: broken reference: lib/herald/agent/session.ex not found
```

### Example 2: Real False Positives from Murail Corpus

```
# Absolute root-prefixed paths -- should be RefHint::Implausible (or Phase 6 resolution)
E001: broken reference: .design/formal-model/murail-formal-model-compact-v8.md not found
E001: broken reference: .design/SPEC.md not found
E001: broken reference: .design/language/three-layer-architecture.md not found

# Prose values -- should be RefHint::Implausible
E001: broken reference: claude-desktop not found
E001: broken reference: spec/SPEC.md v0.8 not found  (contains space + version suffix)
```

### Example 3: Where Plausibility Filter Hooks In

```rust
// In build_graph() -- current code at line 592-599 of parse.rs:
for fe in &field_edges {
    for target in &fe.targets {
        pending_edges.push(PendingEdge { ... });
    }
}

// After Phase 4: classify before creating PendingEdge
for fe in &field_edges {
    for target in &fe.targets {
        let hint = classify_frontmatter_value(target);
        match hint {
            RefHint::External => {
                // Track but don't create PendingEdge
                // (Phase 7 CONFIG-02 will create HandleKind::External)
            }
            RefHint::Implausible { reason } => {
                // Emit plausibility diagnostic, don't create PendingEdge
            }
            _ => {
                // Normal flow: create PendingEdge as before
                pending_edges.push(PendingEdge { ... });
            }
        }
    }
}
```

### Example 4: Existing Test Factories (for reference)

```rust
// From handle.rs -- test factories already exist
Handle::test_file("test.md", Some("draft"))
Handle::test_label("OQ", 64, None)

// New tests should follow this pattern for DiscoveredRef
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Raw string target identities | Classified RefHint before resolution | Phase 4 (this phase) | Eliminates false positive E001 from non-handle frontmatter values |
| Silent URL dropping | Explicit External classification | Phase 4 (this phase) | URLs tracked in extraction output, not silently lost |
| 4 intermediate types | 1 uniform DiscoveredRef | Phase 4 type intro, Phase 5 migration | Cleaner extraction boundary for pulldown-cmark swap |

## Open Questions

1. **Non-markdown file paths in frontmatter (e.g., `lib/herald/agent/session.ex`)**
   - What we know: These are valid references to code files outside the corpus
   - What's unclear: Should they be External, Implausible, or a new variant?
   - Recommendation: Classify as Implausible with reason "non-corpus path" for now. Phase 7 CONFIG-02 introduces HandleKind::External which is the proper home.

2. **Root-prefixed paths (`.design/foo.md`) -- plausibility vs resolution**
   - What we know: These ARE valid .md references, just with a root prefix that doesn't match the relative path in the graph
   - What's unclear: Should Phase 4 filter them or leave them for Phase 6 RESOLVE-03?
   - Recommendation: Leave them as `RefHint::FilePath` in Phase 4. They are plausible -- they just fail resolution. Phase 6 RESOLVE-03 adds root-prefix stripping to fix them. This avoids hiding real broken references.

3. **"spec/SPEC.md v0.8" -- path with trailing prose**
   - What we know: The existing `strip_trailing_parenthetical()` handles `"foo.md (note)"` but not `"foo.md v0.8"`
   - What's unclear: Should plausibility filter handle this or should stripping be enhanced?
   - Recommendation: Enhance the stripping function to also strip trailing version suffixes. This is a parse-time fix, not a plausibility concern.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, pure Rust type design
- Architecture: HIGH - codebase thoroughly examined, all touchpoints identified
- Pitfalls: HIGH - real false positives examined from Herald and Murail corpora (186 Murail E001s, 50+ Herald E001s analyzed)

**Research date:** 2026-03-29
**Valid until:** 2026-05-01 (stable domain, no external dependencies)
