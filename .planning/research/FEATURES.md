# Feature Landscape: Parser Hardening & UX Polish

**Domain:** Markdown knowledge graph — reference extraction, diagnostic enrichment, false positive suppression
**Researched:** 2026-03-29
**Confidence:** HIGH (tool source code and docs verified)

## Capability 1: Markdown Reference Extraction (pulldown-cmark body scanner)

### What existing tools do

**Lychee** (link checker, Rust) uses pulldown-cmark 0.13 to extract links from markdown. Its approach:
- Iterates `into_offset_iter()` which yields `(Event, Range<usize>)` tuples
- Filters for `Tag::Link`, `Tag::Image`, `Tag::CodeBlock`, `Tag::HtmlBlock`, and `Event::Text`
- Maintains boolean flags for context: `inside_code_block`, `inside_link_block`, `inside_wikilink_block`
- Uses a `SourceSpanProvider` to convert byte offsets to line/column — this is a custom struct, not from pulldown-cmark itself
- Applies offset adjustments per link type: wikilinks offset by 2 (for `[[`), inline code by 1 (for backtick)
- Has a separate `extract_markdown_fragments()` for heading anchor generation (GitHub kebab-case slugs)
- Source: [lychee-lib/src/extract/markdown.rs](https://github.com/lycheeverse/lychee)

**mdbook-linkcheck** also uses pulldown-cmark. Key pattern:
- Wraps each extracted link with a `Span` tracking its byte range in the source
- Uses the `codespan` crate for source file management and the `codespan-reporting` crate for rendering diagnostics with line numbers and source snippets
- Filters for `Event::Start(Tag::Link {...})` and `Event::Start(Tag::Image {...})`
- Categorizes resolved links into local file paths vs web URLs
- Source: [Creating a Robust, Reusable Link-Checker](https://adventures.michaelfbryan.com/posts/linkchecker)

### pulldown-cmark capabilities (v0.13.3)

Relevant extensions anneal could enable:
- `ENABLE_WIKILINKS` — native `[[WikiLink]]` parsing (Obsidian-style), yields proper `Tag::Link` events
- `ENABLE_YAML_STYLE_METADATA_BLOCKS` — frontmatter between `---` delimiters (could replace hand-rolled `split_frontmatter`)
- `ENABLE_HEADING_ATTRIBUTES` — custom heading IDs like `{#my-id}` for explicit section anchors
- `ENABLE_FOOTNOTES` — footnote references `[^IDENT]` (currently invisible to anneal's regex scanner)

Key API: `Parser::new_ext(input, options).into_offset_iter()` yields `(Event<'_>, Range<usize>)`. The `Range<usize>` is byte offsets into the source string, NOT line numbers. Line number conversion requires counting newlines up to the byte offset — a ~15 line utility function.

### Table Stakes vs Differentiators

| Feature | Category | Complexity | Depends On |
|---------|----------|------------|------------|
| Parse `[text](url.md)` markdown links natively | Table Stakes | Low | pulldown-cmark dep |
| Parse `[[WikiLink]]` references natively | Differentiator | Low | pulldown-cmark with `ENABLE_WIKILINKS` |
| Track byte offset per extracted reference | Table Stakes | Low | `into_offset_iter()` |
| Convert byte offset to line number | Table Stakes | Low | Custom `fn byte_offset_to_line()` |
| Skip code blocks structurally (not regex toggle) | Table Stakes | Low | pulldown-cmark events |
| Extract heading slugs from AST | Table Stakes | Low | `Tag::Heading` events |
| Distinguish inline links from reference-def links | Differentiator | Med | `LinkType` enum in pulldown-cmark |
| Classify raw text matches vs structured links | Differentiator | Med | Two-pass: AST links first, then text scan |

### Recommendation

Replace the 5-pattern `RegexSet` body scanner with pulldown-cmark event iteration. Keep the regex patterns ONLY for text content nodes (`Event::Text`) where labels like `OQ-64` appear in running prose rather than in markdown link syntax.

Pattern to follow from lychee: iterate offset events, maintain context flags, extract structured links from AST events, then scan `Event::Text` content with regex for label/section references that aren't markdown links.

**Do NOT** replace `split_frontmatter()` with pulldown-cmark's `ENABLE_YAML_STYLE_METADATA_BLOCKS` — anneal already hand-rolls this (15 lines) and feeds the YAML to `serde_yaml_ng`. Replacing it would add complexity for zero value since anneal needs the raw YAML string, not pulldown-cmark's metadata event.

### Estimated Complexity

**Medium.** The core iteration is straightforward (~100 lines), but the `DiscoveredRef` + `RefHint` type design and the two-pass approach (AST events then text scan) need careful design. The current `scan_file()` function is ~120 lines; replacement will be ~150-180 lines but with much better precision.

---

## Capability 2: "Did You Mean?" Suggestions on Broken References

### What existing tools do

**Rust compiler (rustc)** uses Levenshtein distance for "did you mean?" suggestions. Key design decisions:
- Wording: "there is a struct with a similar name: `Foo`" NOT "did you mean: `Foo`?" — the compiler style guide explicitly forbids question-form suggestions
- Suggestions carry an `Applicability` level: `MachineApplicable`, `MaybeIncorrect`, `HasPlaceholders`, `Unspecified`
- Only suggests when edit distance is below a threshold relative to the identifier length
- Source: [Rust Compiler Dev Guide: Diagnostics](https://rustc-dev-guide.rust-lang.org/diagnostics.html)

**clap** (CLI parser) uses the `strsim` crate for "Did you mean '--myoption'?" on typos. This is an optional feature flag (`suggestions`) that brings in strsim as a dependency. Uses Jaro-Winkler or Levenshtein with a threshold.

**strsim** crate provides: Hamming, Levenshtein, OSA, Damerau-Levenshtein, Jaro, Jaro-Winkler, Sorensen-Dice. All have normalized (0.0-1.0) variants. This is what clap uses internally.

### anneal-specific candidates

The v1.1 PROJECT.md describes a resolution cascade with four specific fuzzy matchers:
1. **Root-prefix**: `spec.md` -> `specs/detailed-spec.md` (bare filename in different directory)
2. **Bare filename**: `model.md` -> `formal-model/model.md` (path resolution)
3. **Version stem**: `formal-model-v17` -> `formal-model/formal-model-v17.md` (missing .md or path)
4. **Zero-pad**: `OQ-1` -> `OQ-01` (number formatting difference)

These are deterministic transforms, not fuzzy string matching. They should be tried BEFORE any edit-distance fallback.

### Table Stakes vs Differentiators

| Feature | Category | Complexity | Depends On |
|---------|----------|------------|------------|
| Deterministic resolution cascade (root-prefix, bare filename, version stem, zero-pad) | Table Stakes | Low | Existing `resolve_bare_filename()` + extend |
| Show best candidate on E001 broken-reference errors | Table Stakes | Med | Resolution cascade |
| Edit-distance fuzzy fallback (strsim) | Differentiator | Low | New dep: strsim ~0.11 |
| "there is a handle with a similar name: X" wording | Table Stakes | Low | None (style choice) |
| Applicability levels on suggestions | Anti-Feature | Med | Over-engineered for anneal's scope |
| Auto-fix mode that rewrites files | Anti-Feature | High | Violates KB-P1 (files are truth, anneal reads only) |

### Recommendation

Implement the four deterministic transforms first. They cover the actual false positives observed in Herald (50 errors) and Murail (186 errors). Add strsim only if deterministic candidates miss common cases.

When adding strsim, use `strsim::jaro_winkler` with a threshold of ~0.8 for handle IDs. Levenshtein is better for long strings; Jaro-Winkler is better for short identifiers like `OQ-64` which is anneal's primary use case.

Follow rustc's wording style: "similar handle exists: `OQ-64`" not "did you mean `OQ-64`?"

### Estimated Complexity

**Low-Medium.** The four deterministic transforms are ~40 lines each. The `Resolution` enum (Exact/Fuzzy/Unresolved) is clean type design that replaces the current bool-return from `resolve_pending_edges`. strsim is a zero-cost optional addition (~100KB, pure Rust, no transitive deps).

---

## Capability 3: Content Snippet/Preview in Lookup Commands

### What existing tools do

**grep -C N** shows N context lines around matches. This is the universal pattern for content preview: show the match with surrounding context.

**bat** (cat replacement) provides syntax-highlighted file preview with line numbers, a pattern that terminal users expect from modern CLI tools.

**rust-analyzer** and **codespan-reporting** both render source snippets with:
- Line numbers in the gutter
- Underline/caret pointing at the specific span
- 1-3 context lines above and below the relevant span

**mdbook-linkcheck** uses `codespan-reporting` to render diagnostics with source snippets. Pattern: store the full source text of each file, then pass `(file_id, byte_range)` to the reporting crate which extracts and renders the snippet.

### anneal-specific needs

The `anneal get <handle>` command currently shows: id, kind, status, file, outgoing edges, incoming edges. It does NOT show any content from the file itself. For a "convergence assistant," seeing the first few lines of a handle's content is essential for orientation.

For `anneal check` diagnostics, showing the line where a broken reference occurs helps the user understand and fix the issue without opening the file.

### Table Stakes vs Differentiators

| Feature | Category | Complexity | Depends On |
|---------|----------|------------|------------|
| Show first N lines of file content in `get` output | Table Stakes | Low | File I/O (already done during scan) |
| Show heading + first paragraph for Section handles in `get` | Differentiator | Med | Heading range detection |
| Show frontmatter summary in `get` (status, updated, edges) | Table Stakes | Low | Already parsed, just display |
| Show source line in `check` diagnostics for broken references | Table Stakes | Med | Line number tracking (Capability 1) |
| Truncate long snippets with `...` | Table Stakes | Low | String truncation |
| Syntax highlighting in snippets | Anti-Feature | Med | Would require `syntect` or similar; overkill for markdown |

### Recommendation

For `anneal get`:
- Add a `content_preview` field: first 5 non-empty body lines for File handles, heading + first paragraph for Section handles
- Show frontmatter summary (status, updated, key edges) as structured fields — already partly done
- Respect `--json` by including preview as a string field

For `anneal check` diagnostics:
- Once line numbers are tracked (Capability 1), show the offending line in the diagnostic
- Follow compiler-style format: `error[E001]: broken reference: OQ-99` / `  --> file.md:42` / `   | the line content here`
- Do NOT adopt full codespan-reporting — it's designed for programming languages with rich span info. Anneal's diagnostics are simpler (one-line references, not multi-line expressions). The current hand-rolled `Diagnostic::print_human()` is good enough with a line number and source line added.

### Estimated Complexity

**Low.** Content preview is ~30 lines for file reading + truncation. Source line in diagnostics requires line number (from Capability 1) plus reading the line from the file content — another ~20 lines. No new dependencies.

---

## Capability 4: False Positive Suppression Configuration

### What existing tools do

**Lychee** uses three suppression mechanisms:
1. `.lycheeignore` file — one regex pattern per line, matches against full URLs including scheme
2. `lychee.toml` config — `exclude` field accepts regex patterns; `exclude_all_private`, `exclude_mail` boolean flags
3. CLI flag `--exclude <pattern>` for ad-hoc suppression
4. No inline suppression comments
- Source: [Excluding Links](https://lychee.cli.rs/recipes/excluding-links/)

**Vale** uses four mechanisms:
1. `.vale.ini` config file — global settings, style paths, min alert level
2. Per-scope rules — target `heading.h1`, `paragraph`, `table.cell` etc.
3. Inline comments: `<!-- vale off -->` / `<!-- vale on -->` toggles all rules; `<!-- vale RuleName = NO -->` / `<!-- vale RuleName = YES -->` toggles specific rules
4. Vocabulary files: `accept.txt` and `reject.txt` for per-project term lists
- Source: [Vale documentation](https://vale.sh)

**Cargo clippy** uses three mechanisms:
1. `Cargo.toml` `[lints.clippy]` section — per-lint allow/deny/warn, with priority ordering
2. `clippy.toml` / `.clippy.toml` — lint-specific threshold configuration (not allow/deny)
3. Inline attributes: `#[allow(clippy::lint_name)]` on items, modules, or crate root
- Source: [Clippy Configuration](https://doc.rust-lang.org/clippy/configuration.html)

**rust-analyzer** provides:
1. `rust-analyzer.diagnostics.disabled` — list of diagnostic codes to suppress
2. `rust-analyzer.diagnostics.enable` — master toggle
3. Inherits rustc/clippy `#[allow(...)]` attributes

### Patterns observed across tools

| Pattern | Used By | anneal Fit |
|---------|---------|------------|
| Config file regex exclusion list | lychee, clippy | YES — `anneal.toml` already exists |
| Inline comment suppression | vale, clippy | MAYBE — markdown comments (`<!-- -->`) could work |
| Per-diagnostic-code suppression | clippy, rust-analyzer | YES — anneal already has error codes (E001, W001, etc.) |
| Per-file suppression | lychee (exclude-path) | YES — extends existing `exclude` dirs config |
| Per-handle suppression | None directly | YES — unique to anneal; suppress by handle ID |
| Vocabulary / accept list | vale | YES — maps to confirmed/rejected handle namespaces (already exists) |

### Table Stakes vs Differentiators

| Feature | Category | Complexity | Depends On |
|---------|----------|------------|------------|
| `anneal.toml` ignore list for specific handle IDs | Table Stakes | Low | Config parsing |
| `anneal.toml` regex pattern exclusion for references | Table Stakes | Low | Config parsing + regex |
| Per-diagnostic-code suppression (suppress all W001) | Table Stakes | Low | Config parsing |
| Per-file suppression (exclude specific files from checks) | Table Stakes | Low | Extends existing `exclude` |
| Inline `<!-- anneal:ignore -->` comment suppression | Differentiator | Med | pulldown-cmark HTML comment detection |
| Inline `<!-- anneal:ignore E001 -->` code-specific | Differentiator | Med | Comment parsing + code matching |
| `--suppress` CLI flag for ad-hoc suppression | Table Stakes | Low | CLI arg |
| Baseline file (record current state, only flag new issues) | Differentiator | High | Snapshot diffing |

### Recommendation

**Phase 1 (config-based):** Add three fields to `anneal.toml`:

```toml
[check]
# Suppress specific diagnostic codes globally
suppress = ["W001", "I001"]

# Suppress specific handle references from triggering E001
ignore_refs = ["SHA-256", "AVX-512", "RFC-2606"]

# Suppress diagnostics for files matching these patterns
ignore_files = ["archive/**", "*.draft.md"]
```

This covers the most common false positive sources observed in real usage:
- Herald: frontmatter URLs and prose false positives (ignore_refs covers URL-like patterns)
- Murail: terminal-file noise (already handled by `--active-only`; `ignore_files` extends this)

**Phase 2 (inline):** Add `<!-- anneal:ignore -->` next-line suppression and `<!-- anneal:ignore E001 -->` code-specific suppression. This follows vale's pattern but with anneal-prefixed comments to avoid conflicts. Requires pulldown-cmark to detect HTML comments (it already emits `Event::Html` for them).

**Do NOT** implement a baseline file approach — it's high complexity, creates state management burden, and conflicts with anneal's stateless-recompute philosophy (KB-P1).

### Estimated Complexity

**Phase 1: Low.** Config parsing is ~30 lines (anneal.toml already parsed). Filter application is ~20 lines in `cmd_check()`. No new dependencies.

**Phase 2: Medium.** Inline comment detection requires pulldown-cmark HTML event processing and associating comments with the next reference on the following line. ~60 lines.

---

## Capability 5: Line Number Tracking in Diagnostics

### What existing tools do

**Lychee** solves this with a custom `SourceSpanProvider`:
- Builds a line-start-offset index from the source text (precompute `Vec<usize>` of byte positions where each line starts)
- Uses binary search on the index to convert byte offset -> line number
- Wraps this in `OffsetSpanProvider` for per-element offset adjustments

**mdbook-linkcheck** uses the `codespan` crate:
- Each file is registered as a `FileId`
- Spans are `(FileId, Range<usize>)` — byte ranges into the source
- The `codespan-reporting` renderer converts spans to line numbers and renders source snippets with gutter line numbers

**rust-analyzer** maintains source maps (`BodySourceMap`) mapping between AST nodes and source code positions. This is heavy infrastructure for a full IDE; anneal does not need this.

**Cargo clippy / rustc** track spans as byte ranges `(lo: BytePos, hi: BytePos)` in the original source. The `SourceMap` structure handles conversion to line/column. Again, heavier than anneal needs.

### Pattern synthesis

All tools follow the same core pattern:
1. **Precompute** a line-starts index: `Vec<usize>` where `line_starts[i]` is the byte offset of the start of line `i`
2. **Store** byte offset with each extracted reference
3. **Convert** byte offset to line number via binary search: `line_starts.partition_point(|&start| start <= byte_offset)`

This is ~15-20 lines of Rust. No crate needed.

### Table Stakes vs Differentiators

| Feature | Category | Complexity | Depends On |
|---------|----------|------------|------------|
| Track byte offset per discovered reference | Table Stakes | Low | pulldown-cmark `into_offset_iter()` |
| Convert byte offset to line number | Table Stakes | Low | ~15 line utility |
| Show line number in `check` diagnostics | Table Stakes | Low | Diagnostic struct already has `line: Option<u32>` |
| Show source line content in diagnostics | Differentiator | Low | Read line from cached content |
| Show caret/underline pointing at reference | Differentiator | Med | Column tracking + rendering |
| Full codespan-reporting integration | Anti-Feature | High | Heavy dep, over-engineered for anneal |
| Full miette integration | Anti-Feature | High | Same — designed for rich error types |

### Recommendation

**Hand-roll the line-starts index.** This is a ~15 line function:

```rust
fn build_line_starts(source: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(source.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

fn byte_offset_to_line(line_starts: &[usize], offset: usize) -> u32 {
    line_starts.partition_point(|&start| start <= offset) as u32
}
```

This follows anneal's philosophy of hand-rolling small utilities (graph ~135 lines, frontmatter ~15 lines, JSONL ~30 lines) instead of pulling in crates for 5% of their surface area.

Store line numbers in `DiscoveredRef` (the new extraction type). Flow them through to `Diagnostic.line`. The `Diagnostic` struct already has `line: Option<u32>` — currently always `None`. Populating it is the primary deliverable.

**Do NOT** adopt codespan-reporting or miette. They're designed for programming language diagnostics with multi-span errors, notes, and suggestions. Anneal's diagnostics are single-location with a message. The current `Diagnostic::print_human()` format (`error[E001]: message` / `  -> file.md:42`) is correct and sufficient — just needs the `:42` part to not be `None`.

### Estimated Complexity

**Low.** The line-starts index is ~15 lines. Storing offsets in `DiscoveredRef` is part of Capability 1 (pulldown-cmark migration). Flowing line numbers to `Diagnostic` is ~10 lines of plumbing. Total: ~25 new lines, no new dependencies.

---

## Anti-Features

Features to explicitly NOT build in v1.1.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| Full codespan-reporting / miette integration | 40KB+ dep, designed for programming language IDEs. Anneal's diagnostics are simpler: one location, one message. | Hand-roll line numbers (~15 lines) and source line display (~20 lines) |
| Auto-fix mode (rewrite files to fix references) | Violates KB-P1 (files are truth). anneal reads and reports, never writes content. | Show "similar handle exists" suggestions; user fixes manually |
| Syntax highlighting in content previews | Would require syntect (~2MB) or tree-sitter. Markdown content preview doesn't need highlighting. | Plain text preview with line numbers |
| Baseline/snapshot-based suppression | Creates persistent state, conflicts with stateless recompute philosophy. | Config-based suppression + `--active-only` filtering |
| comrak instead of pulldown-cmark | comrak builds a full AST (heavier allocation). anneal only needs event iteration, not rendering. pulldown-cmark's pull parser is faster for extraction-only use. | Use pulldown-cmark for event-based extraction |
| LSP/Language Server integration | Premature — CLI must prove useful first (KB-OQ4). | Focus on CLI diagnostics; LSP is v2+ |
| Per-line inline suppression (`<!-- anneal:ignore-next-line -->`) | Complex to implement correctly in streaming parser. | Per-block suppression (`<!-- anneal:ignore -->` / `<!-- anneal:end-ignore -->`) is sufficient |

---

## Feature Dependencies

```
pulldown-cmark body scanner (Cap 1)
  |
  +-> byte offset tracking (Cap 5) -- line_starts index
  |     |
  |     +-> line numbers in Diagnostic struct (Cap 5)
  |     |     |
  |     |     +-> source line content in diagnostics (Cap 3)
  |     |
  |     +-> content preview in `get` command (Cap 3) -- uses file content already read
  |
  +-> DiscoveredRef type with RefHint classification
  |     |
  |     +-> Resolution cascade with candidates (Cap 2)
  |           |
  |           +-> "similar handle exists" in E001 diagnostics (Cap 2)
  |           |
  |           +-> strsim fuzzy fallback (Cap 2, optional)
  |
  +-> HTML comment detection for inline suppression (Cap 4, Phase 2)

anneal.toml config extensions (Cap 4, Phase 1)
  -- independent of pulldown-cmark migration, can ship first
```

---

## MVP Recommendation

### Must ship (Table Stakes):

1. **pulldown-cmark body scanner** replacing RegexSet — this is the foundation for all other capabilities. Without it, line numbers, structured link extraction, and HTML comment detection are all blocked.

2. **Line number tracking** in diagnostics — the `Diagnostic` struct already has the field. Populating it transforms `check` output from "search your whole file" to "go to line 42."

3. **Config-based false positive suppression** — `[check]` section in `anneal.toml` with `suppress`, `ignore_refs`, `ignore_files`. Independent of pulldown-cmark; can ship in parallel.

4. **Deterministic resolution cascade** with four transforms (root-prefix, bare filename, version stem, zero-pad) — covers the observed false positives without needing strsim.

### Should ship (High-value Differentiators):

5. **Content preview in `get`** — first 5 body lines for File handles, heading + first paragraph for Section handles.

6. **"Similar handle exists"** message on E001 errors — uses resolution cascade candidates from (4).

### Defer:

7. **Inline comment suppression** — medium complexity, requires pulldown-cmark HTML event handling. Ship after the body scanner is stable.

8. **strsim fuzzy matching** — only needed if deterministic cascade doesn't cover enough cases. Evaluate after shipping (4).

9. **Source line content in diagnostics** — nice-to-have beyond line numbers. Low complexity but lower priority than the structural improvements.

---

## Sources

### HIGH confidence (official docs, source code)
- [pulldown-cmark Options](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.Options.html) — v0.13.3 extension flags
- [pulldown-cmark OffsetIter](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.OffsetIter.html) — byte offset API
- [Clippy Configuration](https://doc.rust-lang.org/clippy/configuration.html) — allow/deny patterns
- [Rust Compiler Dev Guide: Diagnostics](https://rustc-dev-guide.rust-lang.org/diagnostics.html) — suggestion wording guide
- [strsim crate](https://docs.rs/strsim/latest/strsim/) — string similarity algorithms

### MEDIUM confidence (verified tool documentation)
- [lychee source code](https://github.com/lycheeverse/lychee) — pulldown-cmark extraction patterns
- [lychee: Excluding Links](https://lychee.cli.rs/recipes/excluding-links/) — suppression mechanisms
- [mdbook-linkcheck: Creating a Robust Link-Checker](https://adventures.michaelfbryan.com/posts/linkchecker) — codespan + pulldown-cmark patterns
- [Vale documentation](https://vale.sh) — inline suppression `<!-- vale off/on -->`
- [codespan-reporting](https://docs.rs/codespan-reporting) — diagnostic rendering crate
- [miette](https://docs.rs/miette/latest/miette/) — error reporting crate

### LOW confidence (community discussion, single source)
- [lychee DeepWiki](https://deepwiki.com/lycheeverse/lychee) — architecture overview
