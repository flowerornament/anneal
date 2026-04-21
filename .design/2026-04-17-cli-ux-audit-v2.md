---
status: draft
updated: 2026-04-17
description: >
  Visual-UX audit of anneal's CLI surface. Scores each command against a
  shared design system (adapted from nx-rs) and proposes unified printer
  primitives. Complements the 2026-04-02 context-budget audit, which
  covers payload size rather than presentation.
---

# anneal CLI UX Audit (v2, presentation-focused)

The 2026-04-02 audit asked "how much does it dump?". This one asks "does
it look like a coherent tool?". Same CLI, different lens.

## Current state (one paragraph)

`src/style.rs` defines a small semantic palette (`error`, `warning`,
`info`, `suggestion`, `label`, `dim`, `bold`, `green`) wrapped in
`console::Style`. 14 CLI modules import `S::` directly and interleave
`println!` / `writeln!` with inline `.apply_to(..)`. There is no
`Printer`, no glyph tier, no layout grid, no `--plain` / `--unicode` /
`--minimal` mode. Each command invents its own shape: `status` uses
leading-space + two-column indent; `check` uses `level[CODE]: body \n
  -> ref`; `orient` uses `Title:\n  item` with right-padded byte
budgets; `areas` uses a Unicode-rule table; `garden` uses
`N. [tag] body   blast=level`. Spacing is inconsistent (some sections
blank-line-separated, others not). There is no common "heading (count)"
pattern, no action glyphs, no error-recovery hint convention.

## Design system delta

Target primitives, adapted from nx-rs:

| Primitive | Anneal today | Target |
|---|---|---|
| Semantic palette | `src/style.rs` — 8 roles | Extend to `success`, `error`, `warning`, `heading`, `path`, `number`, `callout`, `dim`, `activity`. Map to `console::Style` centrally. |
| Glyph tiers | none (ASCII only, usually no glyphs) | Three-tier: Unicode default (`✔ ✘ ➜ • ~`), ASCII fallback (`+ x > - ~`), auto-detect + `--plain`/`--minimal` overrides. Not pursuing Nerd Font tier — niche. |
| Printer | none | `crate::output::Printer` with `heading`, `kv`, `bullet`, `result_ok/err`, `hint`, `section_break`, `table`, `list_item`. Owns glyph + color policy. |
| Layout grid | ad hoc | Gutter col 0–1 for glyphs; content col 2; sub-indent col 4. |
| Output modes | `--json` / `--pretty` only | Add `--plain` (no color, no glyphs) and `--minimal` (ASCII glyphs). `NO_COLOR` already respected via `console`. |
| Heading pattern | mixed (`Read next (...)`, `Found 22 matches for "x":`, `Graph summary:`, no heading on `status`) | `**Heading** (count)` universal. Bold, parenthetical count. |
| List/KV shape | mixed — some aligned, some not | Fixed label width 12–14, paths in `path` color, numbers in `number` color. |
| Hint/next-step | mixed — `status` has `next` block, `areas` has a trailing sentence, most commands have nothing | Universal "Try: `anneal X`" callout, blank line before, `callout` color, not dim. |
| Error + recovery | diagnostics-only; no fix suggestions on real errors (missing handle, bad flag) | When a user-facing error can suggest a fix, emit recovery hint on next line. |

Implementation plan (not this doc's scope, but the shape):

1. Add `src/output/style.rs` (mode + glyph policy) and
   `src/output/printer.rs` (semantic methods). Keep the current
   `crate::style::S` as a thin re-export so migration is incremental.
2. Migrate one command at a time, starting with the heaviest
   hand-formatters (`status`, `garden`, `orient`, `check`).
3. Each migration commit ships with a snapshot test capturing the new
   output so regressions are loud.

## Command-by-command audit

Score legend: **P** (pattern fit), **T** (typography), **C**
(consistency with siblings), **E** (error/hint recovery). 1 = bad,
3 = good.

### anneal status

Current (murail corpus):

```
 corpus  401 files, 12557 handles, 7660 edges
         12304 active, 253 terminal
    pipeline  0 raw → 18 draft → 17 research → 0 exploratory → 2 plan → 0 current → 19 active

 health  2 errors, 0 warnings

 convergence  holding (resolution +0, creation +0, obligations 0)

 suggestions  55
      35  S001 orphaned handles
      13  S002 candidate namespaces
       2  S003 pipeline stalls
       5  S005 concern group candidates

 next
   anneal check for detailed diagnostics
   anneal garden for ranked maintenance tasks
```

Scores: P1 T2 C2 E3. The structure (section label at col 1, values
at col 9, nested kv at col 4) is unique to this command. Section
labels are lowercased (not bold), which makes them easy to miss when
scanning. The `next` block is the right idea but looks like body text.

Violations:
- No `**Heading** (count)` pattern; section labels at col 1 read as
  keys not titles.
- 253 terminal, 2 errors, etc. are pure numbers; no `number` color
  makes scannability poor.
- `next` hints are indented like data rows, not styled as callouts.
- Pipeline arrows `→` are fine, but the whole line is unstyled; the
  eye can't find the health number fast.
- Inconsistent leading space (` corpus`, ` pipeline` each prefixed
  with one space, `    pipeline` with four — a visual wobble).

Target:

```
  Corpus status  401 files · 12,557 handles · 7,660 edges
                 12,304 active · 253 terminal
  Pipeline       raw 0 → draft 18 → research 17 → exploratory 0 → plan 2 → current 0 → active 19

  Health         2 errors · 0 warnings

  Convergence    holding  (resolution +0 · creation +0 · obligations 0)

  Suggestions (55)
    35  S001   orphaned handles
    13  S002   candidate namespaces
     2  S003   pipeline stalls
     5  S005   concern group candidates

  Try: anneal check          for detailed diagnostics
       anneal garden         for ranked maintenance tasks
```

- KV labels bold, col 2, width 14.
- Thousands separators on counts; counts in `number` color.
- `→` pipeline unchanged; "active" suffix stays.
- `Suggestions (N)` follows the universal heading pattern.
- Hints via `Try:` callout block, not a section.

### anneal check

Current (self corpus):

```
info[I001]: 270 section references use section notation, not resolvable to heading slugs
  -> 2026-04-02-progressive-disclosure-output-spec.md

0 errors, 0 warnings, 1 info, 0 suggestions
```

Current (murail, truncated):

```
info[I001]: 2442 section references use section notation, not resolvable to heading slugs
  -> references/domain/2026-03-29-rust-ecosystem.md
suggestion[S002]: candidate namespace: H-COMP (4 labels found, not in confirmed namespaces)
  -> research-log/2026-04-07-formal-model-jit-constraints.md
...
```

Scores: P2 T2 C1 E1. Close to a `compiler-style` diagnostic, which
is a real and respected pattern (rustc, clippy). But:
- No color on severity words when TTY — `error`, `warning`, `info`,
  `suggestion` all render identical weight/color (the code has styles
  but only for `error`).
- `->` arrow for "site of occurrence" is ASCII; mixes with
  Unicode `→` used in `status`. Pick one.
- The summary footer is not visually separated; a blank line before
  it would make it scannable.
- No file:line anchoring, which is the whole point of a
  compiler-style diagnostic. A plain filename is weaker than
  `path/to/file.md:42` that editors can open.
- On `0 errors, 0 warnings`, we still print the summary line. Good.
  But when the summary is non-trivially long ("2442 section
  references..."), the "-> path" continuation line attaches
  ambiguously to the prior message.

Target:

```
  warning[I001]  270 section references use section notation, not resolvable to heading slugs
                 at 2026-04-02-progressive-disclosure-output-spec.md

  suggestion[S002]  candidate namespace: H-COMP (4 labels found)
                    at research-log/2026-04-07-formal-model-jit-constraints.md

  0 errors · 0 warnings · 1 info · 0 suggestions
```

- Severity colored semantically (red/yellow/cyan/dim) with padding so
  codes align.
- `at <path>` instead of `-> <path>`. Path in `path` color. Agrees
  with nx-rs idiom.
- Blank line between diagnostics improves scan density without
  meaningfully inflating the byte budget.
- Blank line before summary; summary uses `·` separator like
  `status`.
- Where we have line numbers (extraction already tracks them for
  section handles), append `:NNN`.

### anneal orient

Current (self, `--budget=20k`, truncated):

```
Read next (area entry points, ranked by centrality × recency):
  anneal-spec.md                                                         [14k]
      Specification for anneal — a convergence assistant for knowledge...
  2026-04-02-cli-output-audit.md                                         [4k]
      Date: 2026-04-02
  ...

Budget: 47k / 50k used
```

Scores: P2 T2 C2 E2. This is one of the cleaner commands visually,
but still:
- Heading `Read next (...)` is plain text, not bold. The parenthetical
  is where our pattern catalog would put a count, but here it's a
  tagline.
- File paths at col 2, byte budgets right-aligned at col ~70 via
  hard-coded padding — won't reflow in narrow terminals.
- Snippets at col 6 in normal weight: works, but should be `dim` so
  the filename (the clickable unit) dominates.
- `[14k]` uses ASCII brackets + lowercase `k`; elsewhere we say
  `20k tokens` in help. Align the token-shorthand across commands.
- Footer `Budget: 47k / 50k used` is prose; should mirror the
  "trailing summary" pattern we want on `check`.

Target:

```
  Reading list (7 files · 47k / 50k budget)
  ranked by centrality × recency
  
    anneal-spec.md                                14k
        Specification for anneal — a convergence...
    2026-04-02-cli-output-audit.md                 4k
        Date: 2026-04-02
    ...

  Try: anneal get <file>    for details on one entry
       anneal impact <file> for downstream review
```

- Heading with count, blank sub-line with ranking rationale in `dim`.
- File in default, snippet in `dim`, token count in `number`.
- Right-pad to terminal width (or to 68 cols as default), not a
  hard 72.
- Hint block replaces trailing `Budget:` line (budget now lives in
  heading). Keeps the info, loses the footer repetition.

### anneal garden

Current (murail, truncated):

```
 1. [fix] 2 broken refs in implementation/   blast=high
             broken reference: specimens/cpu-fast-path/pitch-control-filter-interaction/family.toml not found
             fix:     resolve or remove the broken references listed
             context: anneal orient --area=implementation --budget=20k
             verify:  anneal check --area=implementation --errors-only
 2. [tidy] 9 orphaned labels in compiler/   blast=med
             2026-03-16-bytecode-format-v1, 2026-03-16-compiler-design-v1, ...
             fix:     resolve or remove the broken references listed
             ...
```

Scores: P2 T1 C2 E3. The "fix/context/verify" triad is excellent — a
real usability win. But the visual is dense:
- 13-space indent before the `fix:/context:/verify:` block is arbitrary
  and doesn't compose with any other command's indent.
- Category tag `[fix]` shares bracket style with orient's `[14k]` but
  means something different (category vs scalar). Visual collision.
- `blast=high` / `blast=med` / `blast=low` is data-encoded inside
  the heading line; no color on severity. High blast should read red,
  med yellow, low dim.
- Numbers at col 1 with one-digit/two-digit wobble — `1.` vs `10.`
  shifts the start column.
- Long lists of orphan labels wrap awkwardly.

Target:

```
  Maintenance tasks (8)
  
   1  [FIX]   implementation/  · 2 broken refs                      high
      broken reference: specimens/cpu-fast-path/.../family.toml not found
      fix      resolve or remove the broken references listed
      context  anneal orient --area=implementation --budget=20k
      verify   anneal check --area=implementation --errors-only
  
   2  [TIDY]  compiler/        · 9 orphaned labels                  med
      2026-03-16-bytecode-format-v1, 2026-03-16-compiler-design-v1,
      2026-03-16-core-types-spec-v8 (6 more)
      fix      reference these labels from relevant documents, or retire them
      context  anneal orient --area=compiler --budget=20k
      verify   anneal check --area=compiler --suggest
```

- Right-pad number column to fit 2-digit indices without jitter.
- Category tag uppercased, monospaced-width (`[FIX]`, `[TIDY]`,
  `[LINK]`, `[CLEAN]` — 5 chars each).
- Area in `path` color, counts in `number` color.
- Blast label at right edge, colored by severity; drops `=` sign.
- Action triad left-aligned at col 6, no colon on the label (label in
  `callout` color so it still scans).
- Label list truncates to N entries with `(K more)`, not `...`.
- Blank line between tasks.

## Remaining commands (audit stub)

To be filled in before any Printer migration. Rows marked **priority**
ship first.

| Command | P | T | C | E | Notes |
|---|---|---|---|---|---|
| status    | 1 | 2 | 2 | 3 | done above. **priority** |
| check     | 2 | 2 | 1 | 1 | done above. **priority** |
| orient    | 2 | 2 | 2 | 2 | done above. **priority** |
| garden    | 2 | 1 | 2 | 3 | done above. **priority** |
| areas     | — | — | — | — | Table pattern. Grade column is already color-coded in code but not in doc. |
| find      | — | — | — | — | List needs a count header; dedup filename echo; kind badge. |
| get       | — | — | — | — | KV block; incoming/outgoing edges are a sub-table. Close to nx-rs "info" command. |
| map       | — | — | — | — | Two surfaces (summary + around). Needs heading/branching discipline. |
| impact    | — | — | — | — | Two list sections; closest to a pair of headings + bullets. |
| diff      | — | — | — | — | KV block; already compact. Easy migration. |
| obligations | — | — | — | — | One-line summary; easy. |
| init      | — | — | — | — | Config diff / dry-run preview; could borrow the code-panel pattern. |
| query     | — | — | — | — | Subcommand tree; each subcommand is a list. |
| explain   | — | — | — | — | Narrative output; hardest to systematize. |
| prime     | — | — | — | — | Pure passthrough of skills/anneal/SKILL.md. No change needed. |

## Rollout plan

1. **Printer scaffolding** — add `src/output/{printer.rs,style.rs}`
   alongside `src/style.rs`. Extend the palette; add glyph tiers;
   expose semantic methods. No behavior change yet.
2. **Migrate priority 4** — `status`, `check`, `orient`, `garden`.
   Each lands with a snapshot test and a before/after in the PR body.
3. **Fill in the stub table** above; migrate the rest.
4. **Retire `src/style.rs`** once no `S::` references remain.

Snapshot tests belong under `tests/ui/` (new directory) and exercise
each command against a small fixture corpus so layout regressions are
caught in CI.

## Non-goals

- Nerd Font glyph tier (low audience overlap with anneal's users).
- Spinners or progress bars (anneal is fast; all commands finish in
  < 1s on tested corpora).
- Interactive prompts (anneal has none today; `init` writes on
  confirmation but doesn't prompt inline).
- Breaking `--json`. JSON stays machine-first; all visual work is
  human-facing.

---

# Round 2: comprehensive audit (planned — not yet executed)

After migrating 12 of 15 commands, manual inspection surfaced cross-cutting
coherence issues that weren't visible from per-command work:

- `·` separators everywhere (breaks Tufte — whitespace should be the
  divider, glyph is chartjunk)
- Inconsistent blank-line cadence between commands
- Inconsistent truncation (orient=120, find=160; nothing enforces)
- Columnar alignment enforced in `table()` but ad-hoc in KV rows and
  indexed lists
- Some signals are color-only (agents piping anneal see no color; signal
  must live in text)
- Not every command starts with a heading; visual landmark absent

Round 2 performs a full audit — every command, every relevant flag,
every output mode — through three lenses, populates a running findings
table, writes design rules first, then fixes in waves.

## Methodology

Each output is evaluated on three lenses. All three must pass.

### Lens 1 — Graphic design

Act as a visual designer:

- **Hierarchy** — does the eye find the most important thing first?
- **Rhythm** — consistent indentation, padding, blank-line cadence?
- **Whitespace-as-divider** — any `·`/rule/border that could be replaced
  with space?
- **Alignment** — columns enforced where data is columnar?
- **Typography** — bold only where it matters, dim only for secondary,
  consistent use?
- **Density** — neither sparse nor cramped?

### Lens 2 — Agent parseability

Assume no color. Assume regex:

- Does text alone carry every signal? (color is an enhancement, not a
  carrier)
- Can an agent regex/split key fields easily?
- Are truncations marked so agents know they can expand? (`… N more`,
  `showing N of M`)
- Row shapes consistent within a command?
- Tokens wasted on decorative chars?
- Is `--json` available and useful where structure matters?

### Lens 3 — Cross-command consistency

- Same primitive → same look?
- Heading pattern universal?
- Blank-line policy uniform?
- Navigation hints (`Try`, `expand with`) consistent?

## Test matrix

Run on **two corpora** (`~/code/anneal/.design` + `~/code/murail/.design`)
and four modes where relevant (default / `--plain` / `--minimal` /
`--json`). Capture all outputs to `/tmp/anneal-audit-round2/`.

| Command       | Variants to capture                                                  |
|---------------|----------------------------------------------------------------------|
| `anneal`      | default                                                              |
| `status`      | default, `--verbose`, `--json`, `--json --compact`                   |
| `check`       | default, `--errors-only`, `--suggest`, `--stale`, `--file=X`, `--json` |
| `get`         | single, `--context`, `--refs`, `--trace`, `--full`; batch; `--status-only` |
| `find`        | empty+filter, keyword, `--limit=5`, `--offset`, `--context`, `--sort=date` |
| `init`        | `--dry-run`                                                          |
| `impact`      | default, `--area=X`, `--json`                                        |
| `map`         | summary, `--around=X`, `--concern=X`, `--by-area`, `--render=text --full`, `--render=dot` |
| `diff`        | default, `--days=7`, `--by-area`                                     |
| `obligations` | default                                                              |
| `prime`       | default                                                              |
| `areas`       | default, `--sort=grade`, `--sort=name`, `--include-terminal`         |
| `garden`      | default, `--category=fix`, `--area=X`, `--limit=3`                   |
| `orient`      | default, `--area=X`, `--file=X`, `--budget=20k`, `--paths-only`      |
| `query`       | handles, edges, diagnostics, obligations, suggestions                |
| `explain`     | diagnostic, impact, convergence, obligation, suggestion              |

## Design rules (enforced — write the rule first, then fix)

Seed set. Add rules as the audit surfaces violations the current set
doesn't name.

- **R1** No `·`/`•` glyph separators in inline content. Comma is the
  inline separator for nominal lists (`9 files, 351 handles, 7 edges`).
  Blank line divides sections. List indentation replaces bullet markers.
- **R2** Every command output starts with `**Heading** (count?)`.
- **R3** Blank lines only between logical sections. Never within a
  section. Never at output start or end.
- **R4** Text carries every signal. Color is a redundant enhancement,
  never the sole carrier.
- **R5** Columnar data uses `Printer::table` or aligned `kv_block`.
  Never ad-hoc `" ".repeat(n)` padding for alignment.
- **R6** One `SNIPPET_MAX` constant for all truncation (120 chars).
- **R7** Navigation hints go through `Printer::hints` — never ad-hoc
  `Try:` strings.
- **R8** Truncations are explicit: `… N more` inline, or `showing N of
  M · offset K` in headings.
- **R9** Integer counts use `Line::count(usize)` (thousands-separated).
  Floats use `Line::float(f, precision)`. No bespoke numeric formatting.
- **R10** Paths always `Tone::Path`. Counts always `Tone::Number`.
  Severity labels always `Severity::tone()`.
- **R11** `--json` is the machine-parseable surface. `--plain` is the
  no-glyph no-color human surface. `--minimal` is ASCII glyphs + color.
  Default is Unicode + color (pipe-detected off).

## Findings table

Populated by Phase 1 against two corpora (~/code/anneal/.design,
~/code/murail/.design). Severity: `critical` (breaks parseability),
`design` (visual rough edge), `nit` (polish).

**Rule refinement (R1).** During capture, "double-space as the divider"
proved hard to read for inline nominal lists (`9 files  351 handles  7
edges` blurs values). Comma is punctuation, not chartjunk, and matches
how these same counts already render in the `Convergence` detail
(`resolution +0, creation +0, obligations 0`). R1 is therefore
amended: **inline separators between list items use comma**. `·`/`•`
as glyph-separator stays banned; whitespace divides sections.

| ID  | Command              | Variant                      | Issue                                                                                            | Severity | Rule | Fix                                                                                         | Status |
|-----|----------------------|------------------------------|--------------------------------------------------------------------------------------------------|----------|------|---------------------------------------------------------------------------------------------|--------|
| F01 | status               | default                      | `· ` separator in corpus/active/terminal tally; also between health counts and convergence       | design   | R1   | `Printer::tally` → comma; inline helpers use comma                                          | closed |
| F02 | status               | default                      | Convergence detail already uses comma — separator choice is inconsistent with tally              | design   | R1   | unified comma closes the gap                                                                | closed |
| F03 | status               | verbose                      | `0` vs `+0` inconsistency on `obligations` depending on default/verbose render path              | nit      | —    | audit the convergence-detail formatter so sign convention is one code path                  | open   |
| F04 | status               | verbose                      | No blank line between last pipeline file and the Health block — visual crush                     | design   | R3   | False alarm on re-capture: blank is emitted before Health group                              | closed |
| F05 | check                | default, errors-only, stale, suggest | Missing top heading — hard to landmark                                                            | design   | R2   | Emit `Diagnostics (N)` or `Check` heading (count = total findings)                          | closed |
| F06 | check                | default                      | Summary tally uses `·`                                                                           | design   | R1   | closes with F01 via `tally()`                                                                 | closed |
| F07 | check                | stale with zero findings     | Output is just a summary — no heading context; agent can't tell what ran                         | design   | R2   | heading emitted unconditionally                                                              | closed |
| F08 | check                | default on murail            | 16 suggestions back-to-back, no visual grouping between severity classes                         | nit      | —    | Insert a blank line between severity groups (errors → info → suggestions)                   | closed |
| F09 | garden               | default                      | Missing top heading                                                                              | design   | R2   | `Maintenance tasks (N)` heading                                                              | closed |
| F10 | garden               | default, limit3              | `5 broken refs · implementation/` — `·` as separator                                             | design   | R1   | Rewrite to `5 broken refs in implementation/`                                                | closed |
| F11 | garden               | default                      | `high/med/low` blast shoved right with jitter; title width determines blast column               | design   | R5   | Move blast to leading 4-char column: `1  HIGH  [FIX]  5 broken refs in implementation/`     | closed |
| F12 | garden               | default                      | Orphan list truncation: `... (4 more)` in TIDY tasks vs `... (+12 more)` in STALE tasks          | design   | R8   | Single format: `... (N more)` everywhere                                                    | closed |
| F13 | garden               | default                      | Footer `Showing 10 of 32 tasks — --limit=20 for more` uses em-dash + ad-hoc phrasing             | design   | R7   | Replace with `Printer::hints` block: `Try  --limit=20  expand`                               | closed |
| F14 | garden               | default                      | Footer truncation display is a one-liner; find uses a sub-head `showing N of M · offset K`       | design   | R8   | Unify: both use the subtitle form under the heading                                         | closed |
| F15 | find                 | kw, limit5, sort-date        | Very long hash-suffixed handle IDs blow out the first column; later rows don't align with first  | design   | R5   | Verified on re-capture: `Printer::table` widens to max; rows align cleanly. No fix needed    | closed |
| F16 | find                 | kw (murail)                  | File column repeats same base path 18× when all hits are sections of one file                    | design   | —    | Out of scope (needs grouping feature); defer to Round 3                                      | defer  |
| F17 | find                 | context                      | Context snippets appear only on `file` kind; sections have no snippet even with `--context`      | design   | —    | Product-level feature gap; not a presentation bug. Defer                                     | defer  |
| F18 | orient               | default                      | Footer `Budget 49k / 50k tokens used` is prose trailer; inconsistent with heading pattern         | design   | R2   | Accepted as section-separator form (blank + heading-styled "Budget"). No change              | wontfix |
| F19 | orient               | default                      | Snippet trailing text stops mid-word with no ellipsis when char-truncated                        | design   | R6   | Unified `SNIPPET_MAX` truncator appends `…`                                                  | closed |
| F20 | orient               | --file                       | Two heading levels: `Reading list for X` + tier heading. Re-capture shows both at col 2           | design   | R2   | Accepted: outer anchors the command, tier heading anchors the section. Both at col 2         | wontfix |
| F21 | orient, find         | cross-command                | SNIPPET limits diverge (orient 120, find 160). Agent-inspection inconsistency                    | design   | R6   | One `SNIPPET_MAX = 120` in `cli/mod.rs`                                                      | closed |
| F22 | get                  | default                      | Heading `path (kind)` — path already uses `Tone::Heading` (bold). Reads fine in monochrome       | design   | R2   | Verified adequate on re-capture. No change                                                   | closed |
| F23 | get                  | context                      | `Snippet` kv row duplicates content of `Context` section                                         | design   | —    | Drop `Snippet` when `--context` present                                                      | closed |
| F24 | get                  | full                         | `--full` output identical to `--refs` — doesn't expand the "... 112 more" tail                   | critical | —    | Product-level bug outside UI scope. Open as separate anneal issue for Round 3                | defer  |
| F25 | get                  | default                      | No `Try` hint block — agents don't discover `--context`/`--refs`/`--full` paths                  | design   | R7   | Add hint rows to `GetHumanOutput::render`                                                    | closed |
| F26 | get                  | context                      | `Incoming (10 of 10)` is redundant when returning all (should be `Incoming (10)`)                | nit      | R8   | Drop `of M` when N == M                                                                      | closed |
| F27 | impact               | default                      | Uses `•` bullet but indent already communicates list — decorative                                | design   | R1   | Remove bullet, use indent-only at SUB_COL                                                    | closed |
| F28 | impact               | default                      | `(none)` under `Indirect (0)` is at col 4 (SUB_COL) while bullets are at col 2 — inconsistent    | design   | R5   | With bullets removed, both at SUB_COL; consistent                                            | closed |
| F29 | impact               | default                      | Caption `what depends on <handle>` already renders via `caption()` (dim tone). Acceptable         | nit      | R2   | Verified on re-capture                                                                       | closed |
| F30 | map                  | summary                      | `12,571 nodes · 3,562 edges` — `·`                                                              | design   | R1   | closes with F01                                                                              | closed |
| F31 | map                  | summary                      | `By kind` count column width doesn't expand for 5-digit counts — `11,582  section` pushes right  | design   | R5   | Not yet fixed — by_kind uses inline `indexed`/`line` pairs not a table. Open                 | open   |
| F32 | map                  | --around                     | Raw `writeln!` output; no Printer. `->` arrows, custom `-Cites->` triple, `... and N more files` | critical | R1,R2,R8 | Large rewrite with extensive test updates. Defer to Round 3 with query/explain           | defer  |
| F33 | map                  | --render text --full         | Raw `writeln!` passthrough; uses `-Kind->` triple arrows, `[status]` status suffix               | design   | —    | Accepted as explicit passthrough for DOT/text pipelines                                      | wontfix |
| F34 | map                  | --by-area                    | Count jitter inside `—601→` arrow — `—9→` vs `—601→` widths differ                            | nit      | R5   | Not yet fixed. Open                                                                          | open   |
| F35 | diff                 | default, by-area             | `+0 created · +0 active · +0 terminal` — `·`                                                    | design   | R1   | closes with F01                                                                              | closed |
| F36 | obligations          | default                      | `0 outstanding · 0 discharged · 0 mooted` — `·`                                                 | design   | R1   | closes with F01                                                                              | closed |
| F37 | init                 | --dry-run                    | Header `anneal.toml` + caption indented at col 2, but TOML body unindented at col 0              | design   | R3   | Trade-off accepted: TOML body stays col-0 for copy/paste; header stays col-2 per R2           | wontfix |
| F38 | areas                | all                          | `Try anneal garden  ranked tasks for the 1 degraded area` — phrasing compresses sentence          | nit      | R7   | Hint descriptions vary across commands as prose; acceptable for now                           | wontfix |
| F39 | query                | all subcommands              | Raw `writeln!`; no indent, lowercase `next` footer, `-> path` arrows                             | design   | —    | **Deferred to Round 3** per scope guard                                                     | defer  |
| F40 | explain              | all subcommands              | Raw `writeln!`; lowercase kv style; unindented                                                   | design   | —    | **Deferred to Round 3** per scope guard                                                     | defer  |
| F41 | get                  | default                      | Arrow alignment between Outgoing and Incoming sections uses different padding widths             | nit      | R5   | OK within-section; cross-section mismatch is acceptable. No fix                              | wontfix |
| F42 | find                 | kw, limit5, sort-date        | File column is redundant when kind=section (same as base path without `#hash`)                    | nit      | —    | Could omit for section kind; defer to Round 3 when find gets grouping                        | defer  |

## Execution sequencing

- **Phase 1 — audit (no code changes):** run every matrix cell, capture
  to `/tmp/anneal-audit-round2/`. Populate findings table. Write new
  rules as violations surface. Target: 60 outputs reviewed, findings
  table has every violation indexed. ~30min.
- **Phase 2 — cross-cutting fixes (Wave A, one commit):** changes to
  `Printer`/`Line` primitives that fix multiple commands at once — kill
  `·` in `tally()`/`Glyph::Separator`, add `SNIPPET_MAX`, any new
  primitives needed. All R1/R5/R6/R7/R9/R10 findings close here.
- **Phase 3 — per-command fixes (Wave B, batched commits):** command-
  specific fixes (garden blast-first layout, `check`+`garden` top
  headings, find/orient truncation, etc.). R2/R3/R8/R11 findings close
  here.
- **Phase 4 — verification:** re-capture all 60 outputs, diff against
  findings. Close or reopen each row.
- **Phase 5 — doc sync:** update CHANGELOG, `README.md`, `skills/anneal/
  SKILL.md` if any user-visible behavior shifted. This doc's findings
  table becomes the PR description.

## Scope guards

- No new commands, no new features. Audit is about coherence, not
  surface expansion.
- `query` and `explain` are still narrative-shaped; they're in the test
  matrix but fixing their internal `writeln!`-based rendering is out of
  scope for Round 2. Note findings; defer migration to Round 3.
- Rule changes require updating this doc **before** the fix commit —
  the doc is the enforcement mechanism, and drift from the code means
  future rounds can't self-verify.

## References

- Primitives: `src/output/{mod,style,printer,tests}.rs`
- Shared helpers: `src/cli/mod.rs` (truncate, plural, dedup_edges)
- Design system brief: this doc's §1–§4 (from Round 1)
- Design source material: `~/code/nx-rs/.agents/{cli-design-principles,
  ux-design-system}.md`

  human-facing.
