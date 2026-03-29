# Architecture Patterns: Typed Extraction Pipeline with pulldown-cmark

**Domain:** Refactoring internal pipeline for anneal v1.1
**Researched:** 2026-03-29
**Overall confidence:** HIGH (pulldown-cmark API verified via docs.rs, patterns verified via mdbook-linkcheck and lychee source)

## Executive Summary

The v1.1 milestone replaces anneal's five-regex body scanner with pulldown-cmark and introduces three typed intermediate representations (extraction, resolution, diagnostics). This document answers five specific architecture questions and provides an incremental migration strategy that keeps `just check` green at every step.

The core insight: **extraction and graph construction must be separate phases with a clean data boundary**. The current `build_graph()` conflates file walking, frontmatter parsing, body scanning, and graph mutation into one 155-line function. The new architecture introduces `extract_file()` as a pure function (file content in, `FileExtraction` out, no graph mutation), then a separate `build_graph_from_extractions()` phase that consumes extractions into graph nodes and edges. This is the same pattern used by both mdbook-linkcheck and lychee.

pulldown-cmark 0.13.3 is the right choice. It adds ~3 direct dependencies (bitflags, memchr, unicase), has native wikilink support via `ENABLE_WIKILINKS`, yields `(Event, Range<usize>)` pairs via `into_offset_iter()` for source spans, and handles code block boundaries structurally (eliminating the manual `in_code_block` toggle). The regex crate stays for label pattern matching within text spans -- pulldown-cmark handles document structure, regex handles domain-specific pattern extraction within text nodes.

## Question 1: Should extract_file() Replace build_graph() or Compose With It?

**Recommendation: Compose, not replace.** Introduce `extract_file()` as a new pure function that returns `FileExtraction`, then refactor `build_graph()` to call it internally.

### Architecture

```
build_graph(root, config)           // EXISTING entry point, signature unchanged
  |
  +-- walk_directory(root, config)  // Factor out of build_graph (unchanged logic)
  |
  +-- for each file:
  |     extract_file(content, path, config) -> FileExtraction  // NEW pure function
  |
  +-- build_graph_from_extractions(extractions) -> BuildResult  // NEW graph assembly
```

### Rationale

1. **Backward compatibility.** `build_graph()` is called from `main.rs` and the integration test. Changing its signature is a breaking change across the codebase. Keeping the same entry point while decomposing internals means zero changes to callers initially.

2. **Testability.** `extract_file()` as a pure function (`&str, &Utf8Path, &FrontmatterConfig -> FileExtraction`) can be unit-tested without constructing a `DiGraph`. The current `scan_file()` takes `&mut DiGraph` and a `NodeId`, making it impossible to test extraction in isolation.

3. **Future trait boundary.** PROJECT.md notes the extractor signature should be "clean enough to become a trait when KB-OQ5 arrives" (non-markdown file scanning). A pure function is one step from `trait Extractor { fn extract(&self, content: &str, path: &Utf8Path) -> FileExtraction; }`.

4. **Precedent.** Both mdbook-linkcheck and lychee use this exact pattern:
   - mdbook-linkcheck: `markdown(src) -> impl Iterator<Item = (String, Span)>` -- extraction is a pure iterator, validation is separate.
   - lychee: `extract_markdown(input, options) -> Vec<RawUri>` -- extraction returns data, no graph mutation.

### Migration Path

**Step 1:** Create `FileExtraction` and `DiscoveredRef` types. Create `extract_file()` that calls the existing `scan_file()` internally (adapter pattern). `build_graph()` calls `extract_file()` instead of `scan_file()` directly.

**Step 2:** Replace `scan_file()`'s internals with pulldown-cmark. The `extract_file()` -> `FileExtraction` boundary is stable; only the implementation behind it changes.

**Step 3:** Remove `scan_file()` entirely once pulldown-cmark is working. The `ScanResult`, `LabelCandidate` types become dead code and are removed.

## Question 2: How Does pulldown-cmark's into_offset_iter() Map to SourceSpan?

**Confidence: HIGH** (verified via docs.rs/pulldown-cmark/0.13.3)

### pulldown-cmark's Offset Model

`Parser::new_ext(content, options).into_offset_iter()` yields `(Event<'a>, Range<usize>)` where `Range<usize>` is byte offsets into the source string. This is the standard `core::ops::Range<usize>` -- `start` is inclusive, `end` is exclusive.

Key behaviors:
- For `Event::Start(Tag::Heading { .. })`, the range spans the full heading line including `#` markers.
- For `Event::Start(Tag::Link { .. })`, the range spans the full `[text](url)` syntax.
- For `Event::Text(cow_str)`, the range spans just the text content.
- For `Event::Start(Tag::CodeBlock(_))`, the range includes the opening fence.

### Recommended SourceSpan Type

```rust
/// Source location within a file, derived from pulldown-cmark byte offsets.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SourceSpan {
    /// 1-based line number.
    pub(crate) line: u32,
    /// 0-based byte offset from start of file (for precise diagnostics).
    pub(crate) byte_offset: u32,
}
```

### Byte-to-Line Conversion

Build a line offset index once per file, reuse for all spans in that file. This is the standard approach (used by rustc, miette, codespan).

```rust
/// Pre-computed line start offsets for O(log n) byte-to-line lookup.
struct LineIndex {
    /// Byte offset of the start of each line. line_starts[0] == 0.
    line_starts: Vec<u32>,
}

impl LineIndex {
    fn new(content: &str) -> Self {
        let mut starts = vec![0u32];
        for (i, byte) in content.as_bytes().iter().enumerate() {
            if *byte == b'\n' {
                starts.push(u32::try_from(i + 1).expect("file exceeds 4GB"));
            }
        }
        Self { line_starts: starts }
    }

    /// Convert byte offset to 1-based line number via binary search.
    fn line_for_offset(&self, offset: usize) -> u32 {
        let idx = self.line_starts.partition_point(|&start| (start as usize) <= offset);
        u32::try_from(idx).expect("line count fits u32")
    }
}
```

This is ~15 lines, O(n) construction (one pass over content), O(log n) per lookup via `partition_point`. No external crate needed -- consistent with anneal's hand-roll-small-things philosophy.

### Integration Point

Inside `extract_file()`:
1. Build `LineIndex` from full file content.
2. Pass content body (after frontmatter) to pulldown-cmark.
3. For each `(Event, Range)` pair, compute `SourceSpan` using `LineIndex::line_for_offset(range.start)`.
4. Store `SourceSpan` on each `DiscoveredRef`.

**Important offset adjustment:** If frontmatter is present, the body starts at some byte offset N into the file. pulldown-cmark will produce ranges relative to the body string. The `SourceSpan` must add N to get file-absolute offsets before looking up line numbers. This is a one-line adjustment but easy to forget.

## Question 3: What's the Right Boundary Between Extraction and Graph Building?

**Recommendation: Extraction produces identities (strings), not NodeIds.** This is the critical architectural boundary.

### Why Identities, Not NodeIds

1. **Extraction is per-file; NodeIds are graph-global.** A `NodeId` is an arena index into `DiGraph::nodes`. During extraction, the graph doesn't exist yet. You'd need to pass `&mut DiGraph` into extraction, which is exactly the coupling we're trying to eliminate.

2. **Resolution is a separate phase.** The whole point of the typed pipeline is: extract -> resolve -> check. Extraction discovers "this file mentions `OQ-64`". Resolution decides whether `OQ-64` maps to a graph node. Putting NodeIds in extraction collapses these phases.

3. **This is exactly what PendingEdge already does.** The current `PendingEdge` stores `target_identity: String` precisely because the target may not exist yet. The new `DiscoveredRef` follows the same principle but uniformly.

### The FileExtraction Type

```rust
/// Everything extracted from a single markdown file.
pub(crate) struct FileExtraction {
    /// Relative path from root.
    pub(crate) path: Utf8PathBuf,
    /// Parsed frontmatter status.
    pub(crate) status: Option<String>,
    /// Parsed frontmatter metadata.
    pub(crate) metadata: HandleMetadata,
    /// Frontmatter field edges (extensible mapping).
    pub(crate) frontmatter_edges: Vec<FrontmatterEdge>,
    /// All frontmatter keys observed (for init auto-detection).
    pub(crate) frontmatter_keys: Vec<String>,
    /// Section headings discovered in body.
    pub(crate) sections: Vec<DiscoveredSection>,
    /// All references discovered in body and frontmatter.
    pub(crate) refs: Vec<DiscoveredRef>,
}

/// A section heading discovered during extraction.
pub(crate) struct DiscoveredSection {
    pub(crate) heading: String,
    pub(crate) span: SourceSpan,
}

/// A reference discovered during extraction, not yet resolved.
pub(crate) struct DiscoveredRef {
    /// What was found: the raw text of the reference.
    pub(crate) raw: String,
    /// Classification hint for the resolver.
    pub(crate) hint: RefHint,
    /// How this reference should be edged.
    pub(crate) edge_kind: EdgeKind,
    /// Source location.
    pub(crate) span: SourceSpan,
}

/// Classification of a discovered reference for the resolver.
pub(crate) enum RefHint {
    /// Label reference: "OQ-64" -> prefix "OQ", number 64.
    Label { prefix: String, number: u32 },
    /// File path reference: "formal-model/v17.md".
    FilePath(String),
    /// Section cross-reference: "section:4.1".
    SectionRef(String),
    /// Wiki-link: "[[Some Page]]".
    WikiLink(String),
    /// Markdown link to local file: "[text](path.md)".
    MarkdownLink(String),
    /// Frontmatter field reference (already a string identity).
    Frontmatter(String),
}
```

### Graph Assembly Phase

After extraction, a new function assembles the graph:

```rust
pub(crate) fn build_graph_from_extractions(
    extractions: Vec<FileExtraction>,
    config: &AnnealConfig,
) -> BuildResult {
    // 1. Create File nodes for each extraction
    // 2. Create Section nodes
    // 3. Collect all label refs for namespace inference
    // 4. Return BuildResult with pending edges as DiscoveredRefs
}
```

This replaces the inner loop of `build_graph()` but keeps the same `BuildResult` output type (or a compatible successor) so downstream code (resolve, checks) doesn't need immediate changes.

## Question 4: How Do lychee and mdbook-linkcheck Structure Their pulldown-cmark Integration?

**Confidence: HIGH** (verified from source code)

### mdbook-linkcheck Pattern

**Architecture:** Extract -> Categorize -> Validate

```rust
// Extraction: pure function, returns iterator of (url, span)
pub fn markdown(src: &str) -> impl Iterator<Item = (String, Span)> + '_

// Implementation: filter_map over offset_iter
Parser::new_with_broken_link_callback(src, opts, callback)
    .into_offset_iter()
    .filter_map(|(event, range)| match event {
        Event::Start(Tag::Link(_, dest, _))
        | Event::Start(Tag::Image(_, dest, _)) => Some((
            dest.to_string(),
            Span::new(range.start as u32, range.end as u32),
        )),
        _ => None,
    })
```

**Key decisions:**
- Only extracts on `Start` events for links/images (not `End` or `Text`).
- Uses `codespan::Span` for source locations (anneal should hand-roll equivalent).
- Categorizes into `FileSystem { path, fragment }` vs `Url(Url)` after extraction.
- Validation is fully separate from extraction.

### lychee Pattern

**Architecture:** Extract -> Collect -> Check

```rust
// Extraction: returns Vec of RawUri with span info
fn extract_markdown(input: &str, options: &Options) -> Vec<RawUri>

// RawUri carries both the URL and its source context
struct RawUri {
    text: String,
    element: Option<String>,
    attribute: Option<String>,
    span: RawUriSpan,
}
```

**Key decisions:**
- Uses `Parser::new_ext()` with configurable options.
- Handles different link types (inline, reference, wikilink, autolink, email) via match arms on `LinkType`.
- Applies manual span adjustments for different syntax forms (wikilinks offset +2 for `[[`, inline code offset +1 for backtick).
- Uses direct function dispatch (no trait objects) for format-specific extractors.
- All extractors return the same `Vec<RawUri>` output type.

### Lessons for anneal

1. **Extract on `Start` events only.** Both projects extract link information from `Event::Start(Tag::Link { .. })`, not from subsequent `Text` events. The `dest_url` field on the `Tag::Link` variant contains the URL.

2. **Span adjustment is necessary.** pulldown-cmark's ranges for wikilinks and inline code need manual offset correction. anneal will need similar adjustments, plus the frontmatter offset adjustment noted in Question 2.

3. **Keep extraction as a pure function.** Neither project mutates external state during extraction. Both return owned data structures.

4. **Function dispatch, not trait dispatch.** Both use concrete functions, not trait objects. lychee explicitly notes this is for performance. For anneal's single-format case, this is even more appropriate.

## Question 5: Should the Resolution Cascade Use Trait Objects or Simple Functions With Match Arms?

**Recommendation: Simple function with match arms.** No trait objects.

### Rationale

1. **Closed set of variants.** The resolution cascade has a fixed number of strategies: exact match, root-prefix, bare filename, version stem, zero-pad. This is a closed enum, not an open extension point. Match arms are the idiomatic Rust pattern for closed dispatch.

2. **No dynamic composition needed.** Trait objects would be useful if different configurations needed different resolution strategies at runtime. But every anneal invocation runs the same cascade in the same order.

3. **Precedent in codebase.** STATE.md records the decision: "Concrete enum dispatch for CLI output (Serialize not object-safe, no trait objects)." The same reasoning applies here.

4. **Performance.** Resolution runs for every pending edge (could be thousands). Match arms compile to a jump table; trait objects add vtable indirection. Small difference, but in the wrong direction for zero benefit.

### Recommended Resolution Type

```rust
/// Outcome of attempting to resolve a DiscoveredRef.
pub(crate) enum Resolution {
    /// Exact match: identity string matched a known node.
    Exact(NodeId),
    /// Fuzzy match: no exact match, but a close candidate was found.
    /// The String is the matched identity (for "did you mean?" diagnostics).
    Fuzzy {
        node: NodeId,
        matched_identity: String,
        strategy: FuzzyStrategy,
    },
    /// Could not resolve. Candidates (if any) are "did you mean?" suggestions.
    Unresolved {
        candidates: Vec<String>,
    },
}

/// Which fuzzy strategy matched.
#[derive(Clone, Debug, Serialize)]
pub(crate) enum FuzzyStrategy {
    /// Matched after prepending root-relative prefix.
    RootPrefix,
    /// Matched a bare filename unambiguously.
    BareFilename,
    /// Matched via version stem (e.g., "v17" -> "formal-model-v17.md").
    VersionStem,
    /// Matched after zero-padding (e.g., "OQ-1" -> "OQ-01").
    ZeroPad,
}
```

### Resolution Function Signature

```rust
pub(crate) fn resolve_ref(
    discovered: &DiscoveredRef,
    node_index: &HashMap<String, NodeId>,
    filename_index: &HashMap<String, Vec<Utf8PathBuf>>,
    root: &Utf8Path,
    namespaces: &HashSet<String>,
) -> Resolution {
    // 1. Try exact match in node_index
    // 2. For Label hints: check namespace confirmation first
    // 3. For FilePath hints: try root-prefix, bare filename, version stem
    // 4. Collect near-miss candidates for Unresolved
    match &discovered.hint {
        RefHint::Label { prefix, number } => { /* ... */ }
        RefHint::FilePath(path) => { /* ... */ }
        RefHint::SectionRef(section) => { /* ... */ }
        RefHint::WikiLink(target) => { /* ... */ }
        RefHint::MarkdownLink(path) => { /* ... */ }
        RefHint::Frontmatter(identity) => { /* ... */ }
    }
}
```

## pulldown-cmark Integration Details

### Options Configuration

```rust
use pulldown_cmark::{Options, Parser};

fn anneal_parser_options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);  // Heading IDs for section handles
    opts.insert(Options::ENABLE_WIKILINKS);            // [[wiki links]] as native events
    opts.insert(Options::ENABLE_TABLES);               // Don't extract refs from table syntax
    opts.insert(Options::ENABLE_STRIKETHROUGH);        // Proper ~~strikethrough~~ handling
    // Do NOT enable ENABLE_YAML_STYLE_METADATA_BLOCKS -- anneal has its own
    // frontmatter parser that extracts structured data. pulldown-cmark's metadata
    // blocks only give raw text.
    opts
}
```

### Key Event Processing

```rust
for (event, range) in Parser::new_ext(body, anneal_parser_options()).into_offset_iter() {
    match event {
        // Headings: create DiscoveredSection
        Event::Start(Tag::Heading { level, .. }) => {
            in_heading = true;
            heading_text.clear();
            heading_range = range.clone();
        }
        Event::Text(text) if in_heading => {
            heading_text.push_str(&text);
        }
        Event::End(TagEnd::Heading(_)) => {
            in_heading = false;
            sections.push(DiscoveredSection {
                heading: heading_text.clone(),
                span: line_index.span_for(heading_range.start + frontmatter_offset),
            });
        }

        // Links: create DiscoveredRef with appropriate RefHint
        Event::Start(Tag::Link { link_type, dest_url, .. }) => {
            let hint = classify_link(&link_type, &dest_url);
            // The range covers the full [text](url) syntax
            refs.push(DiscoveredRef {
                raw: dest_url.to_string(),
                hint,
                edge_kind: EdgeKind::Cites,  // refined by line context
                span: line_index.span_for(range.start + frontmatter_offset),
            });
        }

        // Text nodes: scan for labels, section refs via regex
        Event::Text(text) => {
            scan_text_for_labels(&text, &range, frontmatter_offset, &line_index, &mut refs);
        }

        // Code blocks and inline code: SKIP (structural, not manual toggle)
        Event::Start(Tag::CodeBlock(_)) => { /* pulldown-cmark won't emit
                                                 Text events inside code blocks
                                                 as regular Text -- they come as
                                                 Event::Text inside the CodeBlock
                                                 context, but we track state */ }
        Event::Code(_) => { /* inline code, skip entirely */ }

        _ => {}
    }
}
```

### What Regex Stays For

pulldown-cmark handles document structure (headings, links, code blocks, wikilinks). But anneal's domain-specific patterns still need regex:

| Pattern | Why Regex | pulldown-cmark Role |
|---------|-----------|---------------------|
| `[A-Z][A-Z_]*-\d+` (labels) | Domain-specific, not markdown syntax | Provides the `Text` events to scan |
| `[section]\d+(\.\d+)*` (section refs) | Domain-specific notation | Provides the `Text` events to scan |
| `[a-z0-9_/-]+\.md` (file paths) | Bare path references in prose | Provides the `Text` events to scan |

The `RegexSet` fast-path pattern is preserved but applied to `Text` event content instead of raw lines. The five-pattern set becomes three patterns (headings and file paths are now handled by pulldown-cmark events).

### What Regex Loses

| Current Regex | pulldown-cmark Replacement |
|---------------|---------------------------|
| `^#{1,6}\s` (headings) | `Event::Start(Tag::Heading { level, .. })` |
| Code block toggle (`in_code_block`) | Structural: no `Text` events inside `CodeBlock` context |
| URL rejection heuristic | `LinkType::Autolink` / `LinkType::Email` classified natively |
| `[a-z0-9_/-]+\.md` in link context | `Event::Start(Tag::Link { dest_url, .. })` for markdown links |

### Code Block Handling: Structural vs Manual

The current code manually tracks `in_code_block` state:

```rust
// CURRENT: fragile, misses edge cases
if trimmed.starts_with("```") {
    in_code_block = !in_code_block;
    continue;
}
if in_code_block { continue; }
```

pulldown-cmark handles this structurally. Text inside code blocks arrives as `Event::Text` nested within `Event::Start(Tag::CodeBlock(_))` / `Event::End(TagEnd::CodeBlock)` pairs. By only scanning `Text` events that occur outside code block context, the manual toggle disappears entirely. This eliminates a class of false positives (e.g., indented code blocks without fences, which the current regex approach misses).

## Recommended Component Boundaries

### New Module: `extract.rs`

Contains:
- `FileExtraction`, `DiscoveredRef`, `DiscoveredSection`, `RefHint`, `SourceSpan` types
- `extract_file()` function (the pulldown-cmark-powered extractor)
- `LineIndex` (byte-to-line conversion)
- `classify_link()` (LinkType -> RefHint mapping)

Does NOT contain:
- Graph construction (stays in `parse.rs` or moves to a new `build.rs`)
- Resolution logic (stays in `resolve.rs`)
- File walking (stays in `parse.rs`)

### Modified Modules

| Module | Changes |
|--------|---------|
| `parse.rs` | `build_graph()` calls `extract_file()` instead of `scan_file()`. Walk logic stays. `ScanResult`, `LabelCandidate` removed after migration. `FrontmatterEdge` may stay or move to `extract.rs`. |
| `resolve.rs` | `resolve_all()` gains `Resolution` enum return per-ref. `PendingEdge` replaced by `DiscoveredRef` (or adapter). `ResolveStats` enriched with fuzzy match counts. |
| `checks.rs` | `Diagnostic` gains `Evidence` enum field. `check_existence()` receives `Resolution::Unresolved` with candidates instead of raw `PendingEdge`. |
| `main.rs` | Minimal changes: `collect_unresolved_owned()` adapts to new types. Pipeline order unchanged. |

### Unchanged Modules

| Module | Why Unchanged |
|--------|--------------|
| `graph.rs` | `DiGraph`, `Edge`, `EdgeKind` are stable. No structural changes. |
| `handle.rs` | `Handle`, `HandleKind`, `NodeId` are stable. |
| `lattice.rs` | Operates on status strings, not extraction types. |
| `impact.rs` | Operates on graph structure, not extraction types. |
| `snapshot.rs` | Operates on graph + diagnostics, not extraction types. |
| `config.rs` | Unchanged. |
| `cli.rs` | Only changes when `Diagnostic` gains `Evidence` (display logic). |

## Recommended Build Order

### Phase A: Types Foundation (no behavior change)

**Files created:** `src/extract.rs`
**Files modified:** `src/main.rs` (add `mod extract;`)

1. Define `SourceSpan`, `DiscoveredRef`, `RefHint`, `DiscoveredSection`, `FileExtraction` types.
2. Define `LineIndex` with `new()` and `line_for_offset()`.
3. Define `Resolution` enum and `FuzzyStrategy`.
4. All types are defined but not yet used. Tests compile, `just check` passes.

### Phase B: Extraction Adapter (behavior preserved)

**Files modified:** `src/extract.rs`, `src/parse.rs`

1. Implement `extract_file()` that calls existing `split_frontmatter()` + `parse_frontmatter()` + `scan_file()` internally.
2. Convert `ScanResult` -> `Vec<DiscoveredRef>` inside the adapter.
3. Modify `build_graph()` to call `extract_file()`, convert `FileExtraction` back to the old intermediate types.
4. All existing tests pass. New unit tests for `extract_file()`.

### Phase C: pulldown-cmark Swap (behavior change behind stable interface)

**Files modified:** `src/extract.rs`, `Cargo.toml`
**Dependency added:** `pulldown-cmark = "0.13"`

1. Add `pulldown-cmark` to `Cargo.toml`.
2. Rewrite `extract_file()` internals to use `Parser::new_ext().into_offset_iter()`.
3. `FileExtraction` output is the same type -- only the implementation changes.
4. Remove `scan_file()` and its regex-based internals.
5. Reduce `PATTERN_SET` from 5 patterns to 3 (labels, section refs, version refs).
6. Run against Murail and Herald corpora. Compare diagnostic counts.

### Phase D: Resolution Enrichment

**Files modified:** `src/resolve.rs`, `src/parse.rs`

1. `resolve_all()` returns per-ref `Resolution` values alongside `ResolveStats`.
2. `PendingEdge` replaced by `DiscoveredRef` throughout.
3. `LabelCandidate` removed (subsumed by `RefHint::Label`).
4. Old types (`ScanResult`, `LabelCandidate`, `PendingEdge`) deleted.

### Phase E: Diagnostic Enrichment

**Files modified:** `src/checks.rs`, `src/cli.rs`

1. Add `Evidence` enum to `Diagnostic`.
2. `check_existence()` populates `Evidence::BrokenRef { candidates }` from `Resolution::Unresolved`.
3. `Diagnostic::print_human()` renders evidence (candidates, source spans).

## Data Flow Diagram

```
                    CURRENT                              NEW (v1.1)
                    -------                              ----------

  walk files ──> build_graph()           walk files ──> extract_file() per file
                   |                                       |
                   |-- split_frontmatter                   |-- split_frontmatter
                   |-- parse_frontmatter                   |-- parse_frontmatter
                   |-- scan_file(body, &mut graph)         |-- pulldown-cmark parse
                   |     creates Section nodes             |     yield DiscoveredRef[]
                   |     returns ScanResult                |     yield DiscoveredSection[]
                   |-- creates File node                   |-- return FileExtraction
                   |-- creates PendingEdge[]               |
                   v                                       v
              BuildResult                          Vec<FileExtraction>
                   |                                       |
                   v                                       v
             resolve_all()                     build_graph_from_extractions()
                   |                                       |
                   |-- infer_namespaces                    |-- create File + Section nodes
                   |-- resolve_labels                      |-- collect DiscoveredRef[]
                   |-- resolve_versions                    v
                   |-- resolve_pending_edges         BuildResult (same shape)
                   v                                       |
             ResolveStats                                  v
                   |                               resolve_all() -- now with Resolution enum
                   v                                       |
             run_checks()                                  v
                   |                               run_checks() -- with Evidence
                   v                                       v
           Vec<Diagnostic>                        Vec<Diagnostic> (enriched)
```

## Anti-Patterns to Avoid

### Anti-Pattern 1: Passing &mut DiGraph Into Extraction
**What:** Making `extract_file()` take `&mut DiGraph` to create nodes during extraction.
**Why bad:** Re-creates the coupling we're trying to eliminate. Makes extraction untestable in isolation. Prevents future parallelization.
**Instead:** Return `FileExtraction` with string identities. Let graph assembly happen separately.

### Anti-Pattern 2: Big-Bang Rewrite
**What:** Rewriting `parse.rs`, `resolve.rs`, and `checks.rs` simultaneously.
**Why bad:** If something breaks, unclear which change caused it. Can't run tests midway.
**Instead:** Follow the A-B-C-D-E build order. Each phase keeps `just check` green.

### Anti-Pattern 3: pulldown-cmark as Full Replacement for Regex
**What:** Trying to extract labels (`OQ-64`) from pulldown-cmark events alone.
**Why bad:** Labels are domain-specific patterns within prose text, not markdown syntax. pulldown-cmark gives you `Text("See OQ-64 for details")` -- you still need regex to find `OQ-64` within that text.
**Instead:** pulldown-cmark for structure (headings, links, code blocks), regex for domain patterns within text spans.

### Anti-Pattern 4: Storing pulldown-cmark Events
**What:** Keeping `Event` or `Tag` values in `FileExtraction`.
**Why bad:** Events borrow from the source string (`Event<'a>`). `FileExtraction` needs to be `'static` (or at least outlive the parser). Storing events forces lifetime coupling.
**Instead:** Convert to owned types (`DiscoveredRef`, `DiscoveredSection`) immediately during iteration.

### Anti-Pattern 5: Separate SourceSpan Crate
**What:** Adding `codespan` or `miette` just for the span type.
**Why bad:** Adds a dependency for a ~20-line type. anneal's philosophy is to hand-roll small things.
**Instead:** Define `SourceSpan` and `LineIndex` directly in `extract.rs`.

## Dependency Impact

### Build Time

Current: 10 crates, ~10s clean build.
After: 11 crates + pulldown-cmark's 3 transitive deps (bitflags, memchr, unicase).

- `memchr` is likely already a transitive dep of `regex`. No net addition.
- `bitflags` is lightweight (~0.5s compile).
- `unicase` is lightweight (~0.3s compile).
- `pulldown-cmark` itself: ~2-3s compile (13K SLoC).

**Estimated new clean build: ~12-13s.** This is slightly above the 10s target but acceptable given the false-positive elimination value. Incremental builds (the common case) are unaffected.

### Runtime Performance

pulldown-cmark is a pull parser with SIMD-accelerated scanning on x64. It processes markdown in a single pass. For anneal's corpus sizes (262 files, <100ms total pipeline), the parser swap will not be the bottleneck.

The regex `RegexSet` fast path (5 patterns, one automaton pass per line) is being replaced by pulldown-cmark's structural parsing (one pass over the whole document) plus regex on text spans only. Net effect: roughly equivalent, possibly faster since code blocks are skipped structurally rather than checked per-line.

## Sources

- [pulldown-cmark OffsetIter API](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.OffsetIter.html) -- verified Iterator<Item = (Event, Range<usize>)>
- [pulldown-cmark Event enum](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/enum.Event.html) -- 13 variants
- [pulldown-cmark Tag enum](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/enum.Tag.html) -- Heading, Link, CodeBlock variants
- [pulldown-cmark Options](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.Options.html) -- ENABLE_WIKILINKS, ENABLE_HEADING_ATTRIBUTES
- [pulldown-cmark LinkType enum](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/enum.LinkType.html) -- WikiLink variant with has_pothole
- [pulldown-cmark Wikilinks spec](https://pulldown-cmark.github.io/pulldown-cmark/specs/wikilinks.html) -- [[syntax]] behavior
- [pulldown-cmark on lib.rs](https://lib.rs/crates/pulldown-cmark) -- v0.13.3, 8.67M downloads/month
- [mdbook-linkcheck architecture](https://adventures.michaelfbryan.com/posts/linkchecker) -- extraction/validation separation pattern
- [lychee GitHub](https://github.com/lycheeverse/lychee) -- extract module structure, RawUri type
