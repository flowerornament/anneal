---
status: draft
updated: 2026-05-20
author: claude (sub-agent learning experiment)
depends-on:
  - 2026-05-19-compatibility-surface-retire-audit.md
---

# Datalog Learning Path — Cold-Agent Audit — 2026-05-20

A cold-agent simulation: learn enough of anneal's Datalog dialect from in-binary surfaces alone (no master spec, no prelude source, no runtime source) to compose two non-trivial queries. The point is not the queries — it's the trace of what the surface teaches vs. what the agent must guess.

## Summary

Both queries succeeded. **Q1 (warmup)** was composable on the first attempt with two tool calls of orientation (`--help`, `schema`) and one query — total **~5 tool calls** including a follow-up to dedupe via a rule head. **Q2 (harder)** took **~7 tool calls**, all of them productive: `vocab` to confirm the `OQ` namespace exists, `describe` on three primitives (`undischarged`, `obligation`, `discharged`) to disambiguate which "discharged" the corpus actually uses, an `*edge{kind:"Discharges"}` probe to confirm the data shape, and then the rule.

What I had to guess vs. what the surface taught:

- **Surface taught:** the full predicate catalog with signatures (`schema`), the stored-relation field list including order (`schema` signature column), the `*name{...}` named-field syntax with field validation, the `not predicate(...)` negation form, the `head(args) := body, body. ? query.` rule grammar (from `help eval`), and the convention that stored relations get a `*` prefix while derived predicates do not (stated explicitly in `prime` and `help eval`).
- **Had to guess / discover:** that `undischarged` is gated on `obligation(h)` being non-empty (it returned 0 rows on a corpus with no `linear` policy, so I had to fall back to `not *edge{kind:"Discharges", to: h}` directly); that named-field syntax `*handle{id: h}` binds the field to a variable while a literal `kind: "label"` filters; that rule heads dedupe naturally (not stated anywhere in the help — inferred from set semantics).
- **Almost gave up at:** the moment I saw `undischarged(h)` returned zero rows despite there being clearly undischarged labels. Without prior Datalog/SQL intuition for "negation over a stored relation works fine," I would have spent a long time hunting for the "real" undischarged predicate.

I would not have given up on either query if forced to finish, but a less experienced agent — one without prior Datalog or SQL — would plausibly stall at Q2 because `obligation`/`discharged`/`undischarged` form a tight semantic cluster with no worked example showing when each is actually populated.

## Q1 Trace — Files in `.design/` with status="draft" AND at least one outgoing DependsOn edge

### Step 1. `anneal --help`

Result: top-level help is unusually rich for a CLI — it includes a **CORE CONCEPTS** section that defines `Handle`, `Edge` (with the five kinds, including `DependsOn`), `Status`, and `Lattice`, plus a **QUERY EXAMPLES** block with three real one-liners. The very first example was `? *handle{id: h, kind: "file", status: s}.` — which is essentially half of Q1.

Annotation: **clarity high**. The top-level help is doing the work of a tutorial preface, which I did not expect from a normal `--help`. The `*` convention for stored relations is shown by example but is also stated explicitly: "Stored relations use `*` prefixes; prelude and project predicates do not."

### Step 2. `anneal schema --format=text`

Result: 104 entries, each with `name`, `kind` (stored/derived/primitive), `signature` (positional or `name{field, field, ...}`), `determinism`, and `provenance`. I scanned for two things: the `*handle` signature (which told me the field name is `status`, not e.g. `state`), and any predicate that binds outgoing edges. Found `outgoing_edge(h, to, kind)`.

Annotation: **clarity high**. `schema` is the dialect's de-facto grammar reference for atoms. The fact that stored relations show field names in braces (`*handle{corpus, source, ..., id, kind, status, namespace, file, line, date, area, summary}`) and derived predicates show positional names (`outgoing_edge(h, to, kind)`) lets you read the syntax convention straight off the table.

### Step 3. First query attempt

```
anneal --root .design -e '? *handle{id: h, kind: "file", status: "draft"}, outgoing_edge(h, to, "DependsOn").'
```

Result: 5 rows. Three handles appear, but `2026-05-16-help-reference-spec.md` appears twice because it has two outgoing DependsOn edges.

Annotation: query worked first try. **The named-field-with-literal-or-variable convention was learned implicitly** from the `--help` example — `kind: "file"` filters, `id: h` binds. This was not stated anywhere; it had to be inferred from the example. If I'd written `kind = "file"` or `kind == "file"` I would have hit a parse error.

### Step 4. Dedupe via rule head

```
draft_with_dep(h) := *handle{id: h, kind: "file", status: "draft"}, outgoing_edge(h, to, "DependsOn").
? draft_with_dep(h).
```

Result: 4 unique file handles. Done.

Annotation: I knew rule heads dedupe from prior Datalog background. **The surface does not teach this.** `help eval` shows the `head := body` form but doesn't say anything about set semantics. An agent without prior background would either accept the duplicated rows or write a more convoluted query.

## Q2 Trace — OQ-namespace labels in large-corpus not discharged by any handle

### Step 1. `anneal --root /path/to/large-corpus/.design vocab --format=text | grep namespace`

Result: confirmed `OQ` (and `OQ-PR`) namespaces exist. 80 OQ labels (confirmed later).

Annotation: **clarity high**. `vocab` is the right tool. It told me what literal string to put in the namespace filter, which is something the schema alone cannot.

### Step 2. `describe undischarged`, `describe obligation`, `describe discharged`, `describe discharge_count`

Result:
- `undischarged`: "Bind obligations without a discharge edge."
- `obligation`: "Bind handles that are open obligations."
- `discharged`: "Bind obligations with a discharge edge."
- `discharge_count`: "Bind per-handle incoming discharge edge counts."

Annotation: **clarity medium — concept is circular**. All four definitions reference the term "obligation" as if I know what it means. From earlier reading of `prime`, I know `linear([...])` is a project policy that makes a namespace prefix produce obligations — but the `describe` output does not link out to that. The `describe` for `undischarged` should say something like "see `linear()` namespace policy for how a label becomes an obligation."

### Step 3. Probe: `? obligation(h), *handle{id: h, namespace: ns}.`

Result: 0 rows.

Annotation: **this is the moment a less experienced agent would stall**. The schema told me `undischarged` exists and the semantics blurb suggests it answers Q2 verbatim — but the corpus has no `linear` policy configured, so the obligation set is empty and `undischarged` is also empty. **Nothing in the surface flagged this.** I had to know to probe `obligation(h)` directly to discover the prerequisite was unmet. A `vocab` category for "obligation status" or a hint in `describe undischarged` like "requires `linear()` namespace policy" would close this gap.

### Step 4. Pivot: probe the edge table directly

```
? *edge{kind: "Discharges", from: f, to: t}.
```

Result: 5 rows like `f=implementation/research/RQ-06-incremental-recompilation.md t=OQ-6`. So "discharged" in this corpus is operationally "has an incoming `Discharges` edge".

Annotation: this is the workaround. **It worked because the schema's `*edge` signature lists `from, to, kind` as named fields, so I could write the filter both ways.** Without prior SQL/Datalog background, choosing to pivot from a high-level derived predicate down to the stored relation is not obvious — the `prime` briefing emphasizes "use the standard library first" which actually pushed me toward the wrong tool here.

### Step 5. Compose Q2

```
has_discharge(h) := *edge{to: h, kind: "Discharges"}.
oq_undischarged(h) := *handle{id: h, kind: "label", namespace: "OQ"}, not has_discharge(h).
? oq_undischarged(h).
```

Result: 75 rows. Sanity-checked: 80 total OQ labels - 5 with incoming Discharges = 75. Matches.

Annotation: **the negation syntax `not has_discharge(h)` was taught by exactly one line in `help eval`** ("`local_rule(x) := body_atom(x), not excluded(x).`"). That single line was enough. I also tried inline negation `not *edge{to: h, kind: "Discharges"}` and it worked too — but inline negation on stored relations was not in any example.

## Gaps In The Learning Path

These are itemized with evidence from the traces.

1. **No grammar tour.** `help eval` shows a one-line syntax sketch and four examples. There is no place that says: "values are quoted strings or unquoted identifiers (which become variables); commas separate body atoms; `:=` separates head from body; `?` introduces a query; periods terminate clauses; `not` negates an atom." A cold agent infers all of this from the four examples. Evidence: I assumed `kind: "file"` was a filter and `id: h` was a binding from one example; if I'd been wrong I would have had to read source.

2. **Named-field vs. positional convention is example-only.** Stored relations use `name{field: value}`; derived predicates use positional `name(arg1, arg2)`. Stated once in `help eval` syntax (`atom(arg), other_atom(named: value)`) but the example mixes both forms without flagging which is which. The error message when you guess wrong (`expected '{'` for `*handle(id, kind)`) is good but only after you fail.

3. **`describe` is too terse.** Each entry is one sentence. `describe undischarged` says "Bind obligations without a discharge edge" — but the upstream definition of "obligation" requires `linear()` namespace policy that may not be configured. There is no "see also" or "requires" line. Evidence: I almost stalled at Step 3 of Q2.

4. **`examples()` primitive is verb-only.** `examples("outgoing_edge", e)` returns 0 rows. `examples("undischarged", e)` returns 0 rows. Only verbs have examples (24 rows total across 18 verbs). Primitives and derived predicates — the actual building blocks of new queries — have no copy-modifyable examples in the runtime. The `schema` signature column is the only worked guidance, and it shows arity but not realistic literals.

5. **No "starter recipe" surface.** `verbs` lists 18 saved queries, but most are introspection (`schema`, `vocab`, `sources`, etc.) or composite UX commands (`status`, `work`, `context`). A category like "common-shapes" with 5-10 short recipes — "files with status X", "labels in namespace N", "handles with outgoing edge K", "things matching A but not B" — would have eliminated half my orientation calls. Evidence: I composed Q1 in 5 calls but a `recipes` command could have done it in 2.

6. **Negation over stored relations is undocumented.** The `help eval` skeleton shows `not excluded(x)` over a derived predicate. It does not show `not *edge{to: h, kind: "Discharges"}`. Both work, but cold agents have to guess. Evidence: I introduced an intermediate `has_discharge` rule because I wasn't sure inline negation on `*edge{...}` was legal; on a later try it was.

7. **No reverse-lookup from "I want X" to "use primitive Y".** I wanted "labels without a discharge edge". The natural keyword is "discharge". `describe` requires you to know the name already. There is no `find-predicate` or "what answers 'undischarged labels'" surface. Evidence: I had to inspect `schema | grep` style — except I could not, because the schema is one screen of text without ability to query "which predicates take a handle and return something edge-related".

8. **No connective tissue between policy config (`linear`, `rejected`) and runtime predicates that depend on it.** The `prime` text mentions `linear([...])` once. The `schema` lists `obligation` as a primitive. Nothing in the runtime tells you that `obligation` is empty when `linear` is unconfigured. Evidence: Q2 Step 3 — 0 rows from `obligation(h)` on a corpus that obviously has obligations.

9. **`describe handle` collides with the verb name.** `describe handle` returns the verb doc, not the `*handle` relation doc. There is no `describe '*handle'` form documented (and I did not try every quoting variant). Evidence: Step 2 of Q1, the result was the verb description; for `*handle` field semantics I had to rely on the schema signature alone.

10. **No worked rule-construction example beyond the one in `help eval`.** Every example in `--help`, `help eval`, and `prime` is a single `?` query. The only rule-based examples (`head := body. ? head(...).`) live inside verb queries, which are dense and hard to parse as a teaching example. A standalone "this is how you write a multi-line rule" worked example would help.

## What The Introspection Chain Teaches Well

- **`schema` is the killer feature.** A flat, sorted catalog with signatures, kinds, and provenance is exactly the right level of detail for orientation. Field names in braces for stored relations doubles as syntax documentation. I would have failed without it.
- **`vocab` answers "what literal goes in this filter?"** Knowing that the corpus has `OQ` (and not `OQ:` or `OQ-` or `oq`) as a namespace value is exactly what `vocab` provides. This is something a SQL agent would have to discover via `SELECT DISTINCT`; here it is a first-class command.
- **Top-level `--help` includes a CORE CONCEPTS preface.** Most CLIs do not. Defining `Handle`, `Edge` kinds, `Status`, and `Lattice` inline is what made Q1 trivial.
- **Error messages teach.** Three errors I deliberately triggered:
  - Missing `*` prefix → "expected comparison operator" (less helpful — it's a generic parse error)
  - Unknown field → "unknown field 'bogus' for '\*handle'; expected one of: corpus, source, native_id, origin_uri, revision, generation, id, kind, status, namespace, file, line, date, area, summary" (**excellent — teaches the full field list at point of error**)
  - Unknown predicate → "unknown predicate 'unknown_predicate/2'" (acceptable)
  - Wrong arity → "predicate 'outgoing_edge' used with arity 2, expected 3; signature: outgoing_edge(h, to, kind)" (**excellent — restates the signature**)
  - Positional on stored relation → "expected '{'" (terse but enough to point you at the right syntax)
- **`prime` is the right launchpad.** The "First Moves" ladder (arrive → discover → retrieve → ask) maps directly to how the orientation actually went. The "Raw Query Surface" block with common stored relations and prelude families gave me the menu I needed for Q2.

## What An Agent With General SQL/Datalog Background Still Needs That Anneal Does Not Provide

- **The set-semantics of rule heads.** That a rule head deduplicates is a Datalog universal but is nowhere stated. SQL agents will not assume this.
- **The aggregation surface (or lack of it).** I never needed aggregation, but Q1's task says "list of file handles" and ended with rule-head dedupe. If I had needed `count(distinct h)`, the surface offers nothing about it. The schema has `s003_pipeline_stall(status, count, ...)` and `namespace_label_count(namespace, count)` which suggests counts exist as derived predicates, but no example shows how a user composes their own aggregation.
- **The stratification rules for negation.** `not has_discharge(h)` worked. Would `not derived_rule_with_self_reference(h)` work? The surface is silent. Datalog dialects vary widely here.
- **How to read provenance / debug a derivation.** `--explain` is mentioned in `help eval` but no example shows what its output looks like or how to read it. Evidence: I never reached for it because the surface did not suggest when it would help.
- **Where `verb_arg(...)` comes from in verb queries.** Reading the `verbs --format=text` output, the `verb_arg("h", h)` pattern is everywhere, but `describe verb_arg` returns nothing useful, and there is no schema entry for it. This made the verb examples partially opaque as templates.

## Recommendations (ordered by impact)

1. **Add `anneal recipes` (or `examples` for non-verbs).** Five to ten short, copy-paste-ready Datalog snippets covering: "handles by namespace", "files with status X", "outgoing edges of kind K", "things matching A but not B", "namespace counts". This single addition would compress orientation for Q-like questions from ~5 tool calls to ~2.
2. **Extend `describe` with a "requires" or "see also" line.** Especially for `undischarged`, `obligation`, `advancing`, `blocked`, and other predicates whose populated-ness depends on project config. Format: "Requires: `linear()` namespace policy in anneal.dl." This would have un-stalled Q2 at Step 3.
3. **Add a grammar reference page.** `anneal help eval` could have a second page (`anneal help eval --grammar`?) that lists: terminators, comments, value forms (strings/numbers/identifiers/booleans), named-vs-positional rules, negation rules including stratification, comparison operators, rule head dedup semantics, and the `verb_arg` convention.
4. **Disambiguate `describe NAME` when name collides with both verb and stored relation.** `describe handle` returns only the verb. It should return both with a header, or accept `describe '*handle'` / `describe 'handle/relation'` to disambiguate.
5. **Backfill `examples()` for primitives and stored relations.** Today, only 18 verbs populate the `examples` primitive. Even one example per primitive (e.g. `? outgoing_edge("formal-model/v17.md", t, k).`) would let agents copy-modify their way to a working query.
6. **Add a "predicate finder" or keyword search over `schema`.** `anneal schema --search discharge` would have shown me `obligation`, `undischarged`, `discharged`, `discharge_count`, and `*edge.kind=Discharges` together. Without it I had to scroll a 104-line catalog.
7. **Add `--explain` worked output to `help eval`.** A two-line example showing what `--explain` prints would surface this debugging tool at the right moment.
8. **Improve the "missing `*` prefix" parse error.** Today it says "expected comparison operator". Change to: "unknown atom `handle`; did you mean stored relation `*handle`? Stored relations require a `*` prefix." This is the single most likely first-mistake error and the easiest to teach.
