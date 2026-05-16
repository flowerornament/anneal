---
status: draft
updated: 2026-05-16
depends-on:
  - 2026-05-13-corpus-runtime.md
  - 2026-05-16-design-review.md
description: >
  Spec for anneal's help and language-reference system. Defines the
  progressive-disclosure help architecture, the auto-generation pipeline
  for the language reference, the @doc CI gates that prevent drift, and
  the hand-written prose chapters that complement the generated catalog.
  Driven by Phase 11 cold-agent feedback (discoverability gaps,
  introspection arity confusion, vocabulary friction) and the design
  review finding that introspection primitives are both an agent UX
  feature and a documentation system at once.
---

# anneal — Help and Reference System

## Part I: Why this exists [HR-F]

Phase 11 cold-agent testing surfaced a recurring class of friction:
fresh agents and humans both fail to find what the language and tool
actually offer. Three independent cold agents on three independent
corpora all reported the same shapes of gap:

- "what predicates exist?" → eventually answered, after multiple
  exploratory queries
- "what does this verb take?" → mismatched arity, no clue from the
  error message
- "what's the project vocabulary?" → trial and error against config
  guesses
- "what does this output mean?" → unhelpful JSON wall for humans

These aren't UX polish defects in the usual sense. They're a missing
*subsystem*: the help and reference layer that sits between the
runtime's introspection primitives (`describe`, `source_of`,
`predicates`, `verbs`, `schema`, `sources`) and the user's actual
question. The primitives exist; the user-facing affordances over them
are sparse.

This spec defines that subsystem.

### §1 The core insight [HR-D1]

**Definition HR-D1 (Self-describing runtime).** anneal's runtime is
already its own reference: every predicate, verb, source, and schema
field has a structured representation queryable via the introspection
primitives defined in CR-D44 / CR-D46. The help system does not
duplicate this information — it *projects* it through audience-shaped
surfaces (CLI help, repo docs, error messages, editor tooling) that
share a single source of truth.

A consequence: the language reference is not a separate document
maintained alongside the code. It is a *rendering* of what the runtime
already exposes, with hand-written prose for the conceptual layer that
the runtime does not (yet) introspect — grammar, stratification
semantics, error rationales, tutorials.

This makes drift between docs and code impossible by construction.

### §2 Audiences

| Audience | Primary surface | Secondary surface |
|---|---|---|
| Cold agent (LLM, no prior session memory) | Built-in introspection primitives (NDJSON) | Repo docs as fallback |
| Human at terminal | `anneal <verb> --help` + `anneal help <topic>` pretty-rendered | Repo docs |
| Human reading repo | `docs/LANGUAGE.md` rendered Markdown | Source `.dl` files with `@doc` |
| Project author | `anneal describe <topic>` + `anneal schema` | `docs/PROJECT-GUIDE.md` |
| Adapter author (v2.1+) | `docs/ADAPTER-GUIDE.md` | Source examples |
| Editor tooling (LSP, completions) | Introspection primitives + MCP catalog | — |
| CI / quality gates | Renderer idempotency + `@doc` coverage | — |

### §3 The progressive disclosure principle [HR-D2]

**Definition HR-D2 (Progressive help layers).** Help reveals more
detail in named layers. Each layer's command surface pays only the
cost of what that layer needs. Agents and humans choose their entry
point.

```
Layer 1 — Existence
  anneal --help                    What commands exist?
  anneal verbs                     What verbs exist?
  anneal schema                    What predicates and signatures exist?

Layer 2 — Surface usage
  anneal <verb> --help             How do I invoke this?
  anneal help <topic>              What does this topic mean?

Layer 3 — Semantic depth
  anneal describe <name>           What is this for, in prose?
  anneal schema [<name>]           What's the signature, fields, types?

Layer 4 — Source provenance
  anneal -e '? source_of("name", file, lines).'
                                    Where is this defined?
  anneal -e '? examples("name", example).'
                                    What real uses exist?

Layer 5 — Read the source
  anneal read <handle>             Show me the actual definition.

Layer 6 — Full reference
  docs/LANGUAGE.md                 Hand-written prose + generated catalog.
```

A cold agent's first move is Layer 1 (`anneal --help`). A familiar
user goes straight to Layer 2 (`anneal context --help`). A debugging
user reaches Layer 4 (`anneal -e '? source_of("potential", file, lines).'`).
A learning user
opens Layer 6 (`docs/LANGUAGE.md`). No layer is mandatory; each
unlocks the next.

---

## Part II: Help surfaces [HR-S]

### §4 `anneal --help` [HR-D3]

**Definition HR-D3 (Top-level help discipline).** `anneal --help` is
the cold-agent's first impression. It serves three jobs in one screen:

1. **What this tool is** — one sentence.
2. **Where to start** — three pointer commands by intent (orient /
   inspect / validate / resume).
3. **What commands exist** — categorized list (runtime verbs vs
   legacy compatibility), each with one-line purpose.

The output is human-rendered at TTY (paragraph + tables). A
machine-readable help projection is a follow-up surface; v0.11.0
does not ship `--help --format=json`.

Existing failure mode (anneal-wcv, fixed in 653f442): top-level help
omitted runtime commands. The fix landed but the convention should be
explicit: every command registered in the binary appears in
`anneal --help`'s Commands section, period.

### §5 `anneal <verb> --help` [HR-D4]

**Definition HR-D4 (Per-verb help).** Every verb (runtime or legacy)
responds to `--help` with a static-rendered usage block before any
runtime loading. The block contains:

- Usage line with positional args and options
- One-paragraph description
- Argument descriptions
- Option descriptions with defaults
- 2-3 example invocations (mandatory)
- "See also" cross-references to related verbs

The static rendering is bounded — no corpus access, no I/O. This is
what makes `--help` safe to call on any verb without surprise side
effects.

Existing failure mode (anneal-359, fixed in 653f442): runtime subcommands
rejected `--help`. Per HR-D4 every verb's `--help` must work; this is
now a regression-tested invariant.

### §6 `anneal help <topic>` [HR-D5]

**Definition HR-D5 (Topic help).** Non-verb topics — language syntax,
semantic concepts, error codes, conventions — should become reachable
via `anneal help <topic>` after v0.11.0. Examples:

```
anneal help syntax              Grammar overview
anneal help aggregation         How Count/Sum/TopK/TakeUntil work
anneal help stratification      Why negation cycles are rejected
anneal help E001                Diagnostic code reference
anneal help capabilities        ActorContext and capability gating
anneal help convergence         Convergence vocabulary (potential, entropy, etc.)
```

Topic resolution order:
1. Verb names → equivalent to `<verb> --help`
2. Topic registry (compiled into `anneal-cli`) → hand-written help text
3. Predicate names → equivalent to `anneal describe <name>`
4. Fallback → error with similar-topic suggestions

The future topic registry is a small embedded set of hand-written
markdown files in `crates/anneal-cli/src/help/topics/*.md`, loaded at
build time. Each topic is also a section in `docs/LANGUAGE.md` — same
source, two delivery surfaces. v0.11.0's shipped `help` surface is
command-oriented; topic help is not yet a shipped promise.

### §7 `anneal describe <name>` [HR-D6]

**Definition HR-D6 (Runtime semantic introspection).** `describe`
returns the `@doc` prose for any documented name in the runtime —
predicates, verbs, sources, primitive groups. Output is one
`{"name": ..., "doc": ...}` row in JSON for pipes; pretty-rendered
prose for TTY (per HR-D9 below).

`describe` answers the "what is this for?" question. It complements
`schema` (which answers "what's the signature?") and `source_of`
(which answers "where is this defined?").

The data source is `@doc(name, doc)` directives per CR-D46.

### §8 Error messages with help pointers [HR-D7]

**Definition HR-D7 (Errors point at help).** Every static-analysis
error, parse error, and runtime error includes a help pointer when
one exists. Pointer shapes:

```
error: predicate 'verbs' used with arity 2, expected 4
  --> cli-query:1:3
  help: see signature with `anneal schema verbs`
        or `anneal help verbs`

error: graph primitive 'upstream' requires a bound endpoint argument
  --> cli-query:1:3
  help: see `anneal help upstream` for anchoring requirements
        or the rule CR-R7 in the master spec
```

Errors that name a predicate, verb, syntax construct, or diagnostic
code should always include a `help:` pointer to the relevant `help`
topic or `describe` target. Errors without natural help targets
(internal panics, IO errors) emit no pointer rather than a fake one.

This addresses anneal-qke (introspection arity discoverability) and
anneal-3ye (predicates field permutation) directly: error messages
that show the user *how to get unstuck* eliminate most introspection
arity friction by construction.

---

## Part III: TTY rendering vs machine output [HR-R]

### §9 TTY detection contract [HR-D8]

**Definition HR-D8 (TTY-aware default rendering).** Every status-
or summary-shaped verb (status, context, search, read,
garden, areas, orient, work, blocked, broken, trend) detects whether
stdout is a TTY:

- **TTY** → human-rendered (tables, headings, prose, ANSI styling)
- **Pipe / redirect / non-TTY** → NDJSON (or single JSON for verbs
  with composed output like `context`)

A future `--format` flag can explicitly override detection. v0.11.0
uses the shipped `--json`/`--pretty` output controls instead.

```
anneal context "goal" --format=text       Force human render
anneal context "goal" --format=json       Force structured JSON
anneal context "goal" --format=ndjson     Force NDJSON
```

This addresses anneal-41l (status-shaped output as NDJSON to TTY) and
anneal-jlw (context output JSON-only) — both by establishing one
convention rather than fixing each verb in isolation.

### §10 Pretty-render contract per verb [HR-D9]

**Definition HR-D9 (Per-verb pretty rendering).** Each pretty render
is hand-written for the verb's specific output shape:

- `status`: pipeline histogram + work/blocked/broken/
  advancing groups with score + reason
- `context`: goal as header, hits as ranked list with handle and
  reason, spans rendered inline with `›` indent, neighborhood as
  bullet list
- `search`: table with rank, handle, field, score
- `read`: text with span markers and line numbers
- `describe`: prose, possibly multi-paragraph
- `schema`: aligned table of (kind, name, signature)

Pretty renderers live in `crates/anneal-cli/src/render/<verb>.rs` and
share a small `output::tty` toolkit for tables, headings, ANSI
detection.

### §11 NDJSON contract [HR-D10]

**Definition HR-D10 (NDJSON contract preserved).** TTY rendering is
*additive*; it does not change the pipe behavior. NDJSON output for
every verb is stable, documented, and tested independently of the
pretty render. Agents and downstream tools depend on NDJSON; the
contract for that audience does not change with TTY support.

---

## Part IV: Auto-generated language reference [HR-G]

### §12 The renderer tool [HR-D11]

**Definition HR-D11 (Language reference renderer).** A small Rust
binary at `tools/langref/` reads a configured prelude + optional
project anneal.dl, calls the introspection primitives, joins their
output with hand-written prose chapters, and emits a Markdown file.

```
tools/langref/
├── Cargo.toml
├── src/main.rs               Entry point
├── src/render.rs             Catalog → markdown
├── src/prose/                Hand-written chapters
│   ├── 01-introduction.md
│   ├── 02-syntax.md
│   ├── 03-types.md
│   ├── 04-stratification.md
│   ├── 05-aggregation.md
│   ├── 06-errors.md
│   ├── 07-tutorial.md
│   ├── 08-migration.md
│   └── 09-cookbook.md
└── tests/golden/             Pinned outputs for CI
```

Invocation:

```
anneal-langref --output docs/LANGUAGE.md             # default prelude
anneal-langref --root . --output docs/LANGUAGE.md    # include project anneal.dl
anneal-langref --check                               # verify rendered output matches docs/LANGUAGE.md (CI)
```

### §13 Source-of-truth split [HR-D12]

**Definition HR-D12 (Generated vs hand-written portions).**

| Portion | Source | Generated? |
|---|---|---|
| Predicate catalog (signatures, docs, source_of) | `schema()` + `describe()` + `source_of()` | Yes |
| Verb catalog (name, doc, output_schema, query, capabilities) | `verbs()` + `describe()` | Yes |
| Source/adapter catalog | `sources()` | Yes |
| Stored relation catalog | `schema()` over stored predicates | Yes |
| Engine primitive group descriptions | `describe()` over primitive group names | Yes (with hand-written headings) |
| Syntax reference (grammar, operators, literals) | `prose/02-syntax.md` | No |
| Stratification + aggregation semantics | `prose/04-stratification.md`, `prose/05-aggregation.md` | No |
| Error message catalog (`E001`/`W003`/`I001`/`S004` etc.) | analyzer source enum + hand-written rationale | Semi-auto |
| Tutorial / quick start | `prose/07-tutorial.md` | No |
| Migration notes (v1.x → v0.11+) | `prose/08-migration.md` | No |
| Query cookbook | `prose/09-cookbook.md` (with example validation in CI) | Semi-auto |

The split is intentional: machine-knowable facts are generated;
conceptual explanations are hand-written and reviewed.

### §14 The `@doc` contract [HR-D13]

**Definition HR-D13 (`@doc` is mandatory for public surface).** Every
public predicate, verb, source, and engine primitive registered in
the runtime must carry a `@doc(name, doc)` directive (per CR-D46) or
equivalent Rust-side documentation accessible via `describe()`. CI
enforces this:

```
$ anneal-langref --check-coverage
error: predicate 'foo' has no @doc directive
       (declared at crates/anneal-core/src/prelude/checks.dl:42)
```

The `@doc` body convention:

- First line: one-sentence summary suitable for terminal listing
- Optional second paragraph: longer prose explanation
- Optional `Examples:` block with 1-3 example invocations
- Optional `See also:` references to related predicates

```
@doc(potential, """
Energy a handle accumulates from open entropy sources.

Sum of entropy weights for sources that resolve to this handle.
Higher potential = more work waiting; zero = nothing pending.

Examples:
  ? potential(h, score), score > 5.
  ? *handle{id: h, status: "draft"}, potential(h, score).

See also: entropy, blocked, top_work
""").
```

### §15 Project verbs in the reference [HR-D14]

**Definition HR-D14 (Project langref).** Running `anneal-langref
--root .` against a project with its own `anneal.dl` produces a
language reference that includes:

- The standard prelude (always)
- The project's own predicates, verbs, sources (additionally)
- A "Project Extensions" section clearly delineated

Project verbs go through the same `@doc` validation. A project
anneal.dl with undocumented verbs fails `anneal-langref --check` in
the project's own CI — extending the discipline beyond the anneal
project itself.

### §16 Rendering format [HR-D15]

**Definition HR-D15 (Markdown rendering conventions).** The generated
Markdown follows GitHub-flavored conventions and is designed to
render correctly in:

- GitHub repo browsing
- Local Markdown viewers
- `mdbook` if a project chooses to wrap it
- Terminal Markdown renderers (`glow`, `mdcat`)

Conventions:
- Each predicate is a `### name(args)` h3 heading
- Signatures in fenced code blocks tagged `weave` (or whatever the
  language identifier becomes)
- Cross-references use `[name](#name)` anchor links
- Examples are runnable (CI validates them)
- Source locations are GitHub-style links when a repo URL is configured

---

## Part V: Repo documentation layout [HR-DR]

### §17 The four docs [HR-D16]

**Definition HR-D16 (Repo documentation set).** The repo carries
exactly four user-facing documents in `docs/`:

| File | Audience | Length target |
|---|---|---|
| `docs/LANGUAGE.md` | Project authors, query writers, agents | 800-1500 lines |
| `docs/CORPUS-GUIDE.md` | Anyone using anneal on their corpus | 400-800 lines |
| `docs/ADAPTER-GUIDE.md` | Adapter authors (v2.1+) | 300-500 lines |
| `docs/QUERIES.md` | Query cookbook with running examples | 200-400 lines |

The repo also keeps `README.md` (short tool description + install +
first commands) and `CHANGELOG.md` (per release).

### §18 Single source of truth per concept [HR-D17]

**Definition HR-D17 (No duplication between in-binary and repo docs).**
Each piece of information lives in exactly one canonical place:

- Predicate semantics → `@doc` in prelude `.dl` files
- Verb semantics → `@doc` in prelude `.dl` files
- Grammar / syntax → `prose/02-syntax.md` (rendered into LANGUAGE.md
  and surfaced via `anneal help syntax`)
- Error code rationale → `prose/06-errors.md` (rendered into
  LANGUAGE.md and surfaced via `anneal help E001`)
- Quick start tutorial → `prose/07-tutorial.md` (rendered into
  LANGUAGE.md and a section of README.md)
- Adapter authoring → `prose/adapter-guide.md` (rendered into
  ADAPTER-GUIDE.md and surfaced via `anneal help adapter`)

The renderer is the single tool that knows how to project each prose
or generated section into each surface. Drift between in-binary help
and repo docs becomes impossible.

---

## Part VI: CI gates [HR-CI]

### §19 Required CI checks [HR-D18]

**Definition HR-D18 (CI gates).** The langref subsystem adds the
following CI checks to `just check`:

1. **`@doc` coverage** — every public predicate, verb, source must
   have a `@doc` directive. Failure lists undocumented names with
   source locations.

2. **Render idempotency** — running `anneal-langref` produces the
   same output bytes as the committed `docs/LANGUAGE.md`. Diff = fail.

3. **Cross-reference validity** — every `See also: name` and every
   `[name](#anchor)` link must resolve. Broken links = fail.

4. **Example validity** — every example in `@doc` `Examples:` blocks
   and in `prose/09-cookbook.md` parses and analyzes successfully
   (does not require execution; analysis-only).

5. **Help-topic coverage** — every topic referenced by an error
   message's `help:` pointer must exist in the topic registry.

6. **TTY render smoke** — for each pretty-render-capable verb,
   `<verb> --help` parses + produces non-empty output; render
   functions execute against frozen fixtures without panic.

### §20 Acceptance criteria [HR-D19]

**Definition HR-D19 (Acceptance).** The full help-and-reference
subsystem ships when:

1. `anneal help <topic>` works for: syntax, aggregation,
   stratification, capabilities, convergence, plus every diagnostic
   code in checks.dl (E001..S005)
2. `anneal --help` lists every command including runtime verbs
   (already done in 653f442)
3. `anneal <verb> --help` works for every verb (already done)
4. `anneal describe <name>` returns prose for every public predicate
   and verb (gap today; needs `@doc` coverage pass)
5. TTY rendering works for at minimum: `status`, `context`,
   `search`, `garden`
6. `anneal-langref --check` is part of `just check`
7. `docs/LANGUAGE.md` is rendered and committed
8. `docs/CORPUS-GUIDE.md` is hand-written
9. Every static-analysis and parse error includes a help pointer when
   one applies
10. Cold-agent gate: a fresh agent can answer "what predicates exist
    that have to do with X?" in ≤2 tool calls using only the in-binary
    help surface

---

## Part VII: Implementation phases [HR-P]

### §21 Phase A — `@doc` coverage pass [HR-D20]

**Definition HR-D20 (Phase A: documentation pass).** Land `@doc`
directives for every public predicate, verb, source, and engine
primitive group in the prelude. Quality bar: first line is one-
sentence summary; multi-paragraph for non-obvious cases; examples
for the load-bearing predicates (potential, blocked, entropy, work,
context, search, read).

Estimated scope: 80-120 `@doc` blocks, mostly short. Naturally fits
during the polish burn-down currently in flight.

### §22 Phase B — Renderer tool [HR-D21]

**Definition HR-D21 (Phase B: renderer).** Build `tools/langref/`:
introspection primitive calls, catalog assembly, prose loading,
Markdown emission, golden tests, `--check` mode.

Slice acceptance:
- Generates a valid Markdown file
- Output is idempotent
- `--check` mode passes when committed `docs/LANGUAGE.md` matches
  fresh rendering

### §23 Phase C — Topic help [HR-D22]

**Definition HR-D22 (Phase C: topic help registry).** Implement
`anneal help <topic>` with embedded markdown topic files. Each topic
file is also a section of `docs/LANGUAGE.md`.

Initial topic set: syntax, aggregation, stratification, capabilities,
convergence, E001..S005 diagnostic codes, error-pointer-referenced
topics.

### §24 Phase D — TTY rendering [HR-D23]

**Definition HR-D23 (Phase D: TTY rendering layer).** Wire
`output::tty` (or extend the existing legacy output module) to detect
TTY and render per-verb pretty output. Land per-verb renderers for:
anneal, context, search, status, garden first (highest cold-agent
impact per Phase 11 feedback). Other verbs follow.

Slice acceptance:
- Each landed verb renders cleanly at TTY
- NDJSON output unchanged for pipes
- `--format=text|json|ndjson` override works
- Cold-agent smoke against `.design` corpus passes

### §25 Phase E — Error pointers [HR-D24]

**Definition HR-D24 (Phase E: error help pointers).** Extend every
StaticError variant, parser error, and runtime error to include a
`help:` pointer when one applies. Topics referenced must exist
(enforced by HR-D18 check 5).

### §26 Phase F — Repo docs [HR-D25]

**Definition HR-D25 (Phase F: repo documentation).** Land the
hand-written `prose/*.md` chapters and render the four `docs/*.md`
files. The hardest single chapter is `prose/06-errors.md` because it
needs cohesion across the entire diagnostic catalog. The easiest is
`prose/01-introduction.md` (mostly already in README).

---

## Part VIII: Open questions [HR-OQ]

### §27 Should `anneal help` default to a topic index when called bare?

`anneal help` with no argument could either error with usage, or
print a topic index ("Available topics: syntax, aggregation, ...").
The latter is more discoverable; the former is more conventional.

### §28 Should generated docs commit to the repo or be CI-only?

Committing `docs/LANGUAGE.md` means PR diffs show the user-facing
impact of every code change. Not committing means CI gates work but
GitHub readers have to build to see the docs. Commit-and-gate is
probably right (CI fails if generated content drifts from committed).

### §29 Should `@doc` support Markdown formatting in its body?

Pro: richer prose, code blocks, links. Con: makes `describe()` output
either render Markdown at TTY (more work) or expose Markdown markup
literally (ugly). Probably yes-with-restraint: support bold/italic/
code spans/links but no headings or lists in `@doc` bodies; headings
come from the renderer's section structure.

### §30 Should error help pointers be URLs or topic names?

`help: https://anneal.dev/docs/topics/aggregation` vs
`help: anneal help aggregation`. URLs are richer; topic names work
offline and inside the binary. Probably both: topic name primary,
optional URL when configured (`[help] doc_url_base = ...` in
anneal.toml).

### §31 Editor / LSP consumption of introspection primitives

The same introspection primitives that drive the langref could feed
an LSP server: hovers from `describe`, jump-to-definition from
`source_of`, completions from `schema`/`predicates`/`verbs`. This is
v2.1+ work but the contract should be designed to support it. See
CR-R9 (anneal-lang stabilization gate).

### §32 How do `vocab` (per anneal-dc6) and `predicates` relate?

`anneal vocab` would surface project-configured values (status
partition, edge kinds, namespaces). `anneal schema`/`schema(...)`
surfaces language-level predicate signatures; a future
`anneal predicates` could be a thinner predicate catalog projection.
They're different axes (data vs metadata). Help text distinguishes
them before either follow-up CLI surface ships.

### §33 Versioning of the language reference

Each release ships a snapshot of `docs/LANGUAGE.md`. Older versions
remain in git history. Live docs (e.g., `anneal.dev/docs`) probably
render from the current main tag. Per-version archived docs are a
v2.1+ concern.

---

## Part IX: Labels [HR-Labels]

### HR-F (Framing)

- [HR-F](#part-i-why-this-exists-hr-f) — Why the help/reference
  subsystem exists.

### HR-D (Decisions)

- HR-D1: Self-describing runtime (§1)
- HR-D2: Progressive help layers (§3)
- HR-D3: Top-level help discipline (§4)
- HR-D4: Per-verb help (§5)
- HR-D5: Topic help (§6)
- HR-D6: Runtime semantic introspection (§7)
- HR-D7: Errors point at help (§8)
- HR-D8: TTY-aware default rendering (§9)
- HR-D9: Per-verb pretty rendering (§10)
- HR-D10: NDJSON contract preserved (§11)
- HR-D11: Language reference renderer (§12)
- HR-D12: Generated vs hand-written portions (§13)
- HR-D13: `@doc` is mandatory for public surface (§14)
- HR-D14: Project langref (§15)
- HR-D15: Markdown rendering conventions (§16)
- HR-D16: Repo documentation set (§17)
- HR-D17: No duplication between in-binary and repo docs (§18)
- HR-D18: CI gates (§19)
- HR-D19: Acceptance (§20)
- HR-D20: Phase A — documentation pass (§21)
- HR-D21: Phase B — renderer (§22)
- HR-D22: Phase C — topic help registry (§23)
- HR-D23: Phase D — TTY rendering layer (§24)
- HR-D24: Phase E — error help pointers (§25)
- HR-D25: Phase F — repo documentation (§26)

### HR-OQ (Open questions)

- HR-OQ §27: Default behavior of bare `anneal help`
- HR-OQ §28: Commit generated docs to repo or CI-only
- HR-OQ §29: Markdown formatting in `@doc` bodies
- HR-OQ §30: Error help pointers as URLs or topic names
- HR-OQ §31: LSP consumption of introspection primitives
- HR-OQ §32: Relationship between `vocab` and `predicates`
- HR-OQ §33: Versioning of language reference

---

## Part X: Cross-references

- Builds on CR-D44 (Introspection tuple encoding, master spec §43)
- Builds on CR-D46 (Documentation declarations, master spec §43)
- Addresses bd issues anneal-qke (arity discoverability), anneal-3ye
  (predicates field permutation), anneal-dc6 (vocab verb), anneal-3zh
  (status footer discoverability), anneal-41l (TTY render), anneal-jlw
  (context human render), anneal-359 (subcommand --help — done), and
  anneal-wcv (--help missing runtime commands — done)
- Connects to CR-R9 (Language API stabilization gate, master spec
  §8.1) — the langref may eventually become a true parser-only
  consumer, but runtime introspection alone does not satisfy the
  second-consumer gate
- Companion to the design review at
  `.design/2026-05-16-design-review.md` which flagged the
  introspection primitives as both an agent UX feature and a
  documentation system at once.
