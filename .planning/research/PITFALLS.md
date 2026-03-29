# Domain Pitfalls: Parser Hardening & UX Polish

**Domain:** Adding pulldown-cmark parser, typed extraction/resolution pipeline, and UX enrichments to existing Rust CLI convergence tool (anneal v1.1)
**Researched:** 2026-03-29
**Confidence:** HIGH (grounded in codebase analysis of parse.rs/checks.rs/cli.rs/resolve.rs, pulldown-cmark official specs, CommonMark spec, live JSON output inspection)

---

## Critical Pitfalls

Mistakes that cause regressions, rewrite-worthy integration bugs, or break existing users.

### Pitfall 1: Frontmatter Double-Parsing Conflict

**What goes wrong:** pulldown-cmark's `ENABLE_YAML_STYLE_METADATA_BLOCKS` flag treats `---` delimiters as metadata block markers and *removes them from the event stream*. The existing `split_frontmatter()` (parse.rs:82) already strips frontmatter before body scanning. If you enable the metadata blocks flag AND keep `split_frontmatter()`, one of two failures occurs: (a) pulldown-cmark never sees the frontmatter because it was pre-stripped, so the `---` at the body start becomes a thematic break or gets misinterpreted, or (b) you feed the full file to pulldown-cmark and it consumes the frontmatter, but then you also try to extract it separately, causing the YAML to be parsed twice with different boundaries.

**Why it happens:** The current architecture cleanly separates frontmatter extraction from body scanning (parse.rs:564: `split_frontmatter(&content)` then `scan_file(body, ...)`). pulldown-cmark's metadata blocks feature wants to own that separation itself. Two owners of the same boundary = bugs. Furthermore, pulldown-cmark's metadata block spec requires the block to be *explicitly closed* (EOF is not valid) and disallows blank lines after the opening delimiter -- constraints stricter than anneal's current `split_frontmatter()` which handles `---` at EOF.

**Consequences:**
- Frontmatter YAML parsed with different boundary logic produces different status values
- Line numbers from pulldown-cmark's `into_offset_iter()` are offset by the frontmatter length if you pre-strip but don't adjust
- A `---` thematic break inside the document body could be misclassified as a second metadata block ending
- pulldown-cmark's metadata block rules reject some frontmatter that `split_frontmatter()` currently accepts (e.g., trailing `---` at EOF without final newline)

**Prevention:**
- **Do NOT enable `ENABLE_YAML_STYLE_METADATA_BLOCKS`.** Keep the existing `split_frontmatter()` which is battle-tested (handles `\r\n`, handles `---` at EOF). Feed only the body portion to pulldown-cmark. This is the cleanest boundary: anneal owns frontmatter, pulldown-cmark owns markdown structure.
- If you later want pulldown-cmark to handle frontmatter, it must be an all-or-nothing swap with `split_frontmatter()` removed entirely, and you must audit every corpus file against pulldown-cmark's stricter rules.
- When computing line numbers from pulldown-cmark byte offsets, add the frontmatter offset (see Pitfall 2).

**Detection:** Regression test: file with `---` thematic break after frontmatter. File with `---` at EOF without trailing newline. File with blank line inside frontmatter.

**Phase:** Must be decided at parser introduction. Wrong choice here cascades into every line-number computation.

---

### Pitfall 2: Byte Offset to Line Number Misalignment

**What goes wrong:** pulldown-cmark's `into_offset_iter()` returns byte ranges relative to the input string. If you feed it only the body (after `split_frontmatter()`), the byte offsets are relative to the body start. Converting these to line numbers requires knowing the frontmatter's line count and byte size. The current scanner has `line: None` on almost all diagnostics (confirmed from live `--json` output). When v1.1 adds line numbers, this computation becomes critical, and there are two independent failure modes:

1. **Off-by-frontmatter:** Body byte offset 0 corresponds to a different file line depending on frontmatter length.
2. **Multi-byte UTF-8:** Byte offsets are not character offsets. `OQ-64` on a line after a line containing `" "` (3-byte UTF-8) will have a byte offset 2 larger than a character-counting approach would predict.

**Why it happens:** The body is a `&str` slice of the original content. `content.len() - body.len()` gives the byte offset where the body starts, but converting byte offset to line requires a precomputed index.

**Consequences:** Every `"line"` value in every diagnostic is wrong. `error[E001]: broken reference ... -> file.md:3` points to the frontmatter instead of the reference. Users who click line numbers in editors go to the wrong place.

**Prevention:**
- Build a `SourceMap` struct from the *full file content* before splitting frontmatter:
  ```
  struct SourceMap {
      line_starts: Vec<usize>,  // byte offsets of each line start
      body_byte_offset: usize,  // content.len() - body.len()
  }
  ```
- Use `partition_point()` (binary search) on `line_starts` for O(log n) byte-to-line lookup. Name it `byte_offset_to_line()` to make semantics explicit.
- When pulldown-cmark reports byte range `R` relative to body, the absolute byte range is `R.start + body_byte_offset .. R.end + body_byte_offset`.
- Look up the absolute byte offset in the full-file line table to get the correct 1-indexed line number.
- Encapsulate this in `SourceMap`. Never compute line numbers ad-hoc.

**Detection:** Test with a file that has 10 lines of frontmatter including multi-byte UTF-8 values. Assert that a reference on body line 1 reports as line 13 (10 frontmatter + 2 delimiters + 1). Test with a file containing emoji or accented characters in preceding lines.

**Phase:** Parser replacement phase. The `SourceMap` must be built alongside the pulldown-cmark integration, not retrofitted.

---

### Pitfall 3: pulldown-cmark Text Event Fragmentation

**What goes wrong:** pulldown-cmark splits text content into multiple `Text` events at inline markup boundaries. The string `"See OQ-64 for details"` might be emitted as `Text("See ")`, `Text("OQ-64")`, `Text(" for details")` if inline markup is nearby. More critically, softbreaks (`\n` within a paragraph) are separate events that split surrounding text. The current regex scanner processes whole lines (parse.rs:321: `for line in body.lines()`). Edge kind inference (`infer_edge_kind_from_line` at parse.rs:279) checks if a keyword and a reference appear on the *same line*. With pulldown-cmark, there are no "lines" -- only events.

**Why it happens:** pulldown-cmark is a pull parser that yields events at structural boundaries. Text between inline elements becomes separate `Text` events.

**Consequences:**
- Labels are usually single tokens and won't be split, but edge kind inference (which checks same-line co-occurrence of keywords like "incorporates", "builds on") will fail because the keyword and the reference are in different text events.
- File path references that contain `/` characters may be split at boundaries.
- The 5-pattern RegexSet approach (match the whole line first, then extract) cannot be replicated event-by-event.

**Prevention:**
- Buffer all `Text` events within the same block element (paragraph, heading, list item) and concatenate them before applying regex patterns. This is the standard pulldown-cmark approach.
- Track event nesting: collect text between `Start(Paragraph)` and `End(Paragraph)`, between `Start(Heading)` and `End(Heading)`, etc.
- The byte ranges from `into_offset_iter()` still map to individual events, so build a secondary mapping: for each character position in the concatenated buffer, record which original byte offset in the source it came from. This lets you find source locations for matches in the concatenated text.
- Do NOT try to match patterns on individual `Text` events. Always concatenate first, then match, then map back.

**Detection:** Test with `"builds on OQ-64"` where a softbreak or emphasis boundary splits the text. Test with `**OQ-64** is important` and `See OQ-64 and *also* OQ-65`.

**Phase:** Parser replacement phase. The concatenation strategy must be the first thing built before any pattern matching.

---

### Pitfall 4: Breaking --json Consumers When Enriching Diagnostics

**What goes wrong:** The current `Diagnostic` struct (checks.rs:25) serializes to exactly these fields (confirmed by live run):
```json
{
  "severity": "Error",
  "code": "E001",
  "message": "broken reference: OQ-99 not found",
  "file": "formal-model/v17.md",
  "line": null
}
```
And `CheckOutput` wraps as: `{ "diagnostics": [...], "errors": 186, "warnings": 290, "info": 1, "suggestions": 38, "terminal_errors": 177 }`.

Adding new fields (evidence, candidates, snippets, source_location) changes the JSON schema. Any downstream consumer using `deny_unknown_fields` or strict schema validation will break. Changing the *type* of existing fields (e.g., `line: u32` becoming `line: {start: u32, end: u32}`) will definitely break deserialization.

**Why it happens:** anneal's `--json` output is its machine API. The output shape visible in the live run is the contract even though there's no formal schema document.

**Consequences:**
- Scripts that `jq '.errors'` or `jq '.diagnostics[] | select(.code == "E001") | .message'` work fine with new fields (jq ignores unknowns). But scripts that deserialize into a Rust/Python struct with exact fields will fail.
- If `line` changes from `Option<u32>` to a struct, all deserialization breaks.
- The `terminal_errors` field in `CheckOutput` is already in the contract -- removing it is a breaking change.

**Prevention:**
- **Additive-only changes to JSON output.** New fields may be added. Existing fields must not change type or be removed.
- Keep `"line": <u32 | null>` as-is. Add a *new* field `"span"` for richer source info: `{"start_line": 5, "end_line": 5, "start_col": 12, "end_col": 17}`. Old consumers ignore `span`; new consumers use it.
- Add `"evidence"` as a new nullable field on `Diagnostic`: `"evidence": null` for existing diagnostics, structured data for enriched ones.
- Write a JSON snapshot test: serialize current output from both test corpora, assert that existing fields still parse identically after changes.
- Use `#[serde(skip_serializing_if = "Option::is_none")]` on new optional fields to keep output clean for diagnostics that don't have evidence.

**Detection:** JSON snapshot test comparing output before and after changes. Run stored `jq` patterns against both versions.

**Phase:** Diagnostic enrichment phase. Must be designed before any Diagnostic struct changes are coded.

---

### Pitfall 5: HTML Blocks Swallowing Markdown Content

**What goes wrong:** Per CommonMark spec, an HTML block (starting with `<div>`, `<table>`, `<!-- comment -->`, etc.) consumes everything until a blank line. pulldown-cmark emits this as `Html` events where the content is raw text, not parsed as markdown. The current regex scanner treats every line uniformly (parse.rs:321) -- it doesn't know about HTML blocks. Any references inside HTML blocks will be invisible to pulldown-cmark's normal event stream.

**Consequences:**
- References inside `<!-- TODO: see OQ-64 -->` comments vanish
- References inside HTML tables vanish
- References inside `<details>` blocks (used in GitHub-flavored documentation) vanish
- This is spec-correct but defeats anneal's purpose (finding ALL references)

**Prevention:**
- During pulldown-cmark event processing, also scan `Html` event text with the regex patterns for labels, file paths, and section refs. This is the pragmatic choice: anneal's job is reference extraction, not spec-pure markdown rendering.
- Tag these with a distinct extraction source (e.g., `RefHint::HtmlBlock`) so they can be filtered if needed.
- Alternatively, accept the behavior change and document it. But given anneal's purpose, scanning HTML text is better.
- Test with `<!-- OQ-64 -->`, `<table><td>OQ-64</td></table>`, and `<details><summary>OQ-64</summary></details>`.

**Detection:** Search test corpora for HTML constructs. If any exist, this pitfall is active. The Murail corpus likely has HTML comments; the Herald corpus may have `<details>` blocks.

**Phase:** Parser replacement phase. Decision must be made before the scanner is complete.

---

## Moderate Pitfalls

### Pitfall 6: Indented Code Blocks Changing Reference Coverage

**What goes wrong:** CommonMark treats any line indented 4+ spaces as an indented code block. The current regex scanner (parse.rs:319) only tracks fenced code blocks (triple-backtick toggle). pulldown-cmark correctly identifies indented code blocks, which means references inside them will now be skipped. But some content that *looks* indented (e.g., continuation lines in lists, content inside blockquotes) will be classified differently.

**Consequences:**
- References previously scanned (inside 4-space-indented blocks) will be silently dropped, causing new unresolved-reference errors or disappearing true positives
- Test corpus results will shift in both directions
- The change is correct (code examples shouldn't produce references) but surprising if not tracked

**Prevention:**
- Run both scanners in parallel during development and diff their outputs on the Murail and Herald corpora.
- Explicitly decide: should anneal scan inside indented code blocks? For code examples, no. For formatting indentation, the behavior change may lose valid references.
- Keep a comparison test that asserts the new scanner finds >= N% of what the old scanner found (regression floor).

**Detection:** Compare error counts before/after migration on both test corpora. A sudden drop in resolved references indicates indented code block reclassification.

**Phase:** Parser replacement phase. Must have parallel-run comparison before removing the regex scanner.

---

### Pitfall 7: Wikilink Destination Encoding Surprises

**What goes wrong:** pulldown-cmark's `ENABLE_WIKILINKS` emits `WikiLink` events where `dest_url` is percent-encoded. `[[formal model]]` produces destination `formal%20model`, not `formal model`. If anneal tries to resolve `formal%20model` as a handle identity, it will never match the file `formal model.md`.

**Why it happens:** pulldown-cmark follows URL encoding conventions for link destinations. The spec confirms wikilink content is "taken as-is" and then URL-encoded.

**Additional edge cases:**
- The pipe character cannot be escaped in wikilinks: `[[first\|second]]` splits on `|`, making `first\` the URL (with backslash as literal URL character) and `second` the display text.
- Empty wikilinks `[[]]` render as literal text, not links.
- Nested wikilinks `[[outer|[[inner]]]]` -- the deepest one takes precedence.

**Prevention:**
- Percent-decode wikilink destinations before handle resolution. A simple `url_decode()` function handling `%20`, `%23`, etc. is sufficient -- no need for a full URL parsing crate.
- Test with wikilinks containing spaces, hash signs, and pipe characters.
- Document the pipe limitation: `[[page|display]]` works but `|` cannot appear in the destination.

**Phase:** Parser replacement phase, specifically the wikilink event handler.

---

### Pitfall 8: Wikilink Precedence Over Reference Links

**What goes wrong:** When `ENABLE_WIKILINKS` is active, wikilinks take precedence over standard markdown reference links and inline links. `[[OQ-64]]` becomes a wikilink even if `[OQ-64]: #oq-64` is defined as a reference link elsewhere in the document. The current regex scanner treats `[[OQ-64]]` as containing the label `OQ-64` (via `[A-Z][A-Z_]*-\d+`). With pulldown-cmark + wikilinks, it becomes a `WikiLink` event with destination `OQ-64`, and the `Text` event for the label disappears.

**Prevention:**
- Handle `WikiLink` events as an extraction source alongside `Text` events. Extract the destination as a potential handle identity.
- This is actually an improvement: wikilinks give the exact target with no regex ambiguity. But you must wire `WikiLink` events into the extraction pipeline or references will be lost.
- Also handle `Link` events: extract destination as file/URL reference, process text content normally.
- Test that `[[OQ-64]]` still resolves to the OQ-64 label handle and `[[guide.md]]` resolves to the file handle.

**Phase:** Parser replacement phase. Wire wikilink handling early to avoid a gap where wikilink references are silently dropped.

---

### Pitfall 9: "Did You Mean?" Suggestions Creating Confusion

**What goes wrong:** The resolution cascade with "did you mean?" candidates can produce misleading suggestions. `OQ-99` is unresolved; fuzzy match suggests `OQ-9` and `OQ-90`. The user sees "did you mean OQ-9?" but they meant OQ-99 which simply doesn't exist yet. The suggestion implies a typo where there is none.

**Why it happens:** Edit-distance fuzzy matching doesn't understand handle semantics. `OQ-99` and `OQ-9` are edit-distance 1 apart but semantically unrelated. Similarly, `guide.md` might fuzzy-match to `guides.md` (helpful) or `guild.md` (confusing).

**Consequences:**
- Users waste time investigating false suggestions
- In --json output, candidates arrays create noise for automated processing
- For large corpora, every unresolved reference generates N candidates, making output verbose

**Prevention:**
- **Limit fuzzy matching to specific structural transforms, not general edit distance.** The planned cascade (root-prefix, bare filename, version stem, zero-pad) is already better than generic Levenshtein. Stick with those deterministic strategies.
- Zero-pad matching: `OQ-1` matches `OQ-01` (good). Version stem: `guide-v3.md` matches `guide-v3` (good). Root prefix: `foo/bar.md` matches `bar.md` (good). These are all structural, not probabilistic.
- If using strsim/Jaro-Winkler as a fallback, set threshold at 0.85+ for short identifiers. Only suggest if there is exactly one candidate above threshold OR the top candidate is significantly better than the second.
- Cap candidates at 3 per unresolved reference.
- In human output, use neutral language: "similar handles: OQ-9, OQ-90" not "did you mean OQ-9?".
- In --json output, candidates go in the `evidence` field (see Pitfall 4), not in the `message` string.

**Detection:** Run on Herald corpus (89 errors -> 50 with --active-only). Check that suggestions are actionable, not noise.

**Phase:** Resolution cascade phase. Design candidate strategies before implementing the UI.

---

### Pitfall 10: Active-Only Default Breaking CI Scripts and Snapshots

**What goes wrong:** v1.1 wants `--active-only` to become the default behavior for `anneal check`. Currently, the default shows ALL diagnostics (confirmed live: 186 errors on Murail with 177 from terminal files). Changing the default means scripts that parse error counts see a dramatic drop (186 -> 9 on Murail). A CI pipeline asserting `errors == 0` would suddenly pass (false green). More insidiously, convergence tracking via `history.jsonl` snapshots sees a discontinuity -- the snapshot error count drops, making `anneal status` report the corpus is "advancing" massively when it's just a filter change.

**Consequences:**
- CI scripts get false green
- `anneal diff` shows a huge "improvement" that is actually just a filter change
- `anneal status` convergence summary is poisoned by the discontinuity
- The `terminal_errors` field in CheckOutput becomes meaningless (always 0) under active-only default

**Prevention:**
- **Do NOT change the default in v1.1.** Keep current behavior as default; `--active-only` remains opt-in.
- Add a config key `[check] default_filter = "active-only"` so users can opt in per-project without a global behavior change.
- If the default must change later, do it at a major version boundary (v2.0) and make the snapshot format record the filter state so `diff` can compare like-with-like.
- The existing `terminal_errors` field in CheckOutput already lets downstream consumers compute active-only counts themselves.

**Detection:** Search for any scripts, CI configs, or documentation that reference specific error counts or parse `anneal check` output. Check if `history.jsonl` records the filter state.

**Phase:** UX enrichment phase, but the decision must be made early because it affects snapshot compatibility.

---

### Pitfall 11: Over-Engineering the Resolution Sum Type

**What goes wrong:** The `Resolution` enum (Exact/Fuzzy/Unresolved) is introduced to make the resolution cascade explicit. The temptation is to add confidence scores, multiple resolution strategies per variant, provenance tracking, and cascading fallback chains. This makes the type complex without improving outcomes.

**Why it happens:** Sum types invite elaboration. "What if we track *why* it resolved? What if we score confidence?" But anneal's resolution is structural (exact match, prefix match, bare filename match), not probabilistic. Confidence scores on structural matches are theater -- it either matched or it didn't.

**Prevention:**
- Keep `Resolution` to exactly three variants: `Exact(NodeId)`, `Fuzzy { target: NodeId, candidates: Vec<String> }`, `Unresolved { candidates: Vec<String> }`. No confidence scores. No strategy provenance. No fallback chains.
- The candidates list is the useful output. How the candidates were found is an implementation detail -- use tracing for debugging if needed, not type-level features.
- Test the Resolution enum by asserting outcomes, not internal machinery.

**Detection:** If the Resolution enum has more than 5 fields total across all variants, it's over-engineered.

**Phase:** Extraction/resolution pipeline phase. Decide the type shape at design time and resist elaboration.

---

### Pitfall 12: DiscoveredRef Unifying Too Much

**What goes wrong:** The v1.1 plan replaces 4 types (`PendingEdge`, `LabelCandidate`, `FrontmatterEdge`, `ScanResult`) with 1 type (`DiscoveredRef` + `RefHint`). If the unified type carries all information from all 4 original types via optional fields, it becomes a god struct that's worse than 4 small types.

**Prevention:**
- `DiscoveredRef` should carry: source node/file, raw text, byte range, and a `RefHint` enum.
- Each `RefHint` variant carries only kind-specific data. Label hints carry prefix + number. File hints carry the path. Frontmatter hints carry edge kind + direction. WikiLink hints carry the decoded destination.
- Key insight: `DiscoveredRef` is what the extractor produces. `Resolution` is what the resolver produces. They are separate types at separate pipeline stages. Don't merge them.
- The `DiscoveredRef` type should have zero or minimal `Option` fields. Every field should be present for every ref. If you need optionals, the type is trying to represent too many things.

**Detection:** Count `Option` fields on `DiscoveredRef`. Zero is ideal. More than 2 means reconsider.

**Phase:** Extraction pipeline phase. Design the type before implementing extractors.

---

### Pitfall 13: Heading Slug Computation Divergence

**What goes wrong:** The current scanner computes section IDs as `format!("{}#{}", file_path, heading.to_lowercase().replace(' ', "-"))` (parse.rs:349). pulldown-cmark emits heading events, but the text content comes from subsequent `Text` events that need concatenation. If the new heading-to-slug computation differs from the old one in any way (punctuation handling, unicode, consecutive hyphens), existing section references break silently.

**Prevention:**
- Extract the slug computation into a standalone `heading_to_slug(text: &str) -> String` function.
- Before replacing the scanner, add a test asserting the function produces identical slugs for all headings in both test corpora.
- Be explicit about what it does: lowercase + space-to-hyphen. Document what it does NOT do (strip punctuation, collapse hyphens, normalize unicode).
- If `ENABLE_HEADING_ATTRIBUTES` is enabled, headings can have explicit IDs: `## Heading {#custom-id}`. Decide whether anneal should use the explicit ID or the computed slug. Recommendation: use explicit ID when present, fall back to computed slug.

**Detection:** Diff old vs new section handle IDs on both test corpora.

**Phase:** Parser replacement phase. Extract the function before swapping parsers.

---

### Pitfall 14: Inline Code Labels (Improvement That Needs Awareness)

**What goes wrong:** A reference like `` `OQ-64` `` in inline code should NOT be extracted as a handle reference (it's being discussed meta-textually, not referenced). The current regex scanner has no way to detect this -- it treats `` `OQ-64` `` the same as `OQ-64`. pulldown-cmark emits `Event::Code("OQ-64")` for inline code, distinct from `Event::Text("OQ-64")`.

**Why this is a pitfall despite being correct:** Skipping inline code references changes the diagnostic output. If the test corpus has labels in backticks, some previously-reported references will vanish. This is the correct behavior but changes error counts.

**Prevention:**
- Skip `Event::Code` events in the extractor.
- This is an improvement over the current scanner. But document the behavior change: "labels in backticks are no longer treated as references."
- Compare output before/after on test corpora to quantify the impact.

**Phase:** Parser replacement phase. Wire as part of the event handler.

---

## Minor Pitfalls

### Pitfall 15: pulldown-cmark Version Pinning

**What goes wrong:** pulldown-cmark is actively developed. Feature flags like `ENABLE_WIKILINKS` are relatively new. A minor version bump could change wikilink parsing behavior or alter offset computation. The 0.13.3 release already fixed wikilink offset calculation, validating that patch updates matter.

**Prevention:**
- Use caret range `pulldown-cmark = "0.13"` (consistent with anneal's existing dependency style). Run tests on `cargo update` to detect behavior changes.
- The build time impact is acceptable: pulldown-cmark is lightweight (~2s additional compile time), staying well within the <10s target.
- Watch the changelog for behavioral changes to enabled features.

**Phase:** Initial setup when adding the dependency.

---

### Pitfall 16: Verbose Human Output Overwhelming the Terminal

**What goes wrong:** Adding source locations, candidates, and snippets to human-readable output makes each diagnostic 4-6 lines instead of 2. On Murail (186 errors), that's 750+ lines of output. Terminal scrollback loses the summary line.

**Prevention:**
- Keep the current 2-line format as default. Add `--verbose` or `-v` for enriched output.
- In default mode: `error[E001]: broken reference: OQ-99 not found -> file.md:42` (line number is the only new addition -- and it fills in the currently-null `line` field).
- In verbose mode: add candidates, snippets, evidence.
- Always print the summary line last (already the case in cli.rs:104) so it's visible.
- Consider: if > 50 diagnostics, automatically suppress verbose detail and print `"(use -v for full details)"`.

**Phase:** Diagnostic enrichment phase. Design output modes before implementing Evidence formatting.

---

### Pitfall 17: Link Events Producing Duplicate References

**What goes wrong:** A markdown link `[see OQ-64](guide.md)` produces: `Start(Link { dest_url: "guide.md" })`, `Text("see OQ-64")`, `End(Link)`. Extracting references from both the link destination (file ref: `guide.md`) and the link text (label ref: `OQ-64`) is correct. But `[guide.md](guide.md)` produces the same file reference twice.

**Prevention:**
- Deduplicate discovered refs by (source_file, target_identity, edge_kind) before resolution. This is partially done for edges in the current code but should be explicit in the extraction pipeline.
- For `Link` events: extract destination as a file/URL reference. Text content flows through normally as `Text` events between `Start(Link)` and `End(Link)`.
- Consider: link destinations are higher-confidence references than text mentions. Tag `DiscoveredRef` with extraction source so resolution can prioritize.

**Phase:** Parser replacement phase. Handle during event processing.

---

### Pitfall 18: Suppression Config and deny_unknown_fields

**What goes wrong:** `AnnealConfig` uses `#[serde(deny_unknown_fields)]`. Adding a new `[check]` or `[suppress]` section means a user who adds the new config and then downgrades anneal gets a parse error.

**Prevention:**
- Acceptable for pre-1.0 software. Document new config fields in `anneal init` output.
- Ensure all new config fields have `#[serde(default)]` so they're purely additive (old configs with missing new sections still parse).
- If the config schema keeps growing, consider whether `deny_unknown_fields` should be relaxed (but this conflicts with catching typos in config keys -- keep it and accept the forward-compatibility tradeoff).

**Phase:** Any phase that adds config keys.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfalls | Mitigation |
|-------------|----------------|------------|
| pulldown-cmark integration | Frontmatter double-parse (P1), text fragmentation (P3), HTML blocks (P5), indented code blocks (P6), inline code (P14) | Keep `split_frontmatter()`, build concatenation buffer, scan HTML events, skip Code events |
| Line number computation | Byte-offset misalignment (P2), heading slug divergence (P13) | Build `SourceMap` from full file content, extract `heading_to_slug()` function |
| Wikilink support | URL encoding (P7), precedence (P8) | Percent-decode destinations, wire WikiLink events into extraction pipeline early |
| DiscoveredRef + Resolution types | Over-engineering Resolution (P11), god struct (P12) | Max 3 Resolution variants, zero Option fields on DiscoveredRef |
| "Did you mean?" cascade | False suggestions (P9) | Structural transforms only, cap at 3 candidates, neutral language |
| Diagnostic enrichment | JSON schema breakage (P4), verbose output (P16), duplicates (P17) | Additive-only JSON changes, default terse + `--verbose`, deduplicate refs |
| Active-only default change | CI breakage + snapshot discontinuity (P10) | Keep current default, add config opt-in |
| Dependency management | Version drift (P15), config compat (P18) | Caret range pin, `#[serde(default)]` on new fields |

---

## Integration Risk Matrix

Pitfalls interact. These combinations are dangerous:

| Combination | Risk | Why |
|-------------|------|-----|
| P1 + P2 | **HIGH** | If frontmatter handling is wrong, all line numbers are wrong everywhere |
| P4 + P10 | **HIGH** | Changing JSON schema AND changing defaults simultaneously breaks all automation |
| P3 + P9 | MEDIUM | Text fragmentation causes missed references; fuzzy matching then suggests wrong fixes for the gaps |
| P5 + P6 | MEDIUM | Both change what content is scanned; combined effect unpredictable without parallel-run comparison |
| P11 + P12 | LOW | Over-engineering both types wastes effort but is fixable later |

**Recommended implementation order to minimize risk:**
1. P1 + P2 first: frontmatter boundary + SourceMap (everything else depends on this)
2. P3: text concatenation buffer (all pattern matching depends on this)
3. P5 + P6 + P14: content coverage decisions (what to scan, what to skip)
4. P4: JSON compatibility contract (design before any Diagnostic struct changes)
5. P7 + P8: wikilink handling
6. Everything else

---

## Sources

- [pulldown-cmark Options struct](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.Options.html) -- feature flag reference (HIGH confidence)
- [pulldown-cmark wikilinks spec](https://pulldown-cmark.github.io/pulldown-cmark/specs/wikilinks.html) -- edge cases, precedence, pipe handling (HIGH confidence)
- [pulldown-cmark metadata blocks spec](https://pulldown-cmark.github.io/pulldown-cmark/specs/metadata_blocks.html) -- YAML metadata block behavior and delimiter rules (HIGH confidence)
- [pulldown-cmark Parser API](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.Parser.html) -- `into_offset_iter()` byte range semantics (HIGH confidence)
- [pulldown-cmark issue #441](https://github.com/pulldown-cmark/pulldown-cmark/issues/441) -- offset range granularity limitations for link titles (MEDIUM confidence)
- [CommonMark specification](https://spec.commonmark.org/0.12/) -- HTML blocks, indented code blocks, fenced code blocks (HIGH confidence)
- [pulldown-cmark issue #399](https://github.com/pulldown-cmark/pulldown-cmark/issues/399) -- HTML block consuming content through blockquotes (MEDIUM confidence)
- anneal source: `parse.rs` (scanner + frontmatter), `checks.rs` (diagnostics), `cli.rs` (JSON output schema), `resolve.rs` (resolution pipeline) -- direct analysis (HIGH confidence)
- Live `anneal check --json` output from Murail corpus: confirmed exact field names and types in JSON contract (HIGH confidence)
