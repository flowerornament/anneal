---
status: draft
updated: 2026-05-20
author: claude (sub-agent flag audit)
depends-on: 2026-05-19-compatibility-surface-retire-audit.md
---

# anneal v0.11.1 — Flag Quality Audit

## Summary

Audited **~150 distinct flag invocations** across **29 commands** (16 default-help, 13 hidden compat) of anneal v0.11.1. Counts (approximate; some flags double-count across categories):

- **WORKS**: ~55 — flag does the documented thing.
- **CRUFT**: ~38 — accepted but no-op for that command. Concentrated on `prime`, `init`, and `obligations`, where global flags like `--area`, `--recent`, `--since` are inherited but ignored. `--pretty` is cruft on every prelude verb.
- **CONFUSING**: ~22 — flag works but help text omits type, default, semantics, or when to reach for it. `explain diagnostic`'s `--id/--code/--file/--line/--handle` all ship with **empty help text**.
- **INCONSISTENT**: ~15 cross-command issues — `--limit` has three different meanings across `eval` / `search` / `context`; `--format` exists on prelude only; `--pretty` works only on compat JSON; `--scope` exists on `check` and `query *` but not `find`/`get`.
- **UNDOCUMENTED**: ~12 — `--explain`, `--explain-first`, `--explain-all`, `--explain-depth` work on every prelude verb (`status`, `work`, `blocked`, `broken`, `search`, `read`, `vocab`, `verbs`, `schema`, `describe`, `sources`) but are listed only in `help eval`.
- **BROKEN**: ~6 — wrong/dangerous behavior. `diff <nonexistent-ref>` silently treats the entire corpus as new. `find --namespace=CR` returns 0 matches when `CR-*` labels exist. `find --kind=invalid` and `describe nonsense` silently return zero rows instead of erroring. `prime --json` does not produce JSON.

The headline ergonomic problem: anneal has two flag dialects that aren't visibly distinguished. Prelude verbs (`status`, `work`, `blocked`, `eval`, etc.) take `--format`, support `--explain*`, reject `--area/--recent/--since`, and emit NDJSON only. Compat verbs (`check`, `find`, `get`, `map`, `impact`, etc.) reject `--format`, ignore `--explain*`, accept `--area/--recent/--since` (sometimes acting, sometimes silently no-oping), and respect `--pretty`. The `--help` for each command does not flag this split, so users learn it by erroring through it.

---

## Per-Command Tables

### `status` (prelude verb)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--format=text` | WORKS | `anneal status --format=text` prints human table; `--format=json` prints NDJSON; `--format=yaml` errors `--format accepts json or text; got "yaml"`. |
| `--json` | WORKS | Forces NDJSON; same as `--format=json`. |
| `--pretty` | CRUFT | `anneal status --json --pretty` emits unindented NDJSON identical to `--json` alone. `--pretty` is documented as JSON-only and pretty-printing has no effect on NDJSON rows. |
| `--area=foo` | CRUFT | `anneal status --area=foo` errors `status accepts no arguments; got "--area=foo"`. Listed in global help but rejected by the command. Same for `--recent`, `--since`, `--minimal`, `--no-color` as positional. |
| `--plain` | CRUFT | Listed in global help; not in `help status`. When piped, output is NDJSON regardless of `--plain`. No effect. |
| `--explain` | UNDOCUMENTED | `anneal status --explain` adds `_derivation` trees to every row. Not in `help status`. Same applies to all prelude verbs below. |
| extra positional | (correct) | `anneal status extra-arg` errors `status accepts no arguments`. |

### `context <GOAL>` (prelude verb)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--hits=N` | WORKS | Caps search winners. `--hits=2` returns 2 hits. |
| `--limit=N` | INCONSISTENT | Help says "Alias for --hits" but `--hits=2 --limit=5` returns 4 hits, not 2. The two flags evidently set different verb args (`hits` and `limit`) and the verb body picks the larger; documenting them as aliases is wrong. See **Inconsistencies** below. |
| `--budget=N` | CONFUSING | `--budget=500 --hits=2` gives two 300-token spans (total 600). The help "Derives one per-hit read cap; not divided by hits" is technically accurate but unclear — say "tokens per hit". `--budget=4000 --hits=3` returns 7200 total tokens, which a user reading "Token budget" on `read` will not expect. |
| `--depth=N` | WORKS | Alias for `--neighborhood-depth`. Verified `--depth=2` and `--neighborhood-depth=2` produce identical neighborhood length (306). |
| `--neighborhood-depth=N` | WORKS | See above. |
| `--include-low-confidence` | CONFUSING | On `--include-low-confidence` for `xyzzy-nonexistent-token` query, hit count was unchanged (3 vs 3). Unclear what threshold low-confidence triggers; help doesn't define it. |
| `--explain` | UNDOCUMENTED | Same as status. |

### `prime`

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--json` | BROKEN | `anneal prime --json` emits the same markdown briefing as `anneal prime`. Help says `--json` forces JSON output. Word-for-word identical (251 lines both). |
| `--area=foo` | CRUFT | Output identical (251 lines) to `prime`. |
| `--recent` | CRUFT | Output identical. |
| `--since=14d` | CRUFT | Output identical. |
| `--plain` | CRUFT | Output identical. |
| `--minimal` | CRUFT | Output identical. |
| `--no-color` | CRUFT | Output identical (pre-baked markdown). |
| `--pretty` | CRUFT | Output identical. |

Prime is `include_str!`-baked markdown; **every flag except `--help` is no-op**, including `--json` which is documented as forcing JSON.

### `schema`, `verbs`, `vocab`, `sources` (prelude verbs)

Same flag profile as `status`: `--format=text|json`, `--json`, `--explain*` (undocumented but works), `--pretty` (cruft), `--area/--recent/--since` rejected as positional args.

### `describe [NAME]`

| Flag | Category | Evidence/Notes |
|---|---|---|
| `[NAME]` | CONFUSING | `anneal describe` with no NAME returns "anneal runtime: ..." (a one-row self-description). The help says "Defaults to runtime" but does not say users should expect a list. Users from v0.10 or other CLIs will reasonably expect a list-all. `anneal describe foobarbaz` returns `(0 rows)` silently — should suggest `verbs` or `schema`. |
| `--format` | WORKS | Same as prelude. |
| `--explain` | UNDOCUMENTED | `anneal describe search --explain` includes derivation. |

### `eval` / `-e <QUERY>` (prelude verb)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--limit=N` | WORKS, INCONSISTENT | `--limit 5` returns 5 rows; default returns all (824 in test corpus). Help: "Cap returned rows after evaluation". Behavior differs from `search --limit` (whose default is 25) and `context --limit` (which means hits). |
| `--explain` | WORKS | Adds `_derivation` to first 3 rows. |
| `--explain-first=N` | WORKS | `--explain-first=0` produces 0 derivations; `--explain-first=2 --limit=5` produces 2. |
| `--explain-all` | WORKS | Verified: 5 derivations for 5 rows. |
| `--explain-depth=N` | WORKS | At depth 1, `_derivation.children` are leaves; at depth 10, full trees. |
| bad syntax | (correct) | `-e 'bogus'` errors `failed to parse query "bogus": cli-query:1:6: expected '('` — clear. |
| bare `eval` | (correct) | Errors `eval requires a query`. |

### `search <TEXT>`

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--limit=N` | WORKS, INCONSISTENT | Default 25, capped result count. Different semantics than `context --limit`. |
| `--include-low-confidence` | CONFUSING | Same problem as context: `--include-low-confidence` does not visibly expand the result set in the test corpus. The "low confidence" threshold is undocumented. |
| `--explain` | UNDOCUMENTED | Works. |

### `read <HANDLE>`

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--budget=N` | WORKS, INCONSISTENT | `--budget=1000` returns 1000-token span; `--budget=4000` returns 4000; `--budget=8000` returns 8000. Documented default 4000. Semantics: hard cap (correct). Different from `context --budget` which is per-hit (multiplied by hits). |
| `--explain` | UNDOCUMENTED | Works. |
| (missing handle) | (correct) | `anneal read` errors `read requires a handle`. |

### `handle <HANDLE>` (alias `H`)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `H` alias | WORKS | `anneal H README.md` matches `anneal handle README.md`. |
| `--explain` | UNDOCUMENTED | Returns 0 rows on tested handles (because the handle prelude verb may not have a derivation path that exposes interesting trees). |
| no edges of any type | CONFUSING | Help block doesn't say how to bound the edge list. |

### `work`, `blocked <HANDLE>`, `broken`, `trend`

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--explain` | UNDOCUMENTED | Verified working on `work`, `blocked CR-D1`, `broken`. `--explain-depth`, `--explain-all`, `--explain-first` also accepted (inherited from eval). |
| `--format` | WORKS | Same as status. |
| extra positional | (correct) | `anneal work --area=foo` errors `work accepts no arguments; got "--area=foo"`. |
| `blocked` (no arg) | (correct) | Errors with usage hint. |

### `init`

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--dry-run` | WORKS | Prints what would be written, no file. |
| `--force` | WORKS (untested write) | Help says replaces existing anneal.dl or migrates .toml. Not destructively tested. |
| `--json` | WORKS | `init --json --dry-run` emits structured JSON of the inferred config. |
| `--area=foo` | CRUFT | Output identical to `--dry-run` alone. Init has no notion of "area". |
| `--recent` | CRUFT | Output identical. |
| `--since=14d` | CRUFT | Output identical. |
| `--plain`, `--minimal`, `--no-color`, `--pretty` | CRUFT | Output identical (init is structured text). |

### `check` (hidden compat)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--errors-only` | WORKS | Filters to errors. |
| `--stale` | WORKS | Convenience alias for W001. |
| `--obligations` | WORKS | Alias for E002/I002. |
| `--suggest` | WORKS | Alias for severity=suggestion. |
| `--file=<path>` | WORKS | Scopes to one file. |
| `--scope=active\|all` | WORKS | Verified: `--scope=all` returns 1 diagnostic (I001), `--scope=active` returns same — but no terminal-only diags in test corpus to differentiate. |
| `--active-only` | WORKS | Deprecated alias documented in examples. |
| `--include-terminal` | WORKS | Equivalent to `--scope=all`. Two flags for same thing — see Inconsistencies. |
| `--diagnostics` | WORKS | `check --json --diagnostics` adds `diagnostics` key with sampled diagnostic list. |
| `--full` | WORKS | `check --json --full` adds `diagnostics`, `extractions`, `extractions_summary` keys. |
| `--extractions-summary` | WORKS | Adds `extractions_summary` key only. |
| `--full-extractions` | WORKS | Adds `extractions` key only. |
| `--limit=N` | WORKS | Caps diagnostic sample size in JSON mode. |
| `--area=<area>` | WORKS (silent) | Filters to area; on `--area=foo` (nonexistent) silently returns zero diagnostics — confusing. |
| `--recent` | CRUFT (mostly) | On test corpus produces same 0-diag result; cannot confirm filter actually fires. |
| `--format=text` | INCONSISTENT | Errors `unexpected argument '--format'`. Only prelude verbs accept `--format`. |
| `--no-snapshot` | (absent) | Not present. Snapshot is always taken. |

### `get <HANDLE>...` (hidden compat)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--refs` | INCONSISTENT | In text mode, `anneal get HANDLE`, `--refs`, `--trace`, `--full`, and no-flag all produce **identical 22-line output**. They only differ in JSON output. Help doesn't say this. |
| `--trace` | INCONSISTENT | Same as `--refs` in text mode. JSON is identical to `--full`. |
| `--context` | WORKS | Visibly different: prints "Context" section with summary. |
| `--full` | WORKS (JSON) | Same JSON keys as default plus more entries; in text identical. |
| `--status-only` | WORKS | `get HANDLE1 HANDLE2 --status-only` returns batch table. |
| `--limit-edges=N` | WORKS | `--refs --limit-edges 2` reduces incoming/outgoing edges to 2/2 with `truncated_edges: true`. |
| `--area=<area>` | CRUFT (mostly) | Output unchanged in text mode for tested handles. |
| `--recent`, `--since=14d` | CRUFT | Output unchanged. |
| `--format` | INCONSISTENT | `anneal get HANDLE --format=text` errors `unexpected argument '--format'`. |

### `find [QUERY]` (hidden compat)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--all` | WORKS | Includes terminal handles (25 → 25, but total 263 → 265). |
| `--namespace=<NS>` | BROKEN | `find --namespace=CR` returns `Matches (0)` even though `CR-*` labels exist (verified `find "CR" --kind=label` also returns 0, suggesting label kind extraction may be misaligned with namespace filter). Help says "label prefix, e.g., OQ". |
| `--kind=<KIND>` | WORKS / CONFUSING | `--kind=label` returns labels. `--kind=invalid` returns 0 silently rather than erroring. Help lists valid values but parser does not reject invalid ones. |
| `--status=<STATUS>` | WORKS | `--status=draft` returns 5 matching files. |
| `--limit=N` | INCONSISTENT | `find --limit=2` (no query, no filter) errors `empty query requires a narrowing filter or --full`. Different from search/eval/context. |
| `--offset=N` | INCONSISTENT | Same as `--limit`: requires query or filter to be useful. |
| `--full` | WORKS | Returns full match set. |
| `--no-facets` | WORKS | Removes `facets` key from JSON output. No effect on text. |
| `--area=<area>` | WORKS (silent) | Same area-filter pattern. |
| `--recent`, `--since=14d` | CRUFT | No visible effect on test corpus. |
| `--format` | INCONSISTENT | Errors `unexpected argument '--format'`. |
| no query | CONFUSING | `find` alone errors `empty query requires a narrowing filter or --full`. The error suggests `--full` but `--limit`/`--offset` are silently rejected as narrowing. |

### `impact <HANDLE>` (hidden compat)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--area=foo` | BROKEN-ISH | `impact HANDLE --area=foo` (nonexistent area) silently returns `Direct (0) (none)`. Same for `--area=root`. Looks like the filter applies post-hoc and erases real results. Should probably error or warn. |
| `--since=1d` | WORKS (silent) | Reduces line count from 21 to 8 — appears to filter ancestors by file recency. |
| `--recent` | WORKS (silent) | Reduces line count from 21 to 12. |
| `--format` | INCONSISTENT | Errors. |

### `map` (hidden compat)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--render=summary\|text\|dot` | WORKS / CONFUSING | Default `summary` works. `--render=text` and `--render=dot` both error with `full graph rendering requires --full; use anneal map --render=text --full or focus with --around/--concern/--area`. The error is helpful but `--render` should accept the bare value. |
| `--around=<handle>` | WORKS | Prints neighborhood. |
| `--depth=N` | WORKS | `--around X --depth=2` expands from 11 nodes/22 edges to 44 nodes/76 edges. |
| `--upstream` | WORKS | Requires `--around`. Errors clearly when bare. |
| `--downstream` | WORKS | Requires `--around`. |
| `--concern=<group>` | WORKS (silent) | `--concern=foo` returns empty graph. |
| `--by-area` | WORKS | Renders area topology graph. |
| `--min-edges=N` | WORKS | With `--by-area`. |
| `--area=<area>` | CRUFT-ISH | Setting `--area=foo` produces 0 nodes; can't tell if intended. |
| `--recent` | WORKS (silent) | Changes node count (`map --recent` → 293 nodes vs default 823). |
| `--since=1d` | WORKS (silent) | Changes node count. |
| `--format` | INCONSISTENT | Errors. |

### `health` (hidden compat)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--verbose` / `-v` | CRUFT? | Help says "Expand pipeline histogram to list files per level". On the test corpus, `health` and `health --verbose` produce **identical 5-line output**. May be correct only when pipeline ordering produces multi-level histograms; on this corpus there is no expansion to test. |
| `--compact` | WORKS | Emits compact JSON orientation payload. |
| `--json` | WORKS | Yes. |
| `--pretty` | WORKS | Indents JSON. |
| `--area`, `--recent`, `--since` | CRUFT | Output unchanged. |
| `--format` | INCONSISTENT | Errors. |

### `diff [REF]` (hidden compat)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--days=N` | WORKS | `diff --days=1` shows `+7 created, +7 active`. |
| `--by-area` | WORKS | Shows per-area deltas. |
| `[REF]` | BROKEN | `diff nope-not-a-ref` (a nonexistent git ref) silently produces `+824 created, +814 active, +10 terminal` — treats the entire corpus as new. There is no validation that the ref exists. This is dangerous output for any agent that runs `diff $UNTRUSTED_REF`. |
| `--area`, `--recent`, `--since` | CRUFT | No effect. |

### `obligations` (hidden compat)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--json`, `--pretty` | WORKS | Standard. |
| `--area=foo` | CRUFT | Output identical regardless. |
| `--recent`, `--since` | CRUFT | Output identical. |
| `--format` | INCONSISTENT | Errors. |

### `areas` (hidden compat)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--sort=files\|grade\|conn\|name` | CRUFT-ISH | Only one area in test corpus so visible sort impossible to verify. Help text is clear. |
| `--include-terminal` | CRUFT-ISH | Output unchanged on test corpus. |
| `--area=foo` | CRUFT | `areas --area=foo` shows the same `(root)/` area. Filtering an areas-listing by a single area is conceptually weird. |
| `--since=1d` | WORKS | Slight change observed. |
| `--format` | INCONSISTENT | Errors. |

### `garden` (hidden compat)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--category=fix\|tidy\|link\|stale\|meta\|drift` | WORKS | `--category=fix` returns 0 tasks; `--category=link` returns 1. |
| `--limit=N` | WORKS | `--limit=2` returns at most 2 tasks. |
| `--area=<area>` | WORKS (silent) | `--area=foo` returns "corpus is tidy". |
| `--recent`, `--since` | CRUFT | Output identical. |

### `orient` (hidden compat)

| Flag | Category | Evidence/Notes |
|---|---|---|
| `--budget=<token>` | WORKS | Accepts `50k`, `10000`, etc. Errors with helpful message on `--budget=garbage`. |
| `--paths-only` | WORKS | Emits bare file paths. |
| `--file=<file>` | CONFUSING | `orient --file=README.md` errors `handle not found: README.md` when README.md is just absent from the test corpus. Help should mention the file must be a corpus handle. |
| `--area=<area>` | WORKS | Documented in EXAMPLES. |
| `--json`, `--pretty` | WORKS | Both work. |
| `--recent`, `--since` | CRUFT | No observable effect. |
| `--format` | INCONSISTENT | Errors. |

### `query <SUB>` (hidden compat)

`query handles`, `query edges`, `query diagnostics`, `query obligations`, `query suggestions`. Pattern: every subcommand has `--limit`, `--offset`, `--full`, `--scope=active|all`, `--area`, `--recent`, `--since` as standard.

| Flag | Category | Evidence/Notes |
|---|---|---|
| `query handles --kind=<K>` | WORKS | Filter clearly bound to valid enum. |
| `query handles --scope=all` | WORKS | Expands from 11 to 824 handles. |
| `query edges --kind=Cites` | WORKS | Filter works. |
| `query edges --confidence-gap` | UNTESTED | Help describes it; no confidence-gap edges in test corpus. |
| `query diagnostics --severity=<S>` | WORKS | Valid enum. |
| `query diagnostics --errors-only/--stale/--obligations/--suggest` | WORKS | Convenience aliases. |
| `query obligations --namespace <NS>` | CONFUSING | Help text is **empty** — `--namespace <NAMESPACE>` has no description column. |
| `query suggestions --code <CODE>` | CONFUSING | Same: no help text on `--code`. |

### `explain <SUB>` (hidden compat)

`explain diagnostic`, `explain impact <H>`, `explain convergence`, `explain obligation <H>`, `explain suggestion [CODE]`.

| Flag | Category | Evidence/Notes |
|---|---|---|
| `explain diagnostic --id <ID>` | CONFUSING | Help text **empty**. |
| `explain diagnostic --code <CODE>` | CONFUSING | Help text **empty**. |
| `explain diagnostic --file <FILE>` | CONFUSING | Help text empty. |
| `explain diagnostic --line <LINE>` | CONFUSING | Help text empty. |
| `explain diagnostic --handle <HANDLE>` | CONFUSING | Help text empty. |
| `explain diagnostic` (no selector) | WORKS | Errors clearly: `no diagnostic matched the provided selectors; use --id or narrow with --code/--file/--line/--handle`. |
| `explain impact <HANDLE> --full` | CONFUSING | Help text empty. |
| `explain suggestion --id <ID>` | CONFUSING | Help text empty. |
| `explain suggestion --handle <HANDLE>` | CONFUSING | Help text empty. |
| `explain convergence` | WORKS | Returns convergence signal explanation. |
| `explain obligation <H>` | WORKS | Standard subcommand. |

---

## Inconsistencies

### 1. `--limit` has three meanings

| Command | Meaning | Default |
|---|---|---|
| `eval` | Cap returned rows after evaluation | unlimited |
| `search` | Maximum rows | 25 |
| `context` | "Alias for --hits" — actually a *separate* arg that interacts with `--hits` (not a true alias) | 3 (the `--hits` default) |
| `find` | Maximum matches (rejected without query/filter) | 25 |
| `garden` | Maximum tasks | 10 |
| `get` | (absent — uses `--limit-edges` instead) | - |
| `check` | Maximum diagnostics in JSON sample mode | (unspecified) |
| `query *` | Maximum rows | 25 |

```
$ anneal context "convergence" --hits=2 --limit=5 --json | jq '.hits|length'
4    # not 2 (--hits) and not 5 (--limit) — the *interaction* picks 4
```

`context --limit` should either be renamed or behave as a true alias. Currently the help is **wrong**.

### 2. `--format=text|json` is prelude-only

Works on: `status`, `context`, `work`, `blocked`, `broken`, `eval`, `search`, `read`, `handle`, `trend`, `vocab`, `verbs`, `schema`, `describe`, `sources`.

Errors on: `check`, `find`, `get`, `impact`, `map`, `health`, `diff`, `obligations`, `areas`, `garden`, `orient`, `query *`, `explain *`.

```
$ anneal check --format=text
error: unexpected argument '--format' found
$ anneal status --format=text
Status
Broken
 1. anneal-spec.md  score=100  E001
```

### 3. `--pretty` only pretty-prints on compat commands

```
$ anneal status --json --pretty | head -1
{"h":"...","score":3, ...}            # NDJSON, no indent
$ anneal check --json --pretty | head -3
{
  "_meta": {
    "schema_version": 2,
```

`--pretty` is documented identically in both groups' help but is silently a no-op on prelude verbs.

### 4. `--explain` is prelude-only, undocumented per command

`--explain`, `--explain-first=N`, `--explain-all`, `--explain-depth=N` all work on every prelude verb tested (`status`, `work`, `blocked`, `broken`, `search`, `read`, `vocab`, `verbs`, `schema`, `describe`, `sources`) but appear only in `help eval`.

```
$ anneal blocked CR-D1 --explain | jq -c 'keys'   # works
$ anneal help blocked | grep explain              # not mentioned
```

### 5. `--area`, `--recent`, `--since` are silently rejected by prelude

```
$ anneal status --area=foo
error: status accepts no arguments; got "--area=foo"
$ anneal init --area=foo --dry-run
  anneal.dl
  dry run — not written        # silently ignored
```

Prelude verbs error out helpfully. Compat verbs silently no-op or silently filter. **Same flag, different semantics on different commands.**

### 6. `--scope=active|all` vs `--active-only` vs `--include-terminal`

`check` accepts all three. They all mean the same thing. Help says `--active-only` is "Deprecated alias". `--include-terminal` and `--scope=all` are not documented as aliases of each other but should be.

### 7. `--limit-edges` vs `--limit`

`get --limit-edges` caps edges per direction. `get --limit` does not exist. Every other command uses `--limit`. The deliberate naming is fine, but `find --limit` (max matches) vs `get --limit-edges` (max edges per direction) feels arbitrary — when does `--limit` mean rows vs subjects?

### 8. `--no-facets` on `find` is JSON-only

Not documented as such; reads like a top-level toggle.

### 9. Empty `--help` columns on `query` and `explain` subcommands

Multiple `query *` and **every** `explain *` subcommand has flags with no help text:

```
$ anneal explain diagnostic --help
      --id <ID>          
      --code <CODE>      
      --file <FILE>      
      --line <LINE>      
      --handle <HANDLE>  
```

5 of 7 listed flags have no descriptions.

---

## Cruft

Flags accepted but no-op on these commands:

### `prime`
**All flags are cruft.** `prime` outputs an `include_str!`-baked markdown briefing and ignores: `--json`, `--pretty`, `--area`, `--recent`, `--since`, `--plain`, `--minimal`, `--no-color`. Verified: line count is exactly 251 in every case. The `--json` cruft is especially bad because help promises JSON.

### `init`
Cruft: `--area`, `--recent`, `--since`, `--plain`, `--minimal`, `--no-color`, `--pretty`. Init reads filesystem and writes one file; "area" and "recency" have no meaning. Real flags: `--dry-run`, `--force`, `--json`.

### `obligations`
Cruft: `--area`, `--recent`, `--since`. The corpus has 0 obligations so filtering can't change a 0. But conceptually obligations don't filter by file recency either.

### `health`
Cruft on test corpus: `--area`, `--recent`, `--since`. Possibly intended as area-scoped health views but never visibly altered output.

### Compat commands generally
`--minimal`, `--no-color`, `--plain` are global flags wired through clap but mean nothing where there is no color/Unicode to suppress (e.g., `init`, `prime`). These should be hidden where they don't apply.

### Prelude verbs generally
`--pretty` is cruft on every prelude verb (NDJSON is never indented).

---

## Broken

### B1. `prime --json` does not produce JSON

```
$ anneal prime --json | head -3
# Anneal

Use `anneal` as the runtime for a knowledge corpus. It turns corpus files into
```

The help promises "Output as JSON (all commands)". This is a documentation lie.

### B2. `diff <nonexistent-ref>` silently treats whole corpus as new

```
$ anneal --root .design diff nope-not-a-ref
  Diff
  since nope-not-a-ref

  Handles      +824 created, +814 active, +10 terminal
```

There is no git ref `nope-not-a-ref`. The command should either error or fall back to "since last snapshot" with a warning. As-is it will produce wildly misleading numbers for any agent that mistypes a ref.

### B3. `find --namespace=CR` returns 0 with CR labels in corpus

```
$ anneal --root .design find --namespace=CR
  Matches (0)
$ anneal --root .design find "CR"
  Matches for "CR" (25)
  showing 25 of 94, offset 0
```

The corpus has plenty of `CR-D*`-style references in body text. The label-namespace filter doesn't match them — either because they're being extracted as sections rather than labels, or because the namespace filter is comparing against an empty-string namespace for labels with no separator-prefix. Either way the user sees zero with no diagnostic.

### B4. `find --kind=invalid` silently returns 0

Help lists exact accepted values for `--kind`. Parsing accepts any string and returns zero matches. Should reject with clap enum validation like `query handles --kind`.

```
$ anneal --root .design find "spec" --kind=invalid
  Matches for "spec" (0)
```

### B5. `describe foobarbaz` silently returns 0

```
$ anneal --root .design describe foobarbaz
(0 rows)
```

Should hint at `anneal verbs` or `anneal schema`. Cold agents will assume the binary is broken.

### B6. `context --hits --limit` claim of alias is wrong

Already covered. `--limit` and `--hits` are not aliases:

```
$ anneal context "convergence" --hits=2 --limit=5 --json | jq '.hits|length'
4
```

---

## Recommendations

Ordered by impact for ergonomic quality.

### R1. Unify the flag dialect (HIGH IMPACT)

Pick one. Either:

- **A: All commands accept `--format`, `--explain*`, `--area`, `--recent`, `--since`** and silently ignore where meaningless.
- **B: Each command rejects flags it can't act on, and help lists only those it accepts.**

Today's hybrid is the worst of both. Prelude rejects `--area` and compat silently accepts it. Compat rejects `--format` and prelude requires it. Document the split explicitly in `--help` or eliminate it.

### R2. Fix or remove `prime`'s global flags (HIGH IMPACT)

`prime --json` not producing JSON is a documentation lie. Either:
- Hide all global flags on `prime` (it's a static briefing).
- Implement `prime --json` to emit a structured `{briefing: "..."}` JSON.

Same for `init`'s `--area`, `--recent`, `--since`, `--plain`, `--minimal`, `--no-color` — hide them.

### R3. Reject unknown enum values (HIGH IMPACT)

`find --kind=invalid` and (by extension) any `--namespace=NONEXISTENT`, `--status=BOGUS`, `--category=WAT` should error with the enum-list, not silently return zero rows. clap supports `value_parser` enums; `query handles --kind` already does this — apply the same rigor to `find` and `garden`.

### R4. Validate `diff` refs (HIGH IMPACT)

Run `git rev-parse --verify <REF>` (or whatever the loader uses) before computing the diff. If invalid, error with `error: ref 'X' not found; use a snapshot index, --days=N, or a git ref`.

### R5. Document `--explain` on every prelude verb (MEDIUM)

Add to each prelude verb's help:

```
  --explain                  Include derivation trees for first 3 rows
  --explain-first <N>        Include derivation trees for first N rows
  --explain-all              Include derivation trees for every row
  --explain-depth <N>        Derivation expansion depth
```

Or, since this is shared, group them as "Provenance" in the global options block and document once.

### R6. Make `context`'s `--limit` a true alias or rename (MEDIUM)

Either:
- Make `--limit` truly equal `--hits` (one verb arg, last-wins).
- Rename `context --limit` to something domain-specific, or drop it.

The current "alias" docstring is wrong; the verb body responds to both args.

### R7. Fix `find --namespace` (MEDIUM)

Investigate why `--namespace=CR` returns 0 when `CR-D1` etc are present. If labels are being extracted as sections instead, document that. If the namespace separator is wrong, fix the matcher.

### R8. Backfill empty help on `explain *` and `query *` (MEDIUM)

Every `explain diagnostic` flag and several `query` subcommand flags ship without help descriptions. Cold agents have no idea what `--handle` means on `explain suggestion`. Quick win — these are one-line additions in clap.

### R9. Distinguish `--refs` / `--trace` / `--full` on `get` in text mode (LOW)

Three flags that produce identical text output mislead users. Either:
- Render distinct text formatting per flag.
- Mark `--refs`/`--trace`/`--full` as JSON-only in the help.

### R10. `--render=text` and `--render=dot` should not require `--full` (LOW)

Today:

```
$ anneal map --render=text
error: full graph rendering requires --full; use `anneal map --render=text --full` or focus with --around/--concern/--area
```

If the safety rail is necessary, document `--render=text` as "with --full" in help. Currently `--render` accepts `text` and `dot` as enum values, then immediately errors — confusing pattern.

### R11. Mark `--pretty` as JSON-only and no-op on NDJSON (LOW)

Help already says "Only applies with --json" but doesn't note that NDJSON-emitting commands don't honor it. Either:
- Implement pretty NDJSON (one indented JSON per line).
- Document explicitly: `--pretty` works only on commands that emit a single JSON object.

### R12. Hide global flags that don't apply to a command (LOW)

`anneal status --help` should not list `--area`/`--recent`/`--since`/`--minimal`/`--no-color` if they will error when passed. clap supports `hide_short_help`/`hide` on global args per-command via custom derive — apply it.

---

## Test methodology

All tests run against `/path/to/anneal/target/release/anneal` (v0.11.1, freshly built from HEAD as of 2026-05-20). Primary corpus: `.design/` of this repo (20 files, 824 handles). All invocations executed from `/path/to/anneal` with `--root .design` unless noted. Output piped non-interactively (no TTY), so NDJSON-when-piped mode is active for prelude verbs.
