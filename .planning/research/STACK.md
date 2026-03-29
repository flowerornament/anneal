# Technology Stack: v1.1 Parser Hardening & UX Polish

**Project:** anneal
**Researched:** 2026-03-29
**Scope:** New dependencies and integration changes for v1.1 milestone only

## New Dependency

### pulldown-cmark 0.13.3

| Field | Value |
|-------|-------|
| Crate | `pulldown-cmark` |
| Version | `0.13.3` (latest stable, released 2025-03-22) |
| Purpose | Markdown body scanner replacing regex-based line-by-line scan |
| MSRV | Rust 1.71.1 (well within anneal's 1.94 toolchain) |
| License | MIT |

**Cargo.toml entry:**

```toml
pulldown-cmark = { version = "0.13", default-features = false }
```

**Why `default-features = false`:** The default features are `["std", "getopts", "html"]`. anneal needs none of these:
- `getopts` -- CLI argument parsing for the pulldown-cmark binary. Not needed.
- `html` -- HTML rendering via `pulldown-cmark-escape`. Not needed (anneal reads markdown, never renders HTML).
- `std` -- Enables `memchr/std` and `pulldown-cmark-escape?/std`. Without `html` enabled, `pulldown-cmark-escape` is not compiled at all. The `memchr/std` activation is marginal -- memchr works fine without it and anneal already links std anyway. Disabling keeps the dep tree minimal.

**Transitive dependencies added (3 total):**
- `bitflags 2` -- already in anneal's dep tree via other crates
- `unicase 2.6` -- new, small (case-insensitive string comparison for CommonMark)
- `memchr 2.5` (no default features) -- already in anneal's dep tree via `regex`

**Net new crates: 1** (`unicase`). Build time impact: negligible (~1-2s incremental).

### Feature Flags to Enable

**Required for v1.1:**

```rust
use pulldown_cmark::{Options, Parser};

let mut options = Options::empty();
options.insert(Options::ENABLE_HEADING_ATTRIBUTES);  // {#id .class} on headings
options.insert(Options::ENABLE_WIKILINKS);            // [[page]] and [[page|display]]
```

| Flag | Why |
|------|-----|
| `ENABLE_HEADING_ATTRIBUTES` | Extracts heading IDs (`{#custom-id}`) for section handle resolution. Without this, heading IDs are not parsed -- only the text. Knowledge corpora commonly use custom heading IDs. |
| `ENABLE_WIKILINKS` | Parses `[[target]]` and `[[target|display]]` as `LinkType::WikiLink` events. The PROJECT.md explicitly lists "native markdown/wiki-links" as a v1.1 target. WikiLinks are common in Obsidian-style knowledge bases. |

**Not needed for v1.1 (do not enable):**

| Flag | Why Not |
|------|---------|
| `ENABLE_TABLES` | Tables don't produce references; no extraction value |
| `ENABLE_FOOTNOTES` | Footnotes are not handle references in anneal's model |
| `ENABLE_TASKLISTS` | Task lists are not convergence-relevant |
| `ENABLE_STRIKETHROUGH` | No semantic value for knowledge graphs |
| `ENABLE_SMART_PUNCTUATION` | Mutates text; harmful for exact label matching |
| `ENABLE_YAML_STYLE_METADATA_BLOCKS` | anneal already hand-rolls frontmatter split (15 lines, battle-tested). Letting pulldown-cmark parse frontmatter would create dual-authority. Keep existing `split_frontmatter()`. |
| `ENABLE_MATH` | Math expressions are not handle references |
| `ENABLE_GFM` | GFM blockquote tags have no extraction value |
| `ENABLE_DEFINITION_LIST` | No handle-relevant semantics |
| `ENABLE_SUPERSCRIPT` / `ENABLE_SUBSCRIPT` | No extraction value |

**Critical: Do NOT enable `ENABLE_YAML_STYLE_METADATA_BLOCKS`.** The existing `split_frontmatter()` strips frontmatter before passing the body to the scanner. If pulldown-cmark also parses frontmatter, it will consume the `---` fences and emit `MetadataBlock` events, creating a conflict with the existing YAML pipeline. The clean boundary is: `split_frontmatter()` owns the frontmatter, pulldown-cmark owns the body.

### Key API Surface for v1.1

**Parser construction:**
```rust
let parser = Parser::new_ext(body, options);  // body = post-frontmatter content
```

**Offset tracking (for line numbers):**
```rust
let offset_iter = parser.into_offset_iter();
for (event, byte_range) in offset_iter {
    // byte_range: Range<usize> into the body string
    // Convert to line number: body[..byte_range.start].lines().count() + 1
    // (or precompute a line-start index for O(log n) lookup)
}
```

**Events relevant to anneal's extraction:**

| Event | Extraction Use |
|-------|---------------|
| `Start(Tag::Heading { level, id, .. })` | Section handle creation (replaces `HEADING_RE`) |
| `Text(cow_str)` inside heading | Heading text for section ID |
| `Start(Tag::Link { link_type, dest_url, .. })` | Markdown links `[text](target.md)`, reference links, wiki links |
| `LinkType::WikiLink` | `[[target]]` extraction -- new capability not possible with regex |
| `LinkType::Autolink` | `<http://...>` -- classify as External handle |
| `Start(Tag::CodeBlock(..))` | Enter code block -- stop extraction (replaces manual ` ``` ` toggle) |
| `End(TagEnd::CodeBlock)` | Exit code block -- resume extraction |
| `Code(text)` | Inline code -- skip extraction (new: regex scanner couldn't distinguish) |
| `Text(cow_str)` outside code/heading | Body text to scan for labels (`OQ-64`), section refs (`S4.1`), file paths |

**What pulldown-cmark handles that regex doesn't:**
1. Proper code block/inline code boundary tracking (no false ` ``` ` toggle on nested fences)
2. Link target extraction from `[text](path.md)` -- currently caught by `FILE_PATH_RE` which also matches prose
3. WikiLink support (`[[page]]`) -- not possible with current regex
4. Heading ID extraction from `{#custom-id}` syntax
5. Correct handling of HTML blocks, blockquotes, nested structures

**What regex still handles (inside `Text` events):**
- Label patterns: `OQ-64`, `KB-F1` (pulldown-cmark doesn't know about anneal labels)
- Section references: `S4.1` (anneal-specific convention)
- Version references: `v17` (anneal-specific convention)

## No New Dependencies Required

### Typed Extraction/Resolution Pipeline

The `DiscoveredRef` + `RefHint` + `Resolution` types replacing `PendingEdge`, `LabelCandidate`, `FrontmatterEdge`, `ScanResult` are pure Rust enums/structs. They need:
- `serde::Serialize` (already in deps) -- for `--json` output
- `camino::Utf8PathBuf` (already in deps) -- for file paths
- Standard library types (`Range<usize>`, `Option`, `Vec`)

No new crate needed. This is a type refactoring within `parse.rs` and `resolve.rs`.

### Content Snippet Extraction for `anneal get`

Content snippets for `GetOutput` require reading file content and extracting a range around a handle. This needs:
- `std::fs::read_to_string` -- already used in `build_graph`
- String slicing by byte range or line range -- standard library
- pulldown-cmark's `OffsetIter` for precise heading-bounded ranges (for section handles)

**Line number index utility:** To convert byte offsets to line numbers efficiently, build a `Vec<usize>` of line-start byte positions once per file. Binary search gives O(log n) byte-to-line conversion. This is ~10 lines of code, no crate needed.

No new crate needed.

### False Positive Suppression via anneal.toml

Add a `suppress` section to `AnnealConfig`:

```toml
[suppress]
# Patterns to never treat as handle references
patterns = ["SHA-\\d+", "AVX-\\d+", "UTF-\\d+"]
# Specific identities to suppress
identities = ["ISO-8601", "RFC-2119"]
```

This requires:
- Adding a `SuppressConfig` struct with `#[serde(default, deny_unknown_fields)]`
- Adding `pub(crate) suppress: SuppressConfig` to `AnnealConfig`
- The `regex` crate (already a dependency) compiles suppression patterns

**Important:** `AnnealConfig` uses `#[serde(deny_unknown_fields)]`. Adding any new field to the struct is a breaking config change in the sense that older versions of anneal would reject configs with the new field. Since anneal is pre-1.0 and configs are project-local, this is acceptable. The field uses `#[serde(default)]` so existing configs without `[suppress]` continue to work.

No new crate needed.

### HandleKind::External for URLs

Adding an `External` variant to `HandleKind` is a pure enum extension. The autolink detection from pulldown-cmark's `LinkType::Autolink` feeds into this. URL validation is simple enough (starts with `http://` or `https://`) that no `url` crate is needed.

No new crate needed.

## Existing Dependencies: Changes

### regex (stays, slightly reduced role)

Current role: `RegexSet` with 5 patterns for single-pass line scanning, plus individual `Regex` for capture extraction.

Post-v1.1 role: `HEADING_RE` and heading `RegexSet` pattern are eliminated (pulldown-cmark handles headings natively). `FILE_PATH_RE` is partially replaced (markdown links extracted by pulldown-cmark, but bare path references in prose text still need regex). `LABEL_RE`, `SECTION_REF_RE`, and version pattern remain (anneal-specific conventions not recognized by any markdown parser).

The `regex` crate stays. The `RegexSet` may shrink from 5 patterns to 3-4, but is not eliminated. resolve.rs still uses `VERSION_FILENAME_RE`.

### console (stays, unchanged)

Used for terminal styling via `Style`. No changes needed for v1.1.

### All other deps (unchanged)

anyhow, camino, chrono, clap, serde, serde_json, serde_yaml_ng, toml, walkdir -- no version bumps or feature changes needed.

## What NOT to Add

| Crate | Why Not |
|-------|---------|
| `url` | URL validation for External handles is a prefix check, not full RFC 3986 parsing. 2 lines vs 200KB crate. |
| `pulldown-cmark-to-cmark` | We don't round-trip markdown. We extract, not transform. |
| `comrak` | Alternative markdown parser. pulldown-cmark is lighter, faster, and CommonMark-native. comrak brings a C dependency (via cmark-gfm bindings in older versions) or a massive pure-Rust impl. |
| `markdown` | Another parser. Less mature, fewer extensions than pulldown-cmark. |
| `tree-sitter-markdown` | Heavyweight, requires tree-sitter runtime. Overkill for extraction. |
| `fuzzy-matcher` / `strsim` | For "did you mean?" in resolution. Levenshtein distance is ~20 lines of code. The resolution cascade (root-prefix, bare filename, version stem, zero-pad) is custom logic, not generic fuzzy matching. |
| `textwrap` | For snippet formatting. `console` already handles terminal width. Wrapping is not needed for snippets (they're source excerpts, not prose). |
| `similar` | For diff/snippet highlighting. anneal already has snapshot diff. Content snippets are plain text excerpts, not diffs. |

## Installation

After v1.1, the dependency line in Cargo.toml:

```toml
[dependencies]
anyhow = "1"
camino = { version = "1", features = ["serde1"] }
chrono = { version = "0.4", default-features = false, features = ["clock", "serde"] }
clap = { version = "4", features = ["derive"] }
console = "0.16"
pulldown-cmark = { version = "0.13", default-features = false }
regex = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml_ng = "0.10"
toml = "0.8"
walkdir = "2"

[dev-dependencies]
tempfile = "3"
```

**Crate count:** 12 production dependencies (was 11). Net new transitive crate: `unicase`. Build time target remains <10s clean.

## Integration Architecture

### How pulldown-cmark fits into the existing pipeline

```
build_graph()
  |
  +--> split_frontmatter(content)          -- unchanged, still hand-rolled
  |     |
  |     +--> parse_frontmatter(yaml)       -- unchanged, serde_yaml_ng
  |     |
  |     +--> body text
  |           |
  |           +--> [NEW] pulldown_cmark::Parser::new_ext(body, options)
  |           |      |
  |           |      +--> OffsetIter yields (Event, Range<usize>)
  |           |             |
  |           |             +--> Heading events --> Section handles (replaces HEADING_RE)
  |           |             +--> Link events --> DiscoveredRef with link metadata
  |           |             +--> WikiLink events --> DiscoveredRef (new capability)
  |           |             +--> CodeBlock events --> toggle extraction suppression
  |           |             +--> Text events --> regex scan for labels, section refs, versions
  |           |
  |           +--> [REDUCED] regex scan on Text event content only
  |                  (not on raw lines -- pulldown-cmark pre-filters)
  |
  +--> resolve_all()                       -- enhanced with Resolution enum
        |
        +--> DiscoveredRef -> Resolution::Exact | Fuzzy | Unresolved
```

### Key integration points

1. **`scan_file()` signature changes.** Currently takes `body: &str` and iterates lines. Will take `body: &str` and iterate pulldown-cmark events. The function returns `ScanResult` today; this becomes `Vec<DiscoveredRef>` in the new pipeline.

2. **Line number computation.** pulldown-cmark gives byte offsets via `OffsetIter`. Build a line-start index (`Vec<usize>`) per file body for O(log n) byte-to-line conversion. This replaces the implicit line tracking in the current line-by-line loop.

3. **Frontmatter boundary.** `split_frontmatter()` returns `(Option<&str>, &str)` where the second element is the body. Pass this body to pulldown-cmark. The byte offsets from `OffsetIter` are relative to the body start, not the file start. To get file-relative line numbers, add the frontmatter line count as an offset.

4. **Code block handling.** Currently tracked via manual ```` ``` ```` toggle (fragile -- doesn't handle `~~~`, indented code, or nested fences). pulldown-cmark handles this correctly: `Start(Tag::CodeBlock(..))` / `End(TagEnd::CodeBlock)` events bracket code content. Text events inside code blocks are `Code(text)` for inline or just don't appear as `Text` events inside code block tags. The scanner simply skips extraction during code block events.

5. **Content snippets for `anneal get`.** When resolving a Section handle, use pulldown-cmark to find the heading's byte range, then extract body content from heading start to next same-or-higher-level heading. This gives a clean "section content" snippet without regex line scanning.

## Version Pinning Strategy

Use `0.13` (caret range) not `0.13.3` (exact). pulldown-cmark follows semver within 0.13.x -- patch releases are bug fixes (0.13.3 fixed wikilink offsets). The `0.13` range allows automatic patch updates via `cargo update` while preventing breaking 0.14 changes.

The `ENABLE_WIKILINKS` option was introduced in 0.13.0, and 0.13.3 fixed wikilink byte offset calculation in `OffsetIter`. Pin to `0.13` minimum, which cargo resolves to 0.13.3 today.

## Confidence Assessment

| Claim | Confidence | Source |
|-------|------------|--------|
| pulldown-cmark 0.13.3 is latest stable | HIGH | crates.io search, GitHub releases page |
| `default-features = false` gives core parser | HIGH | Cargo.toml features section on GitHub, verified `html`/`getopts` are opt-in |
| `ENABLE_WIKILINKS` produces `LinkType::WikiLink` | HIGH | docs.rs LinkType enum documentation |
| `OffsetIter` gives byte ranges per event | HIGH | docs.rs OffsetIter documentation |
| `ENABLE_YAML_STYLE_METADATA_BLOCKS` conflicts with `split_frontmatter` | HIGH | Logical analysis: both would consume `---` fences |
| Build time impact is negligible | MEDIUM | unicase is small, bitflags/memchr already in tree; no benchmark data |
| No other new deps needed for v1.1 features | HIGH | Code analysis of existing codebase + feature requirements |
| `ENABLE_HEADING_ATTRIBUTES` extracts `{#id}` | HIGH | docs.rs Tag::Heading documentation |

## Sources

- [pulldown-cmark docs.rs (Event enum)](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/enum.Event.html)
- [pulldown-cmark docs.rs (Tag enum)](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/enum.Tag.html)
- [pulldown-cmark docs.rs (Options)](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.Options.html)
- [pulldown-cmark docs.rs (LinkType)](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/enum.LinkType.html)
- [pulldown-cmark docs.rs (OffsetIter)](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.OffsetIter.html)
- [pulldown-cmark GitHub Cargo.toml](https://github.com/pulldown-cmark/pulldown-cmark/blob/master/pulldown-cmark/Cargo.toml)
- [pulldown-cmark GitHub releases](https://github.com/pulldown-cmark/pulldown-cmark/releases)
- [pulldown-cmark crates.io](https://crates.io/crates/pulldown-cmark)
