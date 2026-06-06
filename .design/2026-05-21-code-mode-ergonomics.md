---
status: superseded
superseded-by: 2026-05-26-surface-evolution-framework.md
updated: 2026-05-26
author: claude + codex (independent review, converged)
depends-on:
  - 2026-05-13-corpus-runtime.md
  - 2026-05-19-compatibility-surface-retire-audit.md
  - 2026-05-20-flag-audit.md
  - 2026-05-20-datalog-learning-path.md
  - 2026-05-21-multi-arg-ergonomics.md
description: >
  Historical v0.12 arc spec. Superseded by the surface evolution
  framework (via the 2026-05-25/26 remove-focused audits), which kept
  relation-pattern calls and describe Common joins but retired the
  cookbook/save teaching surfaces.
  After the v0.10-era compatibility-surface audit closed
  via v0.11.2, two real-world observations from agent usage surfaced a
  deeper question: does the runtime invite composition, or does it just
  permit it? This spec resolves that with relation-pattern call syntax,
  signature unification, D1 naming, and a teaching/cookbook layer.
  Honest framing: parts of the machinery already exist; the work is
  field-pattern composition with omitted fields, durable signatures for
  constant-headed predicates, and the surface that teaches when to reach
  for the language.
---

# Code Mode Ergonomics — v0.12 Arc — 2026-05-21

## Why this exists

The compatibility-surface audit (v0.11.0 → v0.11.2) closed eleven findings
across the help, describe, status, area, search-ranking, and flag-dialect
surfaces. Agents arriving cold can now orient via `status` / `context` /
`prime`, discover vocabulary via `schema` / `describe` / `verbs` / `vocab`,
and compose precise queries via `anneal -e`. The "magic words" complaint
from 2026-05-20 closed as a first-pass problem.

Two observations from real-world agent usage then surfaced the next layer:

**Observation 1.** Agents heavily use `anneal check --area=language` in the
wild, despite the m8wf demotion of the global filter dialect. Muscle memory
plus ergonomic gravity: one command, one knob, scoped diagnostic output.

**Observation 2.** The post-edit "did I break it" workflow has no clean
runtime equivalent. `anneal broken` shows errors only with no file filter;
the eval form `? diagnostic(code, severity, h, file, line, ev), file = "X".`
is six predicates plus a literal comparison instead of one flag.

These could be patched with typed filter args on runtime verbs:
`anneal broken --area=NAME --code=E001 --severity=error`. **That path is a
Code Mode retreat.** Each common workflow accrues a flag. Filter args do not
compose (an agent asking "errors and warnings in the language area touching
files modified in the last 14 days" still drops to eval). The Code Mode
promise that verbs are templates rather than walls quietly weakens.

The strategic alternative: make eval composition genuinely first-class so
that agents *prefer* it. The eval form for the first observation is verbose
because derived predicates like `diagnostic` lack the named-field affordance
that stored relations have. Fix that, and the same workflow becomes
`anneal -e '? diagnostic{subject: h}, area_of{h: h, area: "language"}.'`
— shorter than the would-be flag chain, composable, and a natural
extension of the syntax agents already know from `*handle{...}`.

This spec resolves the asymmetry. It also folds in the long-pending D1
naming triangle (check / diagnostics / broken) from the converged audit
doc and adds a teaching/save-as-verb layer that makes the Code Mode story
concretely true at the workflow level, not just the spec level.

## Strategic frame

Two research-graph claims ground the call:

- **ACI actions should be simple and minimally-optioned to reduce agent
  error rates** — argues against option accretion on the surface.
- **Notation shapes thought, not merely expresses it** (Iverson's executable
  notation lineage) — argues notation should make composition the natural
  move, not the verbose escape.

Both point the same way. Typed filter flags teach agents to wait for the
tool to bless each workflow. Relation patterns teach agents to compose.
The right investment is in the language.

## The keystone: relation-pattern calls

### Implementation finding (what already works)

Master already parses and evaluates named call-site arguments for derived
predicates and primitives. The asymmetry is finer-grained than initially
framed:

- **Primitives** with declared named signatures: named calls work today.
  `search(query: "X", handle: h, span_id: s, score: c, reason: r, field: f, low_confidence: low)`
  evaluates cleanly. Same for `upstream(h: "X", anc: a)`, `read`, etc.
- **Derived predicates with consistent rule heads** (e.g., `top_work(h, energy)`):
  signature inferred from head; named calls work.
- **Derived predicates with literal positions or inconsistent rule head
  variable names** (e.g., `diagnostic`, where 12 rules each start with a
  literal code and severity string, with subject-position variables named
  variously `src` / `h` / `status` / `namespace`): the analyzer's
  `head_parameter_names` returns `Unknown`, the merge across rules returns
  `Ambiguous`, and named calls fail with `named call to 'diagnostic'
  requires a named predicate signature` — even though `anneal describe
  diagnostic` and `anneal schema` both display the canonical signature
  `diagnostic(code, severity, subject, file, line, evidence)`.

**Plus:** named calls today require **every argument**. Omitted fields
fail with "missing argument" errors. There is no way to write
`diagnostic(code: c)` and accept any subject/file/line/evidence.

So the missing surface is not basic named args. It is **field-pattern
calls with omitted fields plus durable signatures for constant-headed
derived predicates**, applied uniformly to primitives and derived
predicates.

### Syntax: relation-pattern calls use braces

A new syntactic form is introduced for **pattern-style calls** that allow
omitted fields:

```dl
diagnostic{code: "E001", file: file, subject: h}.
search{query: "conformance", handle: h, score: score, low_confidence: false}.
area_health{area: area, grade: grade}.
upstream{h: "formal-model/v17.md", anc: anc}.
```

Brace syntax is chosen deliberately:

- **Symmetric with stored relations.** `*handle{id: h, kind: "file"}` and
  `diagnostic{code: "E001"}` share visual category. The `*` prefix
  continues to distinguish stored from derived/primitive, but the field
  affordance is the same: named, omittable, intent-visible.
- **Keeps positional `p(x, y)` and exact-named `p(name: x, other: y)`
  forms intact.** Parens continue to mean a complete call. Braces mean
  pattern call. Two clear surfaces, not one overloaded surface.
- **Square brackets `p[...]` were considered and rejected.** Visually they
  read as indexing or array literals, introduce a new category without
  reusing the existing stored-relation affordance, and parse-conflict
  with list expression syntax in some grammars.

### Rules

1. **Inside braces, every term is name-tagged.** `predicate{name: term}`.
   Positional terms are not allowed inside braces.
2. **Omitted fields become hidden anonymous wildcards.** They do not leak
   as output columns (no `_anon123` rows). Projection semantics treat the
   omitted column as if a fresh unbound variable were introduced internally.
3. **Unknown field errors mirror stored-relation behavior.** "Unknown field
   'X' for 'diagnostic'; expected one of: code, severity, subject, file,
   line, evidence." Matches the `*handle{...}` error shape.
4. **Repeated named fields error.** `diagnostic{code: x, code: y}` is a
   static-analysis error with a source span.
5. **Pattern calls work uniformly on primitives, derived predicates, and
   stored relations** — the stored-relation `*` prefix continues to
   indicate engine-populated facts, but the field syntax is the same.

### `_` wildcard

A literal `_` is accepted as a positional wildcard inside paren calls:

```dl
diagnostic(_, _, h, _, _, _).        # accepted
diagnostic("E001", _, h, _, _, _).   # accepted
```

This is secondary to relation-pattern calls (which make `_` mostly
unnecessary) but is retained for:

- Positional users coming from other Datalog dialects
- Generated/programmatic queries where positional is cheaper than
  field-name lookup
- Cases where a user genuinely wants positional with one or two
  don't-cares

### Examples (before/after)

**Filter diagnostics by area** (Observation 1):

```dl
# before
anneal -e '? diagnostic(code, severity, h, file, line, ev), area_of(h, "language").'

# after
anneal -e '? diagnostic{subject: h}, area_of{h: h, area: "language"}.'
```

**Post-edit check on one file** (Observation 2):

```dl
# before
anneal -e '? diagnostic(code, severity, subject, file, line, ev), file = "document.md".'

# after
anneal -e '? diagnostic{file: "document.md"}.'
```

**Errors only, by area:**

```dl
# before
anneal -e '? diagnostic(code, "error", h, file, line, ev), area_of(h, "language").'

# after
anneal -e '? diagnostic{severity: "error", subject: h}, area_of{h: h, area: "language"}.'
```

**Hub handles with broken refs** (composite that flag args cannot
express):

```dl
anneal -e '? diagnostic{code: "E001", subject: h}, hub(h), recent{h: h, days: d}, d < 14.'
```

The eval form is shorter than the equivalent flag chain in every case,
**and** composes with other predicates without escape-to-eval.

## Slice plan

### Slice 1 — Relation-pattern calls (keystone)

**Scope:**

- Parser: accept `predicate{name: term, ...}` syntax for primitives,
  derived predicates, and stored relations (the stored case already
  works; this confirms uniformity).
- AST: a pattern-call variant carries (predicate, named-fields) and is
  lowered during analysis.
- Analyzer: a unified **signature registry** (see Decision below) maps
  every predicate to its canonical parameter names. Pattern calls
  normalize against this registry; omitted fields lower to hidden
  anonymous wildcards (not visible in output).
- Language metadata: add `@predicate(name: ..., args: [...])` for
  explicit predicate signatures when rule heads cannot provide stable
  parameter names.
- `_` positional wildcard accepted inside paren calls.
- Unknown field, duplicate field, and mixed-paren-with-brace errors mirror
  the stored-relation error shapes.
- Tests: paren positional, paren named, brace pattern with omissions, `_`
  wildcards, error paths.
- No new top-level CLI commands. No flag changes.

**Gate:**

- Both observations expressible cleanly. `area` is not a `diagnostic`
  field, so the area-filtered case uses the `area_of` join:
  `anneal -e '? diagnostic{subject: h}, area_of{h: h, area: "language"}.'` returns rows.
  `anneal -e '? diagnostic{file: "document.md"}.'` returns rows.
- Pattern calls work on every predicate listed by `anneal schema` whose
  signature is registered — explicit metadata present, or unambiguous
  rule-head inference succeeded. Predicates falling through to `Unknown` /
  `Ambiguous` error with a clear hint pointing at the explicit-metadata
  path (CR-D98).
- Tests cover all error paths.
- `just check` green.

### Slice 2 — D1 naming triangle resolution

**Scope:**

The converged compatibility audit (CR-D conversation) agreed on the
shape but the work was never landed. Resolves now:

- **`diagnostics`** is the underlying runtime verb name. Returns all
  severities (errors, warnings, suggestions, info). Maps to
  `? diagnostic{...}.` under the hood.
- **`broken`** stays as the error-only convenience view. Internally
  `diagnostic{severity: "error", ...}`.
- **`check`** becomes a hidden gate-oriented alias for
  `diagnostics --gate` (exits 1 if any error-severity diagnostic exists).
  Useful for CI / pre-commit. Stays callable but does not appear in
  default help.

**No typed filter args.** No `--area`, `--code`, `--severity` flags on
`diagnostics` / `broken`. Composition is via eval pattern calls. The verb
identity parameter rule (e.g., `blocked HANDLE`, `read HANDLE`,
`handle HANDLE`, potentially `area NAME`) stays — those are verb
arguments, not workflow filters.

**Gate:**

- `anneal diagnostics` returns the full diagnostic set.
- `anneal diagnostics --gate` exits 1 if errors present, 0 otherwise.
- `anneal check` is an alias for `anneal diagnostics --gate` (hidden from
  default help).
- `anneal broken` continues to return errors only.
- `describe` and `anneal help eval` teach the `diagnostic{...}` pattern
  composition for filtered cases. (Full cookbook recipes land in Slice 3.)

### Slice 3 — Describe + cookbook learning pass

**Scope:**

- **`describe`** cards extended with a **Common joins** field that lists
  the canonical join patterns for each predicate. Example for `diagnostic`:
  "Common joins: `diagnostic{subject: h}, area_of{h: h, area: \"X\"}` for
  area filtering; `diagnostic{subject: h}, *handle{id: h, kind: \"file\"}`
  for kind filtering."
- **`anneal cookbook`** as a new runtime command. Lists curated common-
  question recipes by question shape. Each recipe is:
  - Question (plain English): "How do I find broken references in one
    area?"
  - Eval query (pattern-call form)
  - One-line explanation of what each predicate contributes
  - When to reach for it
- Cookbook content is part of the prelude, extensible via project
  `@cookbook(name: "...", question: "...", query: "...", doc: "...")`
  declarations in `anneal.dl`.
- describe and help eval are updated to reference the cookbook
  ("`anneal cookbook` for worked recipes by question").

**Gate:**

- `anneal cookbook` lists at least the canonical recipes for: diagnostics
  by file, diagnostics by area, blocked work by area, OQ-namespace
  obligations not discharged, handles citing X, freshness above threshold.
- describe cards show Common joins for at least: diagnostic, search,
  upstream, downstream, top_work, blocked, entropy, undischarged.

### Slice 4 — `anneal save` (save-as-verb)

**Scope:**

- New command `anneal save <name> '<query>' --args '<name:Type>,...' --doc '...'`.
- Writes a `@verb` declaration to `anneal.dl` with the given query, args,
  and doc.
- Validates the query parses, the args match `verb_arg` references in the
  query, and the verb name does not conflict with prelude verbs unless
  `--force` is passed.
- If a declared arg name appears as a variable in the final query body and the
  query does not already bind it through `verb_arg("name", name)`, `save`
  injects that binding into the generated `@verb.query`. More complex local-rule
  shapes can still write `verb_arg(...)` explicitly.
- After save, the verb is callable as `anneal <name>` and shows up in
  `anneal verbs` / `anneal describe <name>` / `anneal help <name>`.

**Gate:**

- Agent workflow: write an eval query, validate it, then
  `anneal save my-area-check '<query>' --args 'area:String' --doc '...'`.
- Subsequent invocation: `anneal my-area-check language` works
  (typed verb args, going through the same dispatch as project verbs).
- Project verbs persist across sessions; `anneal verbs` shows them
  alongside prelude verbs (Steele's criterion).

## Decisions

**CR-D97 (Relation-pattern call syntax).** Pattern-style calls on
predicates use brace syntax: `predicate{name: term, ...}`. Omitted fields
are hidden anonymous wildcards and do not leak as output columns. Brace
syntax applies uniformly to primitives, derived predicates, and stored
relations (with the stored-relation `*` prefix retained as the
engine-populated marker). Paren syntax `predicate(x, y)` and
`predicate(name: x, other: y)` continue to mean complete-call positional
and complete-call named, respectively. The `_` positional wildcard is
accepted inside paren calls for backward compatibility and generated
queries. Rationale: symmetric with stored relations, additive to the
existing grammar, no overload of paren semantics, no new visual category.

**CR-D98 (Predicate signature registry).** A unified internal signature
registry maps every predicate to its canonical parameter names. Sources,
in precedence order:

1. **Explicit metadata** via `@predicate(name: ..., args: [...])`.
   Authoritative when present. Required for constant-headed predicates
   like `diagnostic` whose rule heads contain literals. `@doc` remains
   teaching prose; signatures are executable language metadata.
2. **Primitive signatures** declared in the runtime
   (`crates/anneal-core/src/runtime/primitives.rs`). Already populated.
3. **Rule head inspection** when all rules for a predicate agree on
   variable names in head terms. Already implemented via
   `head_parameter_names` + merge.
4. **`Unknown` / `Ambiguous` fallback** for predicates whose heads contain
   only literals and whose explicit metadata is absent. Pattern calls
   against such predicates error with a clear recovery hint pointing at
   the explicit-metadata path.

`anneal schema` and `anneal describe` consume the same registry as
analysis. The discrepancy where introspection shows names but analysis
rejects named calls (today's `diagnostic` failure) is closed by
construction.

**CR-D99 (Diagnostic verb naming).** `diagnostics` is the canonical
runtime verb for the full diagnostic set. `broken` is the error-only
saved view. `check` is the hidden gate alias `diagnostics --gate`. No
`--area`/`--code`/`--severity`/`--file` typed flags. Filtering is via
eval pattern calls. Resolves D1 from the converged compatibility audit.

## Resolved questions (codex review, 2026-05-21)

- **Q1 → `@predicate` declaration, not `@doc(args:)`.** Signatures are
  executable language metadata; docs are teaching prose. They share
  surface (introspection) but not contract. A new `@predicate(name: ...,
  args: [...])` annotation keeps the boundary clean and lets `@doc` stay
  prose-only.
- **Q2 → No comparisons inside braces in v0.12.** Braces mean named
  equality/pattern only — `predicate{field: term}` binds or filters by
  equality with `term`. Comparisons (`l > 100`, range, regex, etc.) stay
  as body atoms in the surrounding query. Keeps brace semantics tight
  and parsing unambiguous. Future versions may revisit if evidence
  accumulates.
- **Q3 → Project `@cookbook(...)` in `anneal.dl`.** First version keeps
  the extension path inside the language. Loose `.cookbook.dl` files are
  deferred until cookbook volume evidence demands separate files.
  Consistent with how `@verb` and `@doc` already live in `anneal.dl`.
- **Q4 → Require `--force` to override verb-name collisions.** A project
  verb that collides with a prelude verb (by name) errors at load with
  the conflicting source locations clearly shown, unless `--force` is
  passed. Silent override is wrong: it teaches projects that they can
  invisibly redefine prelude semantics, which breaks the cold-agent
  expectation that prelude vocabulary is stable.

## What this is NOT

- **Not "extend named-field syntax from scratch."** The parser, AST, and
  analyzer already handle named call-site args for predicates whose
  signatures are registered. The work is field-pattern composition with
  omissions plus signature unification for constant-headed predicates.
- **Not "add typed filter flags to runtime verbs."** That path is the
  Code Mode retreat the strategic frame argues against. Verbs continue
  to take identity arguments where appropriate (`blocked HANDLE`,
  `read HANDLE`, potentially `area NAME`) and filter via eval pattern
  calls.
- **Not a syntax overhaul.** Paren positional and paren named calls
  continue to work unchanged. Pattern brace calls are additive.
- **Not a v0.11.3 polish slice.** This is a real investment that wants
  its own release cycle.

## Migration impact

- **No breaking changes.** Paren positional and paren named calls work
  exactly as before. Pattern brace calls are new and additive.
- **Behavior change**: `anneal check` becomes a hidden alias for
  `anneal diagnostics --gate`. The compat command continues to work; its
  exit-code-1-on-errors semantics is preserved. CI scripts using
  `anneal check` continue to work unchanged.
- **New surface**: `anneal diagnostics`, `anneal cookbook`, `anneal save`,
  plus pattern-brace syntax.

## Cost estimate

Codex revised estimate after parser/analyzer investigation:

- **Slice 1 (relation-pattern + signature registry):** 3-5 days. Parser
  changes are localized (AST already handles named-call CallArg variants;
  brace form is parallel). Signature registry is the harder piece —
  projection semantics for omitted fields, error paths matching
  stored-relation shapes, explicit metadata declaration form, registry
  population from three sources with precedence. Tests across all three
  predicate kinds.
- **Slice 2 (D1 naming):** 1-2 days. Mostly aliasing `diagnostics` to
  the existing diagnostic-set machinery, marking `check` as hidden alias,
  adding `--gate` flag semantics.
- **Slice 3 (describe joins + cookbook):** 3-4 days. The cookbook is
  content + a small new command. describe Common joins is a field
  addition to the existing teaching-card structure.
- **Slice 4 (`anneal save`):** 2-3 days. New command, `anneal.dl`
  write semantics, validation, collision handling.

**Total: ~9-14 days for the full v0.12 arc.** Likely staged across two
release cycles (Slices 1+2 ship together as the keystone; Slices 3+4
land in a follow-up minor).

## Validation

Before shipping each slice:

- **Slice 1**: re-run the multi-arg-ergonomics experiment with a fresh
  sub-agent. Friction patterns from `.design/2026-05-21-multi-arg-ergonomics.md`
  (silent empty on positional swap, `_` rejection, name-elided errors)
  should collapse. Pattern brace calls should be the natural composition
  mode.
- **Slice 2**: agents reaching for `anneal check --area=X` should find
  the diagnostics surface. Cookbook entry teaches the eval pattern. No
  silent broken workflows.
- **Slice 3**: cold-agent learning experiment (`.design/2026-05-20-datalog-learning-path.md`
  shape) should succeed at non-trivial queries in fewer tool calls.
- **Slice 4**: an agent should be able to save a verb, see it in
  `anneal verbs`, and call it in the next session with typed args.

## Sequencing relative to current state

- v0.11.2 just shipped. The audit arc is complete.
- v0.11.3 cycle in flight: ihcl (verb examples in metadata), kra
  (parsed-@verb caching), c99o (cruft triage), s74 (perf profiling), nty
  (section parent_file). All P3 polish. Land these as v0.11.3 first.
- **v0.12 cycle begins after v0.11.3 ships.** Slice 1 first. Slice 2
  bundles into the same release (D1 wants the pattern-call form to be
  the recommended filter path).
- v0.12.1 / v0.12.2 ships Slices 3 and 4 as the teaching/save layer.

## What this delivers

The Code Mode story we have been telling since the language-first
reframe becomes concretely true at the workflow level:

- **Eval composition is the natural form.** Pattern brace calls are
  shorter than the flag chains they replace, and they compose without
  escaping to eval.
- **Derived predicates are first-class.** The stored/derived asymmetry
  closes. Agents do not have to learn that some predicates accept named
  fields and others do not.
- **The teaching surface points at composition, not at flags.** Cookbook
  recipes teach common shapes; describe Common joins teach predicate
  relationships; `anneal save` makes saved verbs concretely true.

This is the difference between "anneal has a powerful eval escape hatch"
and "anneal teaches agents to program the corpus." We have been claiming
the latter since 2026-05-03. v0.12 makes it actually true.
