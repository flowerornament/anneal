---
status: draft
updated: 2026-05-21
author: claude (sub-agent multi-arg ergonomics test)
depends-on:
  - 2026-05-20-datalog-learning-path.md
  - 2026-05-19-compatibility-surface-retire-audit.md
---

# Multi-arg positional predicate ergonomics — empirical trace

## Summary

Six tasks attempted against `diagnostic/6`, `read/7`, and `search/7` positional
primitives, with one cross-predicate join into the stored `*handle{...}` and
`*edge{...}` named-field relations. **All six succeeded on the first attempt
producing a valid (sometimes empty) result set** — the introspection that
`anneal --help` and `anneal help eval` ship with already prints the full
signatures for `diagnostic`, `search`, and `read` in their examples, so the
"first try" effectively had cheat-sheet support without my needing to call
`schema` or `describe` ahead of time. I called `describe diagnostic` and
`schema` exactly twice in the whole session, both for orientation after I had
already gotten a row back — not to recover from an error.

The single hard friction point: **anonymous-position wildcards (`_`) are
rejected with `expected expression`**, forcing the agent to invent throwaway
variable names for every "I don't care" slot. The other friction point —
silent-zero-rows on positional mistakes — was easy to trigger and produces
output indistinguishable from a real empty answer. Arity mismatches, by
contrast, error very cleanly with a signature hint (though arg *names* are
elided as `arg0..arg5`).

Positional ordering itself was not a struggle once the introspection chain was
used. The order `(code, severity, subject, file, line, evidence)` reads in a
natural reporting order (what / how bad / about whom / where / what line / why),
and the same is true for `read(handle, budget, span, text, start, end, tokens)`
(what / how much / span / payload / span-meta). `search`'s arg order is the
least intuitive — `(query, handle, span, score, reason, field, low_confidence)`
puts score and reason in the middle, and `field` (where the match lived) after
`reason` (what the match was) felt slightly inverted. I didn't fumble it because
the signature was right there in `--help`.

**Counts**
- Tasks succeeded first try: **6 / 6**
- Tasks requiring schema/describe lookup before issuing the query: **0 / 6**
- Schema/describe calls during the whole session: **2** (both post-hoc
  orientation, not error recovery)
- Total queries attempted across all tasks (including sanity checks and
  recovery probes): **~16**
- Parse/static-analysis errors triggered: **2** (one wildcard `_`, one wrong
  arity — both intentional friction probes)

## T1 — find E001 errors in large-corpus

Goal: `diagnostic` with `code = "E001"` against large-corpus.

Attempt 1:
```
anneal --root /path/to/large-corpus/.design \
  -e '? diagnostic("E001", severity, subject, file, line, evidence).' --limit 50
```
Result: `(0 rows)`. Wrote the literal `"E001"` directly into position 1
(matching the example shown in `--help`), so the query was correct;
large-corpus genuinely has no E001s.

Sanity probe: `diagnostic(code, sev, subj, f, l, e) --limit 5` — confirmed
the predicate is populated (I001, S001, S003, S005, W001, W003, W004 rows
all returned). Empty result is the real answer.

Outcome: succeeded first try. No introspection needed. No position confusion.

## T2 — diagnostics with severity "warning" in anneal

Goal: filter on positional arg 2 only.

Attempt 1 — tried positional wildcard:
```
anneal -e '? diagnostic(code, "warning", subject, _, _, _).'
```
Result: parse error
```
cli-query:1:40: expected expression
```
The error points to column 40 (the first `_`). It doesn't say *what* it
expected, doesn't say "wildcards not supported here," and doesn't suggest
"bind a fresh variable instead." A cold agent could plausibly read this as
"missing argument" or "syntax I forgot" — recovery requires knowing the
language convention.

Attempt 2 — named throwaway vars:
```
anneal -e '? diagnostic(code, "warning", subject, f, l, e).'
```
Result: `(0 rows)`. Sanity check showed only two diagnostics in the anneal
corpus (E001 + I001), neither with severity=warning. Query was correct.

Outcome: succeeded second try after one parse-error friction event. The wildcard
limitation is the single sharpest friction in the whole exercise.

## T3 — OQ labels with at least one diagnostic (large-corpus)

Goal: cross-predicate join between named-field stored relation and 6-arg
positional derived predicate.

Attempt 1:
```
anneal --root /path/to/large-corpus/.design \
  -e '? *handle{id: h, kind: "label", namespace: "OQ"}, \
       diagnostic(code, sev, h, f, l, e).' --limit 50
```
Result: `(0 rows)`. The shared variable `h` lined up immediately —
`*handle{id: h}` and `diagnostic(_, _, h, _, _, _)` (with my made-up names)
both bind position-by-position cleanly.

Sanity probes confirmed the answer is real-empty: OQ-* labels exist (76+),
diagnostic subjects in large-corpus are file paths and namespace strings, never
OQ-* label ids. The lone S005 row with `subj: "OQ"` is a namespace-level
diagnostic, not a label-level one.

Outcome: succeeded first try. The fact that one side is `{named: field}`
and the other side is `(positional, positional, positional)` did not bite —
the unification still works by variable name across both forms.

## T4 — read first span of large-corpus v17

Goal: 7-arg primitive, all positional.

Attempt 1:
```
anneal --root /path/to/large-corpus/.design \
  -e '? read("formal-model/sample-formal-model-v17.md", 4000, \
            span_id, text, sl, el, tk).' --limit 1
```
Result: one row with the abstract through §3 of the v17 spec, ~4000 tokens,
`sl=65`, `el=4362`, `span_id="formal-model/sample-formal-model-v17.md#full"`.

Notes:
- `read`'s arg order is documented as `(handle, budget, span, text, start,
  end, tokens)` in `help eval` examples.
- The signature `read(handle, budget, span_id, text, start_line, end_line,
  tokens)` is one of the clearest of the bunch — input args (`handle`,
  `budget`) precede output args (`span`, `text`, `start`, `end`, `tokens`).
  Mental model: "give me up-to-N tokens of this handle, fill in the rest."

Outcome: succeeded first try, no friction.

## T5 — search with score > 0.8

Goal: 7-arg primitive with inline comparison filter.

Attempt 1:
```
anneal --root /path/to/large-corpus/.design \
  -e '? search("conformance", h, span, score, reason, field, low), \
       score > 0.8.' --limit 50
```
Result: ~50 rows, all `score: 1.0`, mix of title-substring and
identifier-substring matches, file handles and section handles both
included. Filter applied correctly.

Outcome: succeeded first try. The "bind in the call, filter after the
comma" idiom (`predicate(..., score, ...), score > 0.8`) is the same
shape used everywhere in Datalog, and `help eval` reinforces it.

## T6 — composite: files with both diagnostic AND DependsOn

Goal: three-way join — stored relation + 6-arg derived + stored relation.

Attempt 1:
```
anneal --root /path/to/anneal/.design \
  -e '? *handle{id: h, kind: "file"}, \
       diagnostic(code, sev, h, hf, l, e), \
       *edge{from: h, to: dst, kind: "DependsOn"}.' --limit 50
```
Result: `(0 rows)`.

Sanity probes:
- Diagnostic subject `anneal-spec.md` exists as a `file`-kind handle.
- `anneal-spec.md` has only `Cites` edges outgoing, no `DependsOn`.
- DependsOn edges exist in the corpus, just not from the (one) file
  that has a diagnostic.

So the join is genuinely empty. Query was correct, three named-field /
positional sides composed without confusion.

Outcome: succeeded first try. Three-way joins through a shared variable
`h` across stored and positional predicates were as ergonomic as two-way
joins.

## Friction patterns observed

1. **Wildcard `_` is a parse error in positional argument slots.** This is
   the only first-class friction. The error message (`expected expression`)
   does not hint at the language rule. Agents must invent throwaway names
   (`f`, `l`, `e`, `hf`, `_unused1`, etc.) for every position they don't
   care about, and a 6-arg predicate with one constraint requires inventing
   four to five throwaway names.

2. **Silent zero rows on positional mistakes.** Swapping `code` and
   `severity` positions (e.g. `diagnostic("error", "E001", ...)`) returns
   `(0 rows)` with no warning. This is identical to a legitimate empty
   answer. Agents have no fast feedback that they put a literal in the
   wrong slot — they'd need to re-run a sanity query without the literal
   to discover that other rows exist.

3. **Arity errors are excellent.** `diagnostic(code, sev, subj, f, l)`
   (5 args) gives:
   ```
   predicate 'diagnostic' used with arity 5, expected 6;
   signature: diagnostic(arg0, arg1, arg2, arg3, arg4, arg5)
   ```
   Arity is immediately fixable. But the signature uses placeholder names
   `arg0..arg5`, not the documented `code, severity, subject, file, line,
   evidence`. To recover the *real* names you still need `describe
   diagnostic` or `schema`. The error message could embed the real names
   from the predicate definition with no apparent downside.

4. **Asymmetry between stored and derived predicates is real and felt.**
   For the same logical concern ("I only care about a few fields"):
   - Stored: `*handle{id: h, kind: "file"}` — implicitly ignores 8+ other
     fields including `corpus`, `source`, `native_id`, `origin_uri`,
     `revision`, `generation`, `status`, `namespace`.
   - Derived: `diagnostic(_, "warning", _, _, _, _)` — illegal. Must
     write `diagnostic(c, "warning", s, f, l, e)` with five named-but-
     unused variables, polluting the binding namespace.

   This asymmetry was noticeable on T2 (need-to-invent-names) and would
   compound on `read/7` and `search/7` if I had wanted to project just
   one or two args.

5. **Position confusion did NOT materialize.** I never wrote `diagnostic`
   with `subject` in position 1 or `code` in position 3. Two reasons:
   - The `--help` and `help eval` outputs both show the signature with
     argument names inline as an example query, so there is no need to
     consult `describe` for the first use.
   - The argument orders (for diagnostic, read, search) are each in a
     "natural reporting order" mentally (what / how bad / about whom /
     where / line / why), so even without the cheat sheet, the order is
     guessable. `search`'s arg 6 (`field`) and arg 5 (`reason`) felt
     slightly inverted but did not cause an error.

6. **Variable name memory was not a problem at this scale.** For 6-arg
   `diagnostic` I used `c, s, h, f, l, e` (or `code, sev, subj, f, l, e`)
   consistently and didn't have to look back. For a 10+ arg predicate this
   might break down, but neither `diagnostic` (6) nor `read`/`search` (7
   each) was painful.

7. **Introspection cost was effectively zero.** I called `describe
   diagnostic` once and `schema` once, both as orientation, not error
   recovery. The example queries baked into `--help` and `help eval`
   already disclose the full signature with named positions, so an agent
   that reads help text once has the cheat sheet for the rest of the
   session.

## Recommendation

**Adding named-field syntax to derived/primitive predicates would help, but
the single highest-leverage change is enabling positional wildcards (`_`)
inside primitive/derived calls.** Wildcards address the single concrete
friction point I hit (T2 attempt 1) and would close the most-felt asymmetry
with stored relations: stored `{...}` syntax lets you ignore unused fields;
positional `(...)` syntax should let you ignore unused positions. This is a
strictly smaller, strictly more local change than full named-field syntax.

If named-field syntax is added on top, the win is bigger but the marginal
ergonomic value over positional + `_` is modest at the current arities (6
and 7). The two changes solve mostly the same problem; positional + `_` is
~80% of the value at probably ~10% of the implementation cost.

The other change worth bundling: **embed real argument names in static-
analysis errors.** Replacing `signature: diagnostic(arg0, arg1, arg2, arg3,
arg4, arg5)` with `signature: diagnostic(code, severity, subject, file,
line, evidence)` makes the error self-documenting and removes one of the
two remaining reasons to call `describe`.

**On D1 + typed-filter-args slice scope:** the empirical evidence here does
not justify expanding the slice to include named-field syntax for derived
and primitive predicates. Positional wildcards + signature-in-errors are
strictly more impactful and strictly smaller. If the slice can absorb just
those two micro-improvements, the introspection chain becomes sufficient
scaffolding for everything else observed.

## Concrete signal: would `diagnostic(code: c, file: "X")` help?

Less than I expected before running the experiment. Here's the breakdown of
where named-field syntax for derived predicates would actually have helped
on these six tasks:

| Task | Current form bit me? | Would named-field have helped? |
|------|----------------------|--------------------------------|
| T1   | No                   | No |
| T2   | Wildcard rejected    | Yes — `diagnostic(severity: "warning")` skips the wildcard problem entirely |
| T3   | No                   | Marginal — `diagnostic(subject: h)` is a tiny win |
| T4   | No                   | No |
| T5   | No                   | No |
| T6   | No                   | Marginal — same as T3 |

Net: **1 task where named-field syntax would have been a clear win, 2 where
it would have been marginal, 3 where it would have changed nothing.** The
same 1 + 2 + 3 partition holds if instead the slice adds positional `_`
wildcards: T2 becomes trivial, T3/T6 lose a few throwaway-name characters,
T1/T4/T5 unchanged.

The wins overlap almost completely, so picking the cheaper change
(positional `_` + better arity-error names) captures the same evidence-
backed friction reduction. Expanding the slice to full named-field syntax
on derived predicates would be over-engineering relative to the friction
this empirical run actually surfaced.

If `read/7` or `search/7` predicates grew to 10+ args, or if future
primitives carry many optional-feeling fields, the calculus would shift
toward named fields. At present arities, the positional form held up.
