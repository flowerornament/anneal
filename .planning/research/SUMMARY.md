# Project Research Summary

**Project:** anneal v1.1 — Parser Hardening & UX Polish
**Domain:** Markdown knowledge graph convergence tool — internal pipeline refactoring
**Researched:** 2026-03-29
**Confidence:** HIGH

## Executive Summary

The v1.1 milestone replaces anneal's five-regex body scanner with pulldown-cmark 0.13 and introduces three typed intermediate representations (extraction, resolution, diagnostics). Research across stack, features, architecture, and pitfalls converges on a single conclusion: this is a well-understood pattern. Both lychee and mdbook-linkcheck (Rust markdown tools with pulldown-cmark extraction pipelines) use the exact architecture being proposed — a pure extraction function producing owned data structures, separate from graph construction and validation. The key architectural move is decomposing the current `build_graph()` monolith into `extract_file()` (pure, per-file) and `build_graph_from_extractions()` (graph assembly), connected by the `FileExtraction` type.

The recommended approach is incremental migration in five phases, each keeping `just check` green: Types -> Adapter -> cmark swap -> Resolution -> Diagnostics. One new dependency is needed: `pulldown-cmark = { version = "0.13", default-features = false }` with `ENABLE_HEADING_ATTRIBUTES` and `ENABLE_WIKILINKS`. This adds one net-new transitive crate (`unicase`). The `regex` crate stays but shrinks from 5 patterns to 3 (domain-specific label, section, and version patterns applied only to `Text` event content). No other new dependencies are required for the full v1.1 scope.

The top risks are: (1) frontmatter byte offset misalignment causing every line number to be wrong, mitigated by building a `SourceMap` from full file content and adjusting body offsets by frontmatter size; (2) pulldown-cmark text event fragmentation breaking regex matching, mitigated by mandatory concatenation of `Text` events within block elements before pattern matching; (3) JSON schema breakage in `--json` output, mitigated by additive-only field additions. A fourth risk — changing `--active-only` to default — is avoided entirely: keep the current default and add a config opt-in instead.

## Key Findings

### Recommended Stack

One new production dependency. No version bumps to existing crates.

**Core technologies:**
- **pulldown-cmark 0.13** (`default-features = false`): Markdown event parser replacing regex body scanner — yields `(Event, Range<usize>)` via `into_offset_iter()` for structural extraction with source spans. Adds 1 net-new transitive crate (`unicase`).
- **regex** (stays, reduced role): 5-pattern `RegexSet` shrinks to 3 patterns (labels, section refs, version refs). Applied only to `Text` event content, not raw lines. Headings and file paths now handled by pulldown-cmark events.
- **All other deps unchanged**: anyhow, camino, chrono, clap, console, serde, serde_json, serde_yaml_ng, toml, walkdir. No version bumps or feature changes.

**Critical version/feature requirements:**
- `ENABLE_HEADING_ATTRIBUTES`: extracts `{#custom-id}` on headings for section handle resolution
- `ENABLE_WIKILINKS`: parses `[[target]]` and `[[target|display]]` as `LinkType::WikiLink` events
- Do NOT enable `ENABLE_YAML_STYLE_METADATA_BLOCKS`: conflicts with existing `split_frontmatter()` which owns the frontmatter boundary. Two owners of the `---` delimiter = bugs.
- Do NOT enable `ENABLE_SMART_PUNCTUATION`: mutates text, harmful for exact label matching

**What NOT to add:** `strsim`/`fuzzy-matcher` (deterministic transforms cover the cases), `url` (prefix check is 2 lines), `codespan-reporting`/`miette` (anneal diagnostics are single-location, hand-roll ~25 lines), `comrak` (heavier, pulldown-cmark is lighter for extraction-only use).

### Expected Features

**Must have (table stakes):**
- pulldown-cmark body scanner replacing `RegexSet` line-by-line scan
- Byte offset tracking per discovered reference via `into_offset_iter()`
- Line number population in `Diagnostic` struct (currently always `None`)
- Structural code block/inline code skipping (replaces fragile `in_code_block` toggle)
- Deterministic resolution cascade: root-prefix, bare filename, version stem, zero-pad
- Config-based false positive suppression in `anneal.toml` (`ignore_refs`, `ignore_files`, `suppress`)
- Heading slug extraction from AST (replaces `HEADING_RE`)

**Should have (differentiators):**
- Native `[[WikiLink]]` parsing via `ENABLE_WIKILINKS`
- "Similar handle exists" message on E001 broken-reference errors
- Content preview in `anneal get` (first 5 body lines for File handles, heading + first paragraph for Section handles)
- Source line content display in `anneal check` diagnostics (compiler-style format)

**Defer (v2+):**
- Inline `<!-- anneal:ignore -->` comment suppression (medium complexity, needs stable body scanner first)
- `strsim` fuzzy matching fallback (evaluate after deterministic cascade ships)
- LSP/Language Server integration
- Auto-fix mode (violates KB-P1: files are truth, anneal reads only)
- Full codespan-reporting / miette integration

### Architecture Approach

The new architecture introduces `extract.rs` as a module containing the pure extraction function and all typed intermediaries. `extract_file()` takes file content and path, returns `FileExtraction` (owned data, no graph mutation, no `&mut DiGraph`). Graph assembly consumes `Vec<FileExtraction>` into the existing `BuildResult` shape. Resolution produces a `Resolution` enum (Exact/Fuzzy/Unresolved) via match arms on a `RefHint` discriminant. No trait objects — the variant set is closed and the codebase already decided against trait objects for CLI output.

**Major components:**
1. **`extract.rs`** (new) — `FileExtraction`, `DiscoveredRef`, `RefHint`, `SourceSpan`, `LineIndex`, `extract_file()`. Pure extraction, no graph state.
2. **`parse.rs`** (modified) — `build_graph()` calls `extract_file()` internally. `ScanResult`, `LabelCandidate` removed after migration. Walk logic stays.
3. **`resolve.rs`** (modified) — `resolve_all()` returns per-ref `Resolution` values. `PendingEdge` replaced by `DiscoveredRef`. Four deterministic fuzzy strategies in `FuzzyStrategy` enum.
4. **`checks.rs`** (modified) — `Diagnostic` gains optional `evidence` field. `check_existence()` populates candidates from `Resolution::Unresolved`.
5. **Unchanged modules** — `graph.rs`, `handle.rs`, `lattice.rs`, `impact.rs`, `snapshot.rs`, `config.rs` are stable.

**Key architectural decisions:**
- `SourceMap` built from full file content, body byte offsets adjusted by frontmatter size before line lookup
- Text events concatenated within block elements before regex matching (mandatory, not optional)
- Resolution cascade: 3 variants (Exact/Fuzzy/Unresolved), no confidence scores — it either matched or it didn't
- "Did you mean?" uses deterministic structural transforms (root-prefix, bare filename, version stem, zero-pad), not fuzzy string matching
- JSON schema changes additive-only: new nullable fields, existing fields never change type or disappear

### Critical Pitfalls

1. **Frontmatter double-parsing (P1)** — Do NOT enable `ENABLE_YAML_STYLE_METADATA_BLOCKS`. Keep existing `split_frontmatter()`. Feed only body to pulldown-cmark. This is the foundational decision; wrong choice cascades into every line-number computation.

2. **Byte offset misalignment (P2)** — pulldown-cmark offsets are relative to body string, not full file. Build `SourceMap` from full file content with `line_starts: Vec<usize>` and `body_byte_offset: usize`. Add frontmatter size to all pulldown-cmark offsets before line lookup. Encapsulate in `SourceMap`, never compute ad-hoc.

3. **Text event fragmentation (P3)** — pulldown-cmark splits text at inline markup boundaries and softbreaks. Concatenate all `Text` events within the same block element before regex matching. Do NOT match patterns on individual `Text` events. This is mandatory for edge kind inference (which checks keyword + reference co-occurrence).

4. **JSON schema breakage (P4)** — `--json` output is anneal's machine API. Additive-only changes: new fields may be added, existing fields must not change type or be removed. Keep `"line": <u32 | null>` as-is. Add new `"span"` and `"evidence"` fields as nullable. Use `#[serde(skip_serializing_if = "Option::is_none")]`.

5. **Active-only default change (P10)** — Do NOT change the `--active-only` default. Breaks CI scripts (186 errors -> 9 on Murail), poisons `history.jsonl` snapshots with a discontinuity, makes `anneal diff` report false improvement. Add config opt-in `[check] default_filter = "active-only"` instead.

## Implications for Roadmap

Based on combined research findings, the five-phase migration structure is strongly supported by all four research files. Dependencies flow strictly downward.

### Phase 1: Types Foundation
**Rationale:** All downstream code needs to import new types. Defining types first with no behavior change proves they compile and lets other phases reference them immediately.
**Delivers:** `extract.rs` module with `SourceSpan`, `LineIndex`, `DiscoveredRef`, `RefHint`, `DiscoveredSection`, `FileExtraction`, `Resolution`, `FuzzyStrategy` types. All defined but unused.
**Addresses:** Type unification (4 types -> 1 `DiscoveredRef` with `RefHint` discriminant)
**Avoids:** P11 (over-engineering Resolution — keep exactly 3 variants, no confidence scores), P12 (god struct — zero `Option` fields on `DiscoveredRef`)
**Needs phase research:** No. Standard Rust type design.

### Phase 2: Extraction Adapter
**Rationale:** Proves the `extract_file()` -> `FileExtraction` boundary works before changing any extraction logic. If the adapter works, the pulldown-cmark swap is a contained change behind a stable interface.
**Delivers:** `extract_file()` calling existing `scan_file()` + frontmatter parsing internally. `build_graph()` calls `extract_file()`. All existing tests pass.
**Addresses:** Testable extraction (pure function, no `&mut DiGraph`), future trait boundary for KB-OQ5
**Avoids:** P2 (build `SourceMap` from full file content here, before pulldown-cmark needs it)
**Needs phase research:** No. Adapter is mechanical.

### Phase 3: pulldown-cmark Swap
**Rationale:** The foundational behavior change. Extraction must produce `DiscoveredRef` before resolution can consume it. WikiLinks, structural code blocks, line numbers, and inline code skip all land here.
**Delivers:** pulldown-cmark event iteration inside `extract_file()`. Same `FileExtraction` output type. `scan_file()` removed. `RegexSet` reduced from 5 to 3 patterns. Dependency added.
**Addresses:** WikiLinks (FEATURES Cap 1), structural code blocks (FEATURES Cap 1), line number tracking (FEATURES Cap 5), heading attribute extraction
**Avoids:** P1 (no `ENABLE_YAML_STYLE_METADATA_BLOCKS`), P3 (text concatenation buffer mandatory), P5 (scan HTML events for references), P6 (document indented code block behavior change), P7 (percent-decode wikilink destinations), P13 (extract `heading_to_slug()` before swapping, compare outputs)
**Needs phase research:** YES. Needs regression testing against Murail (262 files) and Herald corpora. Diagnostic count changes expected but must be characterized. Parallel-run old vs new scanner during development.

### Phase 4: Resolution Enrichment
**Rationale:** With `DiscoveredRef` + `RefHint` flowing from extraction, resolution can now classify outcomes as Exact/Fuzzy/Unresolved and collect candidates. This must precede diagnostics because `Evidence` needs candidate data.
**Delivers:** `resolve_all()` returns per-ref `Resolution` values. `PendingEdge` and `LabelCandidate` removed. Four deterministic fuzzy strategies (root-prefix, bare filename, version stem, zero-pad). `ResolveStats` enriched with fuzzy match counts.
**Addresses:** "Did you mean?" (FEATURES Cap 2), resolution cascade, type cleanup
**Avoids:** P9 (structural transforms only, not edit distance; cap 3 candidates; neutral language "similar handles:" not "did you mean?"), P11 (3 variants only, no confidence scores)
**Needs phase research:** No. The four transform strategies are well-defined and deterministic.

### Phase 5: Diagnostic Enrichment + Config Suppression
**Rationale:** Last phase because it depends on both line numbers (Phase 3) and resolution candidates (Phase 4). Config suppression is independent and can be developed in parallel with earlier phases but ships here for coherent release.
**Delivers:** `Evidence` enum on `Diagnostic`. Line numbers populated (currently `None`). Source line in human output. Content preview in `get`. Config-based suppression (`ignore_refs`, `ignore_files`, `suppress` in `anneal.toml`). `--verbose` flag for enriched diagnostic output.
**Addresses:** Line numbers in diagnostics (FEATURES Cap 5), content preview (FEATURES Cap 3), false positive suppression (FEATURES Cap 4), "similar handle exists" messages
**Avoids:** P4 (additive-only JSON changes, new nullable fields), P10 (no default change for `--active-only`, config opt-in only), P16 (verbose output behind `--verbose` flag, default stays terse), P18 (`#[serde(default)]` on all new config fields)
**Needs phase research:** No. Standard diagnostic rendering patterns.

### Phase Ordering Rationale

- **Types before behavior:** All downstream code can import new types without any behavior change. Compiler catches mismatches early.
- **Adapter before swap:** Proves the `FileExtraction` boundary works before touching extraction internals. If Phase 2 works, Phase 3 is a contained change.
- **pulldown-cmark before resolution:** Extraction must produce `DiscoveredRef` with `RefHint` classification before resolution can match on `RefHint` variants.
- **Resolution before diagnostics:** `Evidence` on `Diagnostic` needs `Resolution::Unresolved { candidates }` data. Can't display "similar handles" without resolution cascade.
- **Config suppression ships with diagnostics:** Independent of pulldown-cmark but same release for coherent UX.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 3 (pulldown-cmark swap):** Needs regression baseline from both corpora. Diagnostic count changes must be characterized before shipping. HTML block and indented code block behavior changes need explicit decisions. Wikilink percent-encoding needs decoding utility.

Phases with standard patterns (skip research-phase):
- **Phase 1 (types):** Pure Rust type design, no external dependencies.
- **Phase 2 (adapter):** Mechanical adapter wrapping existing code.
- **Phase 4 (resolution):** Four deterministic string transforms, closed-set enum dispatch.
- **Phase 5 (diagnostics + config):** Standard diagnostic formatting and TOML config extension.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | pulldown-cmark API verified via docs.rs, version confirmed on crates.io, patterns verified in lychee/mdbook-linkcheck source code |
| Features | HIGH | Tool landscape (lychee, mdbook-linkcheck, vale, clippy) surveyed with source verification. Table stakes vs differentiators clearly delineated. |
| Architecture | HIGH | Pure extraction function pattern used by both lychee and mdbook-linkcheck. `SourceMap`/`LineIndex` is standard (rustc, miette, codespan all use it). Resolution enum dispatch has codebase precedent. |
| Pitfalls | HIGH | 18 pitfalls identified from codebase analysis, pulldown-cmark specs, CommonMark spec, and live JSON output inspection. Critical pitfalls (P1-P5) have concrete prevention strategies and test cases. |

**Overall confidence:** HIGH

### Gaps to Address

- **Regression characterization:** Before Phase 3, need baseline diagnostic counts from Murail and Herald with the old scanner. Compare after migration. This is implementation work, not research.
- **WikiLink resolution semantics:** How should `[[page]]` resolve when "page" matches both a file and a label? Tentative: file-first (wikilinks are typically file references in Obsidian-style corpora). Validate against real wiki-style corpora during Phase 3.
- **HTML block scanning decision:** Should anneal scan references inside HTML blocks/comments? Research says yes (anneal's purpose is finding ALL references). Needs explicit design decision during Phase 3.
- **HandleKind::External interaction with checks:** PROJECT.md mentions external URL handles. Architecture supports classification via `RefHint`, but should external URLs get E001 broken-reference errors? Needs design during Phase 4 or 5.
- **Evidence enum exact variants:** Depends on what data each check rule produces. Will emerge during Phase 5 implementation, not a research gap.

## Sources

### Primary (HIGH confidence)
- [pulldown-cmark docs.rs (Event, Tag, Options, OffsetIter, LinkType)](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/) -- full API surface for v0.13.3
- [pulldown-cmark wikilinks spec](https://pulldown-cmark.github.io/pulldown-cmark/specs/wikilinks.html) -- edge cases, precedence, pipe handling
- [pulldown-cmark metadata blocks spec](https://pulldown-cmark.github.io/pulldown-cmark/specs/metadata_blocks.html) -- YAML delimiter rules (why NOT to enable)
- [CommonMark specification](https://spec.commonmark.org/0.12/) -- HTML blocks, indented code blocks, fenced code blocks
- [Rust Compiler Dev Guide: Diagnostics](https://rustc-dev-guide.rust-lang.org/diagnostics.html) -- suggestion wording style
- [Clippy Configuration](https://doc.rust-lang.org/clippy/configuration.html) -- allow/deny/suppress patterns

### Secondary (MEDIUM confidence)
- [lychee source code](https://github.com/lycheeverse/lychee) -- pulldown-cmark extraction patterns, `SourceSpanProvider`, suppression mechanisms
- [mdbook-linkcheck architecture](https://adventures.michaelfbryan.com/posts/linkchecker) -- extraction/validation separation, codespan integration
- [Vale documentation](https://vale.sh) -- inline suppression `<!-- vale off/on -->` pattern
- [strsim crate docs](https://docs.rs/strsim/latest/strsim/) -- string similarity algorithms (deferred, not needed for v1.1)
- anneal source: `parse.rs`, `checks.rs`, `cli.rs`, `resolve.rs` -- direct codebase analysis
- Live `anneal check --json` output from Murail corpus -- confirmed exact JSON field names and types

### Tertiary (LOW confidence)
- [lychee DeepWiki](https://deepwiki.com/lycheeverse/lychee) -- architecture overview (single community source)
- Build time estimates for pulldown-cmark (~2-3s compile) -- estimated from crate size, no benchmark data

---
*Research completed: 2026-03-29*
*Ready for roadmap: yes*
