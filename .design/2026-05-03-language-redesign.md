---
status: draft
date: 2026-05-03
depends-on:
  - anneal-spec.md
  - 2026-04-15-areas-orient-garden.md
  - 2026-04-17-cli-ux-audit-v2.md
  - 2026-04-21-orient-frontier-foundation.md
  - 2026-04-02-query-explain-spec.md
---

# Language-First Redesign

A proposal to collapse anneal's 14-command CLI into one query language, one
prelude of vocabulary, and seven verbs that double as worked examples.
Convergence becomes the tool's stated identity rather than a property
emergent from a graph database. Project corpora extend the tool's vocabulary
through plain-text rules, not bespoke commands.

This document supersedes the CLI surface in §12 of `anneal-spec.md` and
the query / explain surfaces in `2026-04-02-query-explain-spec.md`. The
underlying model (handles, graph, lattice, snapshots, linear obligations,
local checks) is unchanged. What changes is what the user sees and how
they extend it.

---

## Part I: Motivation

### §1 The current CLI is fourteen commands of mechanism

`anneal-spec.md` §12 lists fifteen commands (`status`, `check`, `get`,
`find`, `map`, `init`, `impact`, `obligations`, `diff`, `query`,
`explain`, `areas`, `orient`, `garden`, `prime`). Each has its own flag
set, its own JSON shape, its own help text. Together they package every
graph operation an agent might want.

Three problems become visible once an agent uses this surface in earnest:

**§1.1 The agent must memorize fourteen surfaces.** Each command answers
one kind of question. New questions ("which formal specs cite an
unanswered OQ in the compiler area?") fall outside any single command,
so the agent strings together two or three calls and parses three
envelope shapes.

**§1.2 The interface speaks graph, the marketing speaks convergence.**
`anneal-spec.md` §2 grounds the tool in context physics: documents have
potential energy, work dissipates it, entropy reintroduces it, the tool
makes the landscape visible. The CLI doesn't reflect this. `impact`,
`map`, `obligations`, `areas` are graph operations; an agent has to
mentally translate every question through the graph to use them. The
convergence frame sits in documentation but not in the action surface.

**§1.3 Project-specific concepts can't extend the surface.** A research
corpus might care about "decision documents" or "release blockers." Today
those are queries the agent writes inline every time, or wraps in shell
aliases. The tool's vocabulary is fixed by the binary; the corpus can
configure the lattice but cannot teach anneal new nouns.

### §2 The redesign in one sentence

Replace the fourteen commands with one Datalog dialect, ship the
convergence vocabulary as a plain-text prelude the agent can read,
expose seven verbs that print their underlying queries as teaching
examples, and let projects extend the vocabulary by adding rules to
`anneal.dl`.

The mental model collapses to: **read `anneal.dl`, run verbs, write
queries.** Every existing capability is reachable; new capabilities are
written by the agent or by the corpus.

### §3 Design principles [LR-P]

The principles from `anneal-spec.md` §3 carry over unchanged. This
proposal adds five:

**[LR-P1] One language, no commands.** All graph and convergence
questions are queries against one Datalog dialect. Verbs are saved
queries, not separate code paths.

**[LR-P2] The prelude is the manifesto.** What "convergence" means is
not hidden in the binary. It is a directory of `.dl` files at the install
path, readable with `cat`. Reading the prelude is reading what the tool
believes.

**[LR-P3] Verbs teach by self-reference.** Every verb prints its
underlying query above its result. Agents learn the language and the
predicate library by watching verbs work.

**[LR-P4] Project extends vocabulary, not interface.** A corpus's
specific notion of "stuck," "release blocker," "decision doc" lives in
`anneal.dl` as rules. Verbs and queries pick up those names as
first-class predicates.

**[LR-P5] Convergence is opt-in.** Corpora without a lattice get a
graph database with check rules. Corpora with one get the full
convergence vocabulary. The same tool serves both; the available
vocabulary differs.

---

## Part II: Architecture

### §4 Three layers [LR-D1]

**Definition LR-D1 (Layers).** The tool decomposes into three layers
with sharp responsibilities:

| Layer | Form | Responsibility |
|---|---|---|
| **Engine** | Rust binary | parser, graph, query evaluator, time travel, diagnostic emission machinery, predicates that need code |
| **Prelude** | `.dl` files at `<install>/share/anneal/prelude/` | convergence vocabulary, check rules, ranking, helpers — anything expressible as Horn clauses |
| **Project** | `anneal.toml` + `anneal.dl` in the corpus | handle conventions, lattice config, project predicates, overrides |

The boundary between engine and prelude is principled: anything
expressible as a Horn clause moves to the prelude; anything that needs
parsing, IO, traversal, or universe-spanning aggregation stays in
Rust. This puts the convergence model entirely in the prelude, which
means the agent can read it, override it, or fork it.

### §5 Loading and resolution [LR-D2]

**Definition LR-D2 (Load order).** On every invocation the engine
loads, in order:

1. The prelude directory (`.dl` files in lexical order, all of them)
2. `anneal.dl` in the corpus root, if present
3. Inline rules from `where` clauses in the current query, if any
4. The query itself

Later layers shadow earlier layers by predicate name. **Shadowing is
total replacement**, not merge. To extend a prelude predicate rather
than replace it, the user provides multiple clauses for the same head
in `anneal.dl`; multi-clause definitions union as Datalog naturally
does. When `anneal.dl` defines a name the prelude also defines, the
engine warns at load on stderr:

```
warning: anneal.dl:42: 'blocked/1' overrides prelude (2 clauses)
         compare: cat $(anneal --prelude-path)/convergence.dl
```

The prelude is owned by the installer and never edited by users.
Updates ship with the binary. Customization happens in `anneal.dl`,
which the installer never touches.

---

## Part III: The Language [LR-L]

### §6 Grammar

Modern Datalog. Named fields, lowercase identifiers, `:=` for "is true
when," `?` for queries, `*relation{...}` for stored data.

```
program     := statement*
statement   := fact | rule | query | directive

fact        := head '.'
rule        := head ':=' body '.'
query       := '?' [local_rules] body '.'
directive   := 'include' string '.'
             | 'at' '(' string ')' '{' statement* '}'

head        := ident '(' arg_list ')'
local_rules := ('where' rule)+
body        := atom (',' atom)*
atom        := stored | derived | comparison | aggregation | negation
stored      := '*' ident '{' field_list '}'
derived     := ident '(' arg_list ')'
comparison  := value op value
negation    := 'not' (stored | derived)
aggregation := value '=' agg_fn '{' var ':' body '}'

field_list  := field (',' field)*
field       := ident                        # bind: same name as variable
             | ident ':' value_or_var       # bind: explicit
arg_list    := value_or_var (',' value_or_var)*
value_or_var := var | literal | '_'
var         := /[a-z_][a-z0-9_]*/
literal     := string | number | bool | list

agg_fn      := 'Count' | 'Sum' | 'Min' | 'Max' | 'Avg' | 'List' | 'Set'
op          := '=' | '!=' | '<' | '>' | '<=' | '>='
             | 'in' | 'matches' | 'contains'
             | 'starts_with' | 'ends_with'
ident       := /[a-z_][a-z0-9_]*/
```

Comments: `#` to end of line. Whitespace insignificant. Statements
terminated by `.`. Strings double-quoted with standard escapes.

### §7 Types and operators

Dynamic, four primitive types plus one composite:

| Type | Literal |
|---|---|
| String | `"OQ-37"` |
| Number | `42`, `3.14` (unified i64/f64) |
| Bool | `true`, `false` |
| Null | `null` |
| List | `[1, 2, 3]`, `["raw", "decided"]` |

No first-class records. Records exist only as patterns inside
`*relation{...}`. This keeps the language small and avoids the
equality-of-records question.

**Operators:**

| Operator | Meaning |
|---|---|
| `=` | unification or equality (context-dependent) |
| `!=` `<` `>` `<=` `>=` | comparison; numbers, strings (lexical), dates |
| `in` | `x in [a, b, c]` or `x in *list_relation` |
| `matches` | `s matches "regex"` |
| `contains` | `s contains "substring"`; list contains element |
| `starts_with` `ends_with` | string prefix / suffix |
| `+` `-` `*` `/` `%` | arithmetic on numbers |

**Built-in functions** (used in expressions, not as predicates):

```
basename(path)      length(s)        lower(s)       upper(s)
max(a, b)           min(a, b)        abs(n)         days(d1, d2)
```

### §8 Stored vs derived predicates [LR-D3]

**Definition LR-D3 (Stored relation).** A relation prefixed `*` reads
from facts produced by the engine during corpus parsing or from
configuration. Pattern-matching a stored relation binds field values
to variables.

**Definition LR-D4 (Derived relation).** A relation without `*` is
defined by one or more rules. Rules may live in the prelude (built-in
vocabulary), in `anneal.dl` (project vocabulary), or inline via
`where` clauses.

The `*` prefix is a visible marker: *this is real data, not derived*.
It tells an agent reading a query whether they're looking at corpus
state or computed inferences. Stored relations always exist (possibly
empty). Derived relations may trigger evaluation when queried.

**Engine-provided stored relations:**

```
*handle{id, kind, status, namespace, file, line, date, area, summary}
*edge{from, to, kind, file, line}
*meta{handle, key, value}              # arbitrary frontmatter
*concern{name, member}                 # concern groups from anneal.toml
*config{key, value}                    # anneal.toml as facts
*snapshot{at, id, key, value}          # history.jsonl entries
```

**Engine-provided derived predicates** (need code, not Horn clauses):

```
upstream(h, anc)               # transitive depends_on
downstream(h, desc)            # transitive reverse depends_on
impact(h, x, depth)            # reverse closure, configured edge set
freshness(h, days)             # date math
flux(h, days: N) = delta       # change rate over window
pipeline_position(h, n)        # index in lattice ordering
pipeline_position_for(s, n)    # index for status string s
cite_count(h, n)               # incoming cites
in_degree(h, n)                # incoming edges of any kind
out_degree(h, n)               # outgoing edges of any kind
discharge_count(h, n)          # incoming Discharges edges
terminal(h)                    # status in lattice.terminal
active(h)                      # not terminal
obligation(h)                  # in a linear namespace
discharged(h)                  # at least one Discharges in
token_estimate(h, n)           # file-size based
```

These are written in Rust because they need either graph traversal,
date math, file IO, or aggregation across the corpus universe — things
Datalog can express but not efficiently or clearly enough at the
expected scale.

### §9 Aggregation

Form: `agg_var = AggFn{ contributing_var : sub_body }`.

```
total_potential(area, total) :=
  total = Sum{ e : potential(h, e), area_of(h, area) }.

namespace_open_count(ns, n) :=
  n = Count{ h : *handle{id: h, namespace: ns},
                 obligation(h),
                 not discharged(h) }.

newest_in_area(area, latest) :=
  latest = Max{ d : *handle{kind: "file", area, date: d}, active(_) }.
```

Standard Datalog aggregation semantics: compute the set of values for
the contributing variable such that the sub-body holds, then collapse
with the aggregation function. Free variables outside the aggregation
form the grouping key.

### §10 Negation, recursion, stratification [LR-D5]

**Definition LR-D5 (Stratified negation).** The engine partitions
rules into strata such that any predicate referenced under `not` is
fully derived in an earlier stratum. Mutual recursion through negation
is rejected at load time with the cycle named.

Recursion without negation is allowed and uses fixed-point evaluation:
the engine applies rules until no new facts are derived, then stops.
Cycles in data terminate naturally because facts are deduplicated.

**Safety rules** (enforced at load):

1. Every variable in a rule head must appear positively in the body
2. Every variable inside `not P(...)` must be bound positively
   elsewhere in the same rule
3. No mutual recursion through negation (engine names the cycle)

**Example load error:**

```
error: cyclic negation between 'blocked' and 'advancing'
  blocked/1 (anneal.dl:48) → not advancing(h)
  advancing/1 (anneal.dl:55) → not blocked(h)
fix: derive both from a non-mutually-negated common predicate.
```

### §11 Time travel [LR-D6]

**Definition LR-D6 (Time-travel block).** An `at(<ref>) { ... }` block
scopes its body to evaluate against the corpus state at `<ref>`.
Stored relations inside the block read historical state; engine-derived
predicates re-evaluate against that state. Multiple blocks can compare
time points.

**Reference grammar:**

```
at("HEAD")                git ref (current)
at("HEAD~3")              git ref (relative)
at("v0.2.1")              git ref (tag)
at("<sha>")               git ref (explicit)
at("2026-04-01")          ISO date — resolves to nearest snapshot
at("--7days")             relative duration
at("--1week")
at("snapshot:last")       most recent snapshot
at("snapshot:abc123")     explicit snapshot id
```

**Performance classes:**

| Form | Cost | Mechanism |
|---|---|---|
| Snapshot or relative | <100ms | read `.anneal/history.jsonl` |
| ISO date | <100ms | resolve to nearest snapshot |
| Git ref | O(corpus) reparse | `git show <ref>:<path>` per file |

The `--at=<ref>` global flag is sugar for wrapping the entire query in
an `at(<ref>) { ... }` block.

### §12 Inline rules and includes

A query may declare rules visible only for that query:

```
?
  where ancient(q, d) :=
    *handle{id: q, namespace: "OQ", status: "open"},
    freshness(q, d), d > 60.
  ancient(q, d), area(q, "compiler").
```

Multiple `where` clauses allowed. Inline rules don't persist; they're
the analog of CTEs in SQL.

External files are imported with `include`:

```
include "checks/release.dl".
include "/abs/path/to/team-vocab.dl".
```

Resolved relative to the file containing the `include`. No
namespacing — rules merge into the global predicate space. Conflicts
form multi-clause definitions, with the same shadowing-warning behavior
as the prelude/anneal.dl boundary.

### §13 Output shape

Queries emit NDJSON to stdout, one record per match. Field names come
from the head's variables (or for headless queries, from the body's
bindings, last-mention-wins). Records are streamed as derived; no
whole-result buffering.

```
hot_handle(h, energy) := potential(h, energy), energy > 5.
? hot_handle(h, e).
```

Output:

```
{"h":"OQ-37","e":12}
{"h":"formal-model/v17.md","e":8}
```

**Cardinality:** set semantics by default (duplicates deduplicated).
For multiset, use explicit aggregation or include the full key in the
head.

**Provenance:** `--explain` flag adds a `_derivation` field to each
record listing the rule chain and supporting facts. Without the flag,
records are bare.

### §14 Errors

Three categories, all to stderr, all with file:line context:

**Parse errors** — surface during load:
```
anneal.dl:42:15: expected ',' or '.', got '{'
  potential(h, energy {
                      ^
```

**Static errors** — surface during load: safety violations, stratification
cycles, unknown predicates with did-you-mean suggestions, reserved
diagnostic IDs.

**Runtime errors** — surface during evaluation: regex compile failures,
time-travel ref not found, division by zero.

All three exit with code 1. Stdout stays clean — no partial NDJSON if a
query failed mid-evaluation.

---

## Part IV: The Prelude [LR-PR]

### §15 Layout

The prelude lives at `<install>/share/anneal/prelude/` as a directory
of `.dl` files. The engine loads all `.dl` files in lexical order at
startup.

```
<install>/share/anneal/prelude/
  graph.dl          # structural shapes (orphan, stub, hub)
  convergence.dl    # potential, entropy, blocked, advancing, weights
  checks.dl         # E001, E002, W001-W004, I001, S001-S005
  ranking.dl        # tier, weighted_in_degree, curated_hub
```

Path discovery: at build time the engine embeds the install prefix; at
runtime `ANNEAL_PRELUDE_PATH` overrides; for development the engine
falls back to a path relative to the binary. `anneal --prelude-path`
prints the active path.

### §16 Convergence vocabulary [LR-D7]

**Definition LR-D7 (Convergence vocabulary).** The set of predicates
defined in `convergence.dl` that name the convergence-physics concepts
of `anneal-spec.md` §2.1. These predicates are the contract between
the convergence frame and the rest of the system.

```
# convergence.dl

# Weights — edit in anneal.dl to retune for your corpus.
potential_weight("undischarged",     5).
potential_weight("broken_ref",       4).
potential_weight("stale_dep",        3).
potential_weight("confidence_gap",   3).
potential_weight("freshness_decay",  2).
potential_weight("missing_meta",     1).
potential_weight("orphan_label",     1).

# Total potential = weighted sum of entropy sources.
potential(h, energy) :=
  energy = Sum{ w : entropy(h, source), potential_weight(source, w) }.

# Entropy sources — each rule names a kind of unsettled work.
entropy(h, "undischarged") :=
  obligation(h), not discharged(h), not terminal(h).

entropy(h, "broken_ref") :=
  diagnostic("E001", _, h, _, _, _).

entropy(h, "stale_dep") :=
  *edge{from: h, to: t, kind: "depends_on"},
  active(h), terminal(t).

entropy(h, "confidence_gap") :=
  *edge{from: h, to: t, kind: "depends_on"},
  pipeline_position(h, n_h),
  pipeline_position(t, n_t),
  n_h > n_t + 1.

entropy(h, "freshness_decay") :=
  *handle{id: h, kind: "file"},
  active(h), freshness(h, days), days > 60.

entropy(h, "missing_meta") :=
  *handle{id: h, kind: "file"},
  active(h),
  not *meta{handle: h, key: "status"}.

entropy(h, "orphan_label") :=
  *handle{id: h, kind: "label"},
  cite_count(h, n: 0),
  not discharged(h).

# Blocked: high potential, no recent change.
blocked(h) :=
  active(h),
  potential(h, energy), energy >= 3,
  flux(h, days: 30) = 0.

# Advancing: actively moving forward in the lattice.
advancing(h) :=
  active(h),
  flux(h, days: 7) > 0,
  recently_advanced(h).

recently_advanced(h) :=
  at("snapshot:last") { *handle{id: h, status: prior} },
  *handle{id: h, status: current},
  pipeline_position_for(prior, p_prior),
  pipeline_position_for(current, p_current),
  p_current > p_prior.
```

The vocabulary names a small physics:

| Predicate | Meaning |
|---|---|
| `potential(h, e)` | numeric measure of unsettled work on `h` |
| `entropy(h, source)` | a specific source of disorder on `h` |
| `blocked(h)` | high potential, no recent flux |
| `advancing(h)` | recently advanced in the lattice |
| `terminal(h)` | settled (engine primitive — no convergence-layer alias) |

Renamings from `anneal-spec.md`'s informal physics: `stuck` → `blocked`
(parallel with `broken`), `flowing` → `advancing` (more concrete),
`crystallized` dropped (redundant with `terminal`). `potential` and
`entropy` retained because they are the load-bearing metaphor and the
prelude documents them.

### §17 Check rules [LR-D8]

**Definition LR-D8 (Check rule).** A rule whose head is `diagnostic(...)`
that derives a fact representing a local consistency violation. The
engine collects all such derived facts across prelude and `anneal.dl`,
exposes them through the `diagnostic/6` predicate for queries, and
provides them to verbs like `broken` and to the dashboard.

The seven check rules from `anneal-spec.md` §7 move from the engine
to `checks.dl`. Each rule is a Horn clause; the engine no longer has
hard-coded check logic. Example:

```
# checks.dl

diagnostic("E001", "error", src, file, line, broken_ref(target)) :=
  *edge{from: src, to: target, kind: _, file, line},
  not *handle{id: target}.

diagnostic("W001", "warning", src, file, line,
           stale_ref(s_status, t_status)) :=
  *edge{from: src, to: target, kind: "depends_on", file, line},
  active(src), terminal(target),
  *handle{id: src, status: s_status},
  *handle{id: target, status: t_status}.

diagnostic("W002", "warning", src, file, line,
           confidence_gap(n_s, n_t)) :=
  *edge{from: src, to: target, kind: "depends_on", file, line},
  pipeline_position(src, n_s),
  pipeline_position(target, n_t),
  n_s > n_t + 1.

# … E002, W003, W004, I001, S001-S005 same shape.
```

Every diagnostic-emitting rule must have a string literal in arg 1.
The engine enforces this at load (see §18).

### §18 Diagnostic ID stability [LR-R1]

**Rule LR-R1 (Diagnostic ID literal).** Every rule whose head is
`diagnostic(...)` must have a string literal as the first argument. A
variable in arg 1 is a load error.

**Rule LR-R2 (Unique within ruleset).** Two rules with the same
diagnostic ID literal in the same loaded ruleset error at load with
both file:line locations.

**Rule LR-R3 (Reserved prefixes).** The prefixes `E*`, `W*`, `I*`, `S*`
are prelude-owned. User rules in `anneal.dl` using these prefixes error
at load. Users pick their own prefix (`PROJ-001`, `RELEASE-002`,
`TEAM-007`).

Load error example:

```
error: anneal.dl:84: diagnostic ID 'E099' is in reserved range E*
       use a project-specific prefix (e.g. 'PROJECT-099')
```

This makes diagnostic ID stability mechanical and validated, replacing
the comment-annotation convention sketched in earlier drafts.

---

## Part V: CLI and I/O [LR-C]

### §19 Verbs

Seven verbs, plus three meta. Each verb is a saved query against the
prelude. Each prints its underlying query as a comment above its
result.

| Verb | Question | Underlying query (sketch) |
|---|---|---|
| `anneal` | where am I | `? dashboard().` |
| `anneal H` | what is this handle | `? resolve(H, info).` |
| `anneal find TEXT` | where's the thing called TEXT | `? *handle{id, ...}, id contains "TEXT".` |
| `anneal work` | where should I work | `? potential(h, e), entropy(h, srcs). sort -e, limit 25.` |
| `anneal blocked H` | what's blocking H | `? entropy("H", source), entropy_detail("H", source, d).` |
| `anneal trend` | which way is the corpus moving | `at(--at) { ... } vs at("now") { ... }` |
| `anneal broken` | are there errors | `? diagnostic(_, "error", _, _, _, _).` |

Meta:

| Form | Purpose |
|---|---|
| `anneal -e '<q>'` | custom query; `-e -` reads from stdin |
| `anneal help [topic]` | reference: `language`, `predicates`, `time-travel`, `schema`, `verbs` |
| `anneal init` | set up corpus: write `anneal.toml` + minimal `anneal.dl` |
| `anneal --prelude-path` | print install path of the prelude |
| `anneal --inspect S` | parse-test a string against handle conventions |

The verb naming criterion: an agent who has never read the docs should
guess what each verb does. `hot` (rejected) requires explanation. `work`
maps to the agent's question ("where should I work?"). `blocked` is
parallel to `broken` and reads naturally. `trend` is conventional.
`find` is conventional.

Verbs whose work doesn't apply in graph mode (no lattice configured)
print:

```
$ anneal work
anneal: 'work' requires a convergence model
       this corpus has no lattice (anneal.toml has no [convergence])
       see: anneal help convergence
```

### §20 Flags

Operational only — never query-shaping. Filters belong in queries.

| Flag | Effect | Scope |
|---|---|---|
| `--root=PATH` | operate on a different corpus | global |
| `--at=<ref>` | evaluate at a historical reference | global |
| `--limit=N` | cap output records | global |
| `--explain` | include `_derivation` per record | global |
| `--no-snapshot` | don't append history on this run | global |
| `--quiet` | suppress stderr chatter | global |
| `--budget=N` | token-budgeted reading list | `work` |
| `--gate` | exit 1 if any results | `broken` |
| `--section=NAME` | print one prelude section | `--print-stdlib` |

Nine flags total. None do filtering. None are sugar for query
expressions. Each one is operational (where, when, how much, what
extras).

### §21 I/O contract [LR-D9]

**Definition LR-D9 (I/O contract).** The behavior every consumer of
anneal can rely on:

- **stdout: NDJSON.** One record per line, `\n` terminated, streamed
  as derived. The dashboard verb is a single nested record; every other
  verb is a stream of homogeneous records. No envelope unless `--meta`.
- **stderr: human text.** Progress, warnings, parse errors with
  line/column. Never NDJSON. Pipes to `/dev/null` cleanly.
- **stdin: `-` means stdin.** `anneal blocked -` reads handles, one per
  line. `anneal -e -` reads a query (heredoc-friendly). Never ambiguous.
- **Exit codes:** 0 success (including empty results), 1 query error,
  2 invocation error, 3 gate failure (`--gate` with non-empty results).
  Empty results are not errors.
- **`--color=auto`** detects TTY. Pipes always get plain text. No flag
  needed.

Composition is whatever the shell already provides: `jq` for shape
filtering, `xargs` for per-line iteration, `fzf` for selection,
`column` for tables, `tee` for branching. anneal is one node in a
Unix pipeline, not a pipeline of its own.

### §22 Help [LR-D10]

**Definition LR-D10 (Help dialog).** A single screen that introduces
the model, lists the verbs, sketches the language, points at the
prelude, and ends with the agent loop. Reading it once suffices for
competent use.

```
anneal — track convergence in a knowledge corpus

THE MODEL
  Corpora carry potential energy. Work dissipates it. Entropy
  reintroduces it. anneal makes the landscape visible and tracks
  how it shifts over time.

VERBS
  anneal              dashboard
  anneal H            resolve a handle
  anneal find TEXT    substring search
  anneal work         ranked by potential — where to work
  anneal blocked H    what's blocking H from advancing
  anneal trend        convergence over time (--at controls window)
  anneal broken       errors (--gate for hooks)
  anneal -e '<q>'     custom query
  anneal help <topic> language, predicates, schema, time-travel
  anneal init         set up corpus

  Every verb prints its underlying query above its result.

LANGUAGE
  rule:    head(args) := body, body, body.
  query:   ? body.
  joins:   shared variable names unify
  recur:   rules can reference themselves
  time:    at("HEAD~3") { ? <q>. }    or  --at=<ref>

VOCABULARY
  cat $(anneal --prelude-path)/convergence.dl   to read what
  potential, blocked, advancing, entropy mean for this tool.
  Override any of them in anneal.dl.

THE LOOP
  1. anneal              see the landscape
  2. anneal work         pick where to work
  3. anneal blocked H    understand why
  4. (do the work)
  5. anneal trend        confirm potential dissipated
```

`anneal help language`, `anneal help predicates`, `anneal help time-travel`,
`anneal help schema` provide the deeper references. `anneal help verbs`
prints the table of verbs with their underlying queries.

---

## Part VI: Handle model [LR-H]

### §23 Kinds (unchanged)

The five handle kinds from `anneal-spec.md` §4.1 are preserved:
`file`, `section`, `label`, `version`, `external`. Discovery logic is
engine code; the patterns that drive discovery are configuration.

### §24 Discovery configuration

Project handle conventions live in `anneal.dl` as facts the engine
consumes during discovery (before query evaluation):

```
# === handles ===
file_extension(".md").
file_extension(".mdx").

scan_root(".").
scan_exclude("node_modules").
scan_exclude(".git").

label_pattern("OQ",    "OQ-(\d+)",    "any").
label_pattern("KB-D",  "KB-D(\d+)",   ".design/**").
label_pattern("KB-OQ", "KB-OQ(\d+)",  ".design/**").

linear_namespace("OQ").
linear_namespace("P").

version_pattern("formal-model", "formal-model-v(\d+)\.md").
external_in_frontmatter("references").

section_min_depth(1).
section_max_depth(3).
```

Discovery facts and query rules coexist in `anneal.dl`. They are
distinguished by section header convention (`# === handles ===` at the
top, `# === overrides ===` for predicate replacements, `# === project ===`
for project predicates). The engine consumes discovery facts at parse
time and rules at query time.

The five kinds are engine-fixed because each requires bespoke
discovery logic (path walking, markdown AST, regex, frontmatter).
A "new kind" — say, "decision document" — is just a predicate over
existing handles:

```
decision_doc(h) :=
  *handle{id: h, kind: "file"},
  *meta{handle: h, key: "status", value: v},
  v in ["decision", "decided"].
```

### §25 Introspection

Five ways an agent learns the handle landscape of a corpus:

```
# 1. Counts by kind
anneal -e '? c = Count{*handle{kind: k, ...}}, k.'

# 2. Label namespaces and counts
anneal -e '? c = Count{*handle{kind:"label", namespace: ns, ...}}, ns.'

# 3. The corpus's discovery conventions
anneal -e '? label_pattern(ns, regex, scope).'

# 4. Inspect a specific string
anneal --inspect "OQ-99"
# kind: label, namespace: OQ, resolved: false (no handle with this id)

# 5. Read the file directly
cat anneal.dl
anneal help handles
```

`--inspect` is the debugger for "why isn't this thing showing up?" and
the single most useful introspection tool when conventions are
unfamiliar.

---

## Part VII: Convergence as opt-in [LR-CV]

### §26 Two modes [LR-D11]

**Definition LR-D11 (Mode).** A corpus runs in one of two modes:

- **Convergence mode**: a lattice is configured (in `anneal.toml`'s
  `[convergence]` block, or inferred by `init` from `status:`
  frontmatter). All convergence vocabulary is meaningful; all verbs
  work; the dashboard includes trend, work, blocked, advancing.
- **Graph mode**: no lattice is configured. Convergence verbs print a
  helpful error directing the user to enable convergence. The dashboard
  shows only graph-flavored facts (file/handle/edge/diagnostic counts).

The engine, prelude, and language are identical in both modes. Only
the available vocabulary differs.

### §27 init detection

`anneal init` detects mode from frontmatter prevalence:

```
$ anneal init

scanning corpus...
  found 47 files
  status frontmatter: present in 41/47 (87%)
  inferred lattice: raw → digested → decided → formal → verified

mode: convergence
prelude: <install>/share/anneal/prelude/  (4 files, 64 rules)

wrote anneal.toml (engine config + [convergence] block)
wrote anneal.dl   (12 lines: handle conventions and stubs)

next steps:
  cat $(anneal --prelude-path)/convergence.dl   # what convergence means
  anneal                                        # see the landscape
  anneal work                                   # find where to work
```

Or, when frontmatter is sparse:

```
$ anneal init

scanning corpus...
  found 263 files
  status frontmatter: present in 4/263 (2%)
  no lattice detected

mode: graph
prelude: <install>/share/anneal/prelude/  (graph + checks only)

wrote anneal.toml (no [convergence] block)
wrote anneal.dl   (8 lines: handle conventions)

note: convergence verbs (work, blocked, trend) require a lattice.
      to enable, add status frontmatter to your files and rerun
      `anneal init`, or define [convergence] manually in anneal.toml.
```

### §28 The agent loop [LR-D12]

**Definition LR-D12 (Convergence loop).** The five-step cycle an
agent executes against a convergence-mode corpus:

```
1. anneal              see the landscape
2. anneal work         pick where to work
3. anneal blocked H    understand why H isn't moving
4. (do the work)
5. anneal trend        confirm potential dissipated
```

Step 5 closes the loop by validating that the work actually reduced
potential. `anneal trend --at=--10min` is the immediate-feedback form;
`anneal trend` alone (default `--at=--7days`) is the session-summary
form.

In graph mode the loop degenerates to:

```
1. anneal              see the landscape
2. anneal broken       find the errors
3. (fix them)
4. anneal broken       confirm clean
```

---

## Part VIII: Files and layout [LR-F]

### §29 Project files

```
<corpus>/
  anneal.toml           # engine config: lattice, scan rules, snapshot path
  anneal.dl             # handle conventions + project predicates + overrides
  .anneal/
    history.jsonl       # snapshot append log
```

`anneal.toml` retains the same role as in `anneal-spec.md` §13: engine
configuration that doesn't fit Datalog (lattice ordering, snapshot
behavior, paths). `anneal.dl` is new — it holds discovery facts,
project predicates, and prelude overrides.

### §30 Prelude files

```
<install>/share/anneal/prelude/
  graph.dl              # orphan, stub, hub, structural shapes
  convergence.dl        # potential, entropy, blocked, advancing
  checks.dl             # E001, E002, W001-W004, I001, S001-S005
  ranking.dl            # tier, weighted_in_degree, curated_hub
```

Owned by the installer. Users never edit. Versioned with the binary.

### §31 Snapshots

Snapshot semantics from `anneal-spec.md` §10 are unchanged. Snapshots
are appended to `.anneal/history.jsonl` by every `anneal`, `anneal trend`,
and `anneal broken` invocation (suppressed by `--no-snapshot`). The
`*snapshot{at, id, key, value}` stored relation exposes them to
queries; `at(<ref>) { ... }` blocks read from them transparently.

---

## Part IX: Migration [LR-M]

### §32 Mapping from current commands

Every existing command is reachable in the new surface:

| Today | New form |
|---|---|
| `anneal status` | `anneal` |
| `anneal status --json --compact` | `anneal --limit=summary` (or just `anneal`) |
| `anneal get H` | `anneal H` |
| `anneal get H --refs` | `anneal H` (refs included by default) |
| `anneal get H --trace` | `anneal H --explain` |
| `anneal find TEXT` | `anneal find TEXT` |
| `anneal find --namespace=OQ --status=open` | `anneal -e '? *handle{kind:"label", namespace:"OQ", status:"open"}.'` |
| `anneal check` | `anneal broken` (errors) or `anneal -e '? diagnostic(c, s, ...).'` (any severity) |
| `anneal check --errors-only` (hooks) | `anneal broken --gate` |
| `anneal check --suggest` | `anneal -e '? diagnostic(c, "suggestion", ...).'` |
| `anneal check --file=X` | `anneal -e '? diagnostic(c, s, h, "X", l, e).'` |
| `anneal map` | `anneal -e '? *edge{from, to, kind}.'` (plus a renderer downstream) |
| `anneal map --around=H --depth=2` | `anneal -e '? impact("H", x, depth), depth <= 2.'` |
| `anneal impact H` | `anneal -e '? impact("H", x, depth).'` |
| `anneal obligations` | `anneal -e '? obligation(h), disposition(h).'` |
| `anneal diff` | `anneal trend` |
| `anneal diff HEAD~3` | `anneal trend --at=HEAD~3` |
| `anneal query handles --kind label` | `anneal -e '? *handle{kind:"label", ...}.'` |
| `anneal query diagnostics --severity warning` | `anneal -e '? diagnostic(c, "warning", ...).'` |
| `anneal explain diag_xyz` | `anneal -e '? diagnostic(c, ...).' --explain` |
| `anneal areas` | `anneal -e '? area_health(area, grade, ...).'` |
| `anneal orient` | `anneal work` |
| `anneal orient --budget=50k` | `anneal work --budget=50k` |
| `anneal orient --file=F` | `anneal -e '? candidate_for_file("F", c, tier, score).' \| greedy_fill --budget=50k` |
| `anneal garden` | `anneal -e '? maintenance_task(t, category, blast).'` (project-defined ranking) |
| `anneal init` | `anneal init` |
| `anneal prime` | `anneal help` |

The new surface is strictly more expressive: questions that fall
outside any current command are reachable through `-e` queries, and
common patterns can be promoted to verbs by the project (via aliases
or saved-query files).

### §33 Migration path

The migration is non-trivial but bounded. Sketch:

1. **Implement the language interpreter.** Build on `crepe` or `ascent`
   (Rust Datalog crates). Estimate: ~2k lines, several weeks.
2. **Author the prelude.** Translate the seven check rules and the
   convergence vocabulary from Rust to `.dl`. Estimate: ~300 lines of
   Datalog, ~1 week including tests.
3. **Rewrite the verb dispatch.** Each verb becomes a saved query plus
   a thin Rust wrapper for non-Datalog work (greedy fill, gate exit
   codes). Estimate: ~500 lines, ~1 week.
4. **Deprecate the old commands.** Ship both surfaces in parallel for
   one minor release; emit deprecation warnings on old commands;
   remove them in the next minor release.
5. **Update the skill briefing.** Rewrite `skills/anneal/SKILL.md` to
   teach the new surface. Roughly half the size — fewer commands, more
   language.

The `anneal-spec.md` document itself becomes a candidate for
restructure: §12 (CLI surface) is replaced by this document; §13
(configuration) gets a new subsection on `anneal.dl`; §14
(architecture) gains the engine/prelude/project boundary; §15 (crate
structure) reorganizes around the language interpreter.

### §34 What stays unchanged

The core model from `anneal-spec.md` Parts I-III is preserved verbatim:

- Handle definition (KB-D1) and the five kinds (KB-D2)
- Graph construction (KB-D5, KB-D6)
- Convergence lattice (KB-D7-KB-D12)
- Local check semantics (KB-D13)
- Linearity (KB-D15)
- Impact analysis (KB-D16)
- Convergence tracking (KB-D17, KB-D18, KB-D19)
- Design principles (KB-P1 through KB-P9)
- Theoretical lineage (KB-F1 through KB-F5)

What changes is the *interface* to these concepts and *how the corpus
extends them*. The model itself is sound.

---

## Part X: Worked example [LR-X]

### §35 An agent's session

An agent lands in `~/code/anneal/.design/`. They've been told:
*"The v0.3 release prep is in progress. Help advance it."*

```
$ anneal
{"files":47,"handles":312,"active":142,"terminal":345,
 "diagnostics":{"errors":0,"warnings":3},
 "trend":{"window_days":7,"potential_delta":-8,"direction":"advancing"},
 "work":[{"handle":"OQ-37","energy":12,"sources":["undischarged","stale_dep"]},
         {"handle":".design/anneal-spec.md","energy":8,"sources":["confidence_gap"]}],
 "advancing":[{"handle":"src/cli/orient.rs","from":"provisional","to":"current"}],
 "blocked":[{"handle":"OQ-12","days_unchanged":48,"energy":7}]}
```

Reading: 0 errors, 3 warnings, trend advancing, two hot spots, OQ-37
clearly the highest-priority work.

```
$ anneal -e '? release_blocker(h, why).'
# (using project-defined release_blocker/2 in anneal.dl)
{"h":"OQ-37","why":"blocking_oq"}
```

The project's release-blocker predicate (defined in `anneal.dl`)
agrees with the convergence ranking: OQ-37 is the v0.3 blocker.

```
$ anneal blocked OQ-37
# ? entropy("OQ-37", source), entropy_detail("OQ-37", source, detail).
{"source":"undischarged","detail":{"namespace":"OQ","days_open":82}}
{"source":"stale_dep","detail":{"upstream":".design/synth/discharge.md","status":"superseded"}}
```

OQ-37 is undischarged in the OQ namespace (open 82 days) and has a
stale dependency on a synthesis doc that has been superseded.

```
$ anneal -e '? *edge{from: x, to: ".design/synth/discharge.md", kind: "supersedes"}.'
{"x":".design/synthesis/2026-04-15-discharge-v2.md"}
```

The supersession chain leads to a v2 synthesis from April.

```
$ anneal work --budget=20k
# pipeline: ? candidate(file, tier, score) → fill(budget: 20000).
{"file":".design/anneal-spec.md","tier":"pinned","tokens":18500}
{"file":".design/synthesis/2026-04-15-discharge-v2.md","tier":"frontier","tokens":1200}
```

Reading list within budget. The agent reads, edits OQ-37 to point at
the v2 synthesis, and confirms:

```
$ anneal trend --at=--10min
{"potential_delta":-12,"sources_cleared":["stale_dep","undischarged"],
 "handles_advanced":["OQ-37"]}
```

12 units of potential dissipated. Two entropy sources cleared on
OQ-37. Convergence advanced.

### §36 Pre-commit hook

```sh
#!/bin/sh
# .git/hooks/pre-commit
anneal broken --gate
```

One line. `--gate` returns exit 1 if any errors exist, exit 0 otherwise.
The shell hook needs no shell logic.

### §37 Project vocabulary in `anneal.dl`

The agent's session referenced `release_blocker/2`, which is project-
defined:

```
# anneal.dl (excerpt)

# OQs become "blocking" when a formal-status doc transitively depends on them.
blocking_oq(q) :=
  *handle{id: q, kind: "label", namespace: "OQ", status: "open"},
  upstream(spec, q),
  *handle{id: spec, status: "formal"}.

# Release blockers — what must clear before v0.3.
release_blocker(h, "broken_ref")    := diagnostic("E001", _, h, _, _, _).
release_blocker(h, "undischarged")  := diagnostic("E002", _, h, _, _, _).
release_blocker(h, "blocking_oq")   :=
  blocking_oq(h),
  *meta{handle: h, key: "milestone", value: "v0.3"}.
```

Three rules. Now `release_blocker` is a first-class predicate that any
agent can query without rewriting the logic. The corpus has *extended
the tool's vocabulary* without changing the tool.

---

## Part XI: Open questions [LR-OQ]

### §38 Greedy fill in `work --budget` [LR-OQ1]

The `--budget` reading-list fill is iterative selection with a running
sum, which Datalog handles awkwardly. Currently a ~10-line wrapper in
the verb. A more powerful Datalog (with `take_until`-style aggregation,
as Cozo offers) could express it natively. Worth investigating but not
blocking.

### §39 Multi-corpus federation [LR-OQ2]

A real research project might span `anneal/.design/` +
`murail/.design/` + `herald/.design/`. `--root=PATH` handles one corpus
at a time. Cross-corpus queries (*"murail's OQ-12 cites anneal's
spec"*) aren't supported. Out of scope for this redesign; flagged for
future consideration.

### §40 Prelude / engine version drift [LR-OQ3]

If the package manager updates the prelude separately from the binary,
versions can drift. Mitigation: each prelude file carries
`# anneal-prelude version X.Y.Z` at the top, and the engine emits a
non-fatal warning on mismatch. The user might be intentionally running
a newer prelude with an older engine.

### §41 Multi-clause definitions across files [LR-OQ4]

When `anneal.dl` adds clauses to a predicate also defined in the
prelude, the engine warns that the prelude is shadowed. But what if
two `include`d files both add clauses to the same predicate? Probably
fine (the rules union), but the warning behavior should be consistent.
Worth pinning down before implementation.

### §42 Section parsing for sub-handle queries [LR-OQ5]

`anneal formal-model/v17.md:§14.3` resolves to a section handle. The
query language doesn't currently have sugar for "all handles whose
parent file is X." Would need either a `parent_file(section, file)`
predicate or path syntax in pattern matching. Not urgent but worth
noting.

### §43 Performance ceiling [LR-OQ6]

A corpus could in principle write thousands of rules in `anneal.dl`.
Datalog handles this but evaluation time grows. For the corpus sizes
the spec considers (~hundreds to thousands of files, dozens of rules),
fine. For a hypothetical 100k-handle corpus with hundreds of project
predicates, may matter. Profile before promising.

---

## Part XII: Labels

### LR-F (Foundations)

- LR-F1: §1.2 The interface speaks graph, the marketing speaks convergence
- LR-F2: §3 Five new design principles

### LR-P (Principles)

- LR-P1: One language, no commands (§3)
- LR-P2: The prelude is the manifesto (§3)
- LR-P3: Verbs teach by self-reference (§3)
- LR-P4: Project extends vocabulary, not interface (§3)
- LR-P5: Convergence is opt-in (§3)

### LR-D (Definitions)

- LR-D1: Layers (§4)
- LR-D2: Load order (§5)
- LR-D3: Stored relation (§8)
- LR-D4: Derived relation (§8)
- LR-D5: Stratified negation (§10)
- LR-D6: Time-travel block (§11)
- LR-D7: Convergence vocabulary (§16)
- LR-D8: Check rule (§17)
- LR-D9: I/O contract (§21)
- LR-D10: Help dialog (§22)
- LR-D11: Mode (§26)
- LR-D12: Convergence loop (§28)

### LR-R (Rules)

- LR-R1: Diagnostic ID literal (§18)
- LR-R2: Unique within ruleset (§18)
- LR-R3: Reserved prefixes (§18)

### LR-C (Commands / Verbs)

- LR-C1: `anneal` (dashboard) (§19)
- LR-C2: `anneal H` (resolve) (§19)
- LR-C3: `anneal find` (§19)
- LR-C4: `anneal work` (§19)
- LR-C5: `anneal blocked` (§19)
- LR-C6: `anneal trend` (§19)
- LR-C7: `anneal broken` (§19)
- LR-C8: `anneal -e` (custom) (§19)
- LR-C9: `anneal help` (§19)
- LR-C10: `anneal init` (§19)

### LR-M (Migration)

- LR-M1: Mapping from current commands (§32)
- LR-M2: Migration path (§33)
- LR-M3: What stays unchanged (§34)

### LR-OQ (Open Questions)

- LR-OQ1: Greedy fill in `work --budget` (§38)
- LR-OQ2: Multi-corpus federation (§39)
- LR-OQ3: Prelude / engine version drift (§40)
- LR-OQ4: Multi-clause definitions across files (§41)
- LR-OQ5: Section parsing for sub-handle queries (§42)
- LR-OQ6: Performance ceiling (§43)

### LR-X (Worked Examples)

- LR-X1: Agent session (§35)
- LR-X2: Pre-commit hook (§36)
- LR-X3: Project vocabulary (§37)

---

## References

### Internal

- `anneal-spec.md` — current authoritative spec (this proposal supersedes §12)
- `2026-04-15-areas-orient-garden.md` — `orient` semantics absorbed into `work`
- `2026-04-17-cli-ux-audit-v2.md` — surface friction this proposal resolves
- `2026-04-21-orient-frontier-foundation.md` — tier ranking moved to `ranking.dl`
- `2026-04-02-query-explain-spec.md` — `query` and `explain` surfaces unified into the language

### External

- Cozo (https://github.com/cozodb/cozo) — modern Datalog with named fields, `:=` syntax, `*relation{...}` patterns
- Logica (https://logica.dev) — Google's Datalog with named columns and SQL-aware compilation
- Mangle (https://github.com/google/mangle) — typed Datalog with friendlier surface
- ascent (https://github.com/s-arash/ascent) — Rust Datalog crate, candidate for the engine
- crepe (https://github.com/ekzhang/crepe) — alternative Rust Datalog crate
