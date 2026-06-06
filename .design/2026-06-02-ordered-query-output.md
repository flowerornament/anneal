---
status: locked
date: 2026-06-02
issue: anneal-9pfj
authors: [claude]
reviewers: [codex]
relates: [.design/2026-06-01-perf-architecture-arc.md]
---

# Ordered query output — `order by` as a projection-boundary primitive — 2026-06-02

## The frustration (dogfooded, murail, 2026-06-02)

The v0.15.1 orientation ladder promises two reading lists. Both lie about order:

```
? recent_frontier(h, rank, recency), rank <= 12.   # "most-recent reading list"
 5. h=…/v18-updates.md         rank=2 recency=1     # the MOST recent — buried at row 5
 1–4. four rank=9 rows first                        # alphabetical by path, not ranked

? ranked_anchor(h, rank, score, why), rank <= 8.   # "the durable spine, in order"
 6. h=…/murail-formal-model-v17.md  rank=1 score=303  # THE anchor — at row 6
 1. h=DESIGN-GOALS.md              rank=7             # a cold agent reads this first
```

The `rank` column is correct; the **row order is alphabetical by handle**. A cold
agent told "here is your reading list" must scan a column and mentally re-sort. The
promise of the flagship cold-start surface is broken at the moment it's trusted most.

## Root

The dialect has no way to declare output order. Results emit in binding order
(lexical, BTreeMap-driven). `rank <= N` *selects* the top-N but cannot *order*
them. `--limit` then `output.rows.truncate(limit)`s those lexically-ordered rows —
so `--limit` isn't even a real top-N; it's "N arbitrary survivors."

This is a general language gap, not orientation-specific: any ranked predicate an
agent composes hits it.

## Decision: `order by` as a first-class query clause (Option A)

Add an ordering clause to the top-level `?` query, applied at the **projection
boundary** — after the fixpoint, over the final result set, before `--limit`.

```
? <body> [ order by <expr> [asc|desc] [, <expr> [asc|desc]]* ] .
```

Examples that make the ladder read top-down:

```
? recent_frontier(h, rank, recency) order by rank asc.
? ranked_anchor(h, rank, score, why) order by score desc.
? *handle{id: h, status: s}, advancing(h) order by s asc, h asc.
```

### Why this surface

- **Agents learn the Datalog** (Morgan's standing constraint): ordering is a
  language primitive any query can use, not a CLI flag or a baked-in verb. The
  `status` dashboard pointer queries gain `order by`, which *teaches the primitive
  by example* — the same change that fixes the flagship surface documents it.
- **Projection-boundary, never in rules.** Rules derive *sets*; order is
  meaningless inside a set and would corrupt fixpoint/stratification. `order by`
  attaches only to the `?` query's result and to `where`-free emission. Local
  `where` rules stay unordered; only the final emitted result is sorted. This is
  exactly where the compiler arc (anneal-g0l4) wants sort to live — a `Sort` node
  at the Plan's projection boundary. The AST field added here lowers straight to it.

### Semantics (the deliberate part)

1. **Stable sort** of result bindings by the key list, each `asc` (default) or
   `desc`. Keys are eval-supported `Expr`s over the result schema (so
   `order by rank`, `order by score desc`, and arithmetic keys like
   `order by recency + boost desc` all work).
2. **Tie-break preserves current order.** After all keys compare equal, fall back
   to the existing deterministic (lexical) order. Consequence: **a query with no
   `order by` is byte-identical to today** (no sort runs) — the differential-harness
   floor. A query *with* `order by` and fully-distinct keys is fully determined.
3. **Order before limit.** Sorting happens in core's query result; CLI `--limit`
   truncates the *ordered* rows. So `order by recency desc … --limit 12` is a true
   top-12-by-recency. (Today --limit truncates lexical order.)
4. **Value ordering** reuses the existing `Value` `Ord` already used by the
   `TopK`/`Rank`/`TakeUntil` aggregate comparators (`eval.rs` ~4939 and ~5022).
   Numbers, strings, bools, null, lists — total order already defined; no new
   comparison semantics. (Note: the `sort_at` comparator at ~1432 is
   snapshot day/string selection, *not* general `Value` ordering — not the
   grounding site for this.)
5. **Type/binding rules — an analysis invariant, not an eval fallback.** Each order
   key's `Expr` variables must be bound by the final result. Because `?` emits all
   body bindings, the result schema is the query body's positive binding variables:
   validate order-key variables against `query.body.positive_binding_variables()`
   *after* named-call normalization and local-rule checking. Literal-only key exprs
   are always safe. An unbound key is a **`StaticError`-class error surfaced before
   any rows** (same class as an unsafe head var) — never an `EvalError` after rows
   start streaming. (CR-R12 honesty: never a confident-looking list in wrong order
   from a typo'd key.)
6. **Aggregate-internal keys are rejected, by construction.** `order by` sees only
   projected/final bindings. If an aggregate (`TopK`/`Rank`/`TakeUntil`) used an
   internal key it did not emit, `order by <that key>` fails the binding check in
   (5). This is the intended boundary: ordering cannot reach back into aggregate
   internals.

### Composition with existing constructs

- **Aggregation / `TopK` / `TakeUntil`:** orthogonal. Aggregates run during
  evaluation and decide *which* rows exist; `order by` sorts whatever survived.
  `(h,rank,recency)=TopK{…}` selects, `order by recency desc` orders — they stack.
  `TopK` does not need to also sort; that conflation goes away.
- **Negation:** in-body, untouched.
- **`--explain` / trails / provenance:** ordering is a pure projection over rows;
  derivations are unchanged, `--explain` rows just emit sorted. Survives lowering
  trivially (HARD constraint from the arc — met by construction).
- **JSON / NDJSON / MCP:** because the sort lands in `QueryOutput.rows` in core,
  every surface inherits it. NDJSON consumers that learned the old lexical order
  see no change *unless* a query opts into `order by` — opt-in, no silent surface
  break (matches the arc's output-ordering resolution).

## Grammar / parser

- `order` + `by` as a **two-token contextual keyword** after the query body.
  `asc`/`desc` are contextual keywords after each key (default `asc`).
- **The current `parse_body_until(Dot)` cannot be reused unchanged** (codex caught
  this — the original draft was wrong). `parse_body_until` only stops at its `end`
  token: for `? foo(h) order by h.` it parses `foo(h)`, sees `order` while not at
  `Dot`, then `expect(Comma)` and **errors**. It does not halt at `order by`.
- **Fix — a query-body parser with an `order by` sentinel and comma-state
  discipline.** Parse atoms in a loop: after each atom, stop if next is `Dot` *or*
  the two-token `order by`; otherwise require a `Comma`, and **after a `Comma`,
  `order by` must NOT be accepted** (a trailing `, order by` is a parse error, not
  a clause). This makes the boundary exact and preserves:
  - `? order(h).` → relation atom (`order` + `LParen`), not a clause.
  - `? foo(h), order(x).` → `order(x)` is the second atom.
  - `? order(h) order by h.` → relation atom `order(h)` + ordering clause.
  - `? where r(h) := foo(h). r(h) order by h.` → only the **final query body**
    uses the order sentinel; local `where`-rule bodies still parse to `Dot` and
    cannot contain `order by`.
- Two-token lookahead is safe because a relation atom requires `order(` or
  `order{`, while the clause is `order by` appearing after an atom and *not* after
  a comma.
- After the body, parse a comma-separated list of `(Expr, Direction)`, then expect
  `Dot`.

## AST / core

- `ast.rs Query`: add
  `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub ordering: Vec<OrderKey>`
  where `OrderKey { expr: Expr, direction: Direction }`, `enum Direction { Asc, Desc }`.
  **`skip_serializing_if` is required, not optional** (codex): a bare
  `#[serde(default)]` would emit `ordering: []` and change serialized AST/program
  JSON for every existing query — violating the byte-identical floor on any
  round-trip/snapshot/introspection path that serializes `Query`. With skip-empty,
  an order-less query serializes exactly as today. Use the typed `Direction` enum,
  never a string (no stringly semantics).
- **Eval — sort bindings *before* projection/explain indexing** (codex, MEDIUM).
  `eval_query` has separate traced and untraced paths; `--explain` selects rows via
  `options.explains_row(index)` *during* projection (`traced_binding_to_row` /
  `binding_to_row`). Therefore: when `ordering` is non-empty, sort the
  `Vec<Binding>` / `Vec<TracedBinding>` **before** `binding_to_row` /
  `traced_binding_to_row`, so explain row numbers and derivations refer to the
  *sorted* output. Comparator folds the keys (`asc`/`desc` flips per key) with a
  final `then_with` on original index (stable). Evaluate each `OrderKey.expr` per
  binding via the existing `eval_expr`. When `ordering` is empty, **skip the sort
  entirely** — no comparator runs, output is byte-identical to today.
- **Analysis:** validate each order-key expr's variables against
  `query.body.positive_binding_variables()` (after named-call normalization and
  local-rule checking); literal-only exprs are safe. Emit a `StaticError` before
  planning/eval otherwise (no rows). Fits the future `AnalyzedProgram` invariant.

## Test plan

- **Parser:** `order by` single key; multi-key; `asc`/`desc`; default-asc;
  `order by` over an expr; relation named `order` still parses as an atom;
  malformed (`order by .`, trailing comma) rejected.
- **Eval unit:** stable tie-break preserves lexical order; desc reverses; multi-key
  precedence; numeric vs string ordering matches `Value` `Ord`; unbound key errors
  pre-rows.
- **Differential (mandatory, murail):** every existing prelude/eval query with NO
  `order by` returns byte-identical rows old-vs-new (build pre-change binary, diff
  result counts + row order on the diagnostic/frontier/flow/blocker/potential
  /holding set — the Lever-3 protocol). Zero drift is the gate.
- **Acceptance (the grin):** on murail,
  `? recent_frontier(h, rank, recency) order by rank asc.` puts rank=1 at row 1;
  `? ranked_anchor(h, rank, score, why) order by score desc.` puts v17 at row 1.
- **Order-before-limit:** `order by recency desc … --limit 5` == top-5 by recency,
  not 5 lexical survivors.

## Rollout

1. Land `order by` (parser + AST + eval + analysis + tests) — the language primitive.
2. Update the `status` dashboard pointer queries and the README/SKILL cold-start
   ladder to use `order by`, so the shipped ladder reads top-down and teaches the
   primitive by example.
3. (Arc) When the Plan/IR lands, lower `Query.ordering` to a projection `Sort`
   node. No surface change — the AST field is already the contract.

## Non-goals

- Ordering inside rules / recursive bodies (sets are unordered, by design).
- A CLI `--sort` flag (rejected: keeps ordering out of the language agents learn).
- `order by` driving selection (that's `rank <= N` / `TopK`); `order by` only sorts.
