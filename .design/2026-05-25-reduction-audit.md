---
status: draft
updated: 2026-05-25
author: claude (sub-agent reduction audit)
depends-on:
  - 2026-05-19-compatibility-surface-retire-audit.md
  - 2026-05-21-code-mode-ergonomics.md
---

# Reduction Audit — v0.12.0

## Summary

35 commands evaluated. **KEEP 10, FOLD 10, CUT 15.** Default-help drops from
23 listed to 9 visible (10 KEEPs minus `help` which is implicit); callable
surface drops from 35 to 13 (10 KEEPs + 3 hidden CI/muscle-memory aliases).
Net surface reduction: **63% of callable commands cut, 61% of listed commands
gone**.

Three whole *clusters* come out together:

1. **The cookbook cluster.** `cookbook` verb + `cookbook` primitive +
   `@cookbook` annotation + `cookbook(...)` rules in `views.dl` + describe
   "cookbook recipes" cross-refs. Recipes belong on `describe NAME` as
   `Common joins` (already exists) plus a copyable `@verb` template under
   `Add to anneal.dl:`. Save is just Edit/Write on `anneal.dl`.
2. **The examples command surface.** The `examples` verb is removed, while the
   `examples(...)` primitive remains queryable and feeds `describe NAME`.
   Examples belong on the teaching card; the separate command duplicates a
   teaching surface.
3. **The `query` / `explain` typed-flag-grammar cluster.** Both are parallel
   query languages made of flags. `eval` + `--explain` is the whole point of
   v0.12. Cut the prior generation.

Plus three loose deletions: `save` (Edit/Write on `anneal.dl` is shorter),
`find` (broken `--format`, identity-substring covered by `eval` over
`*handle`), and `verbs` (collapses into `schema --kind=verb` or
`describe runtime`).

The minimum surviving surface is the one the v2.0 master spec asked for:
*one language, a tight retrieval primitive set, one teaching card surface*.

## Per-command verdicts

Verdict markers: **K**=Keep visible, **F**=Fold into another surface,
**C**=Cut entirely. `H` = keep callable but hide from default help.

### Default-help commands (23)

#### `status` — K
What: convergence landing — work, blocked, broken sections.
Justification: the philosophy carrier. Magic word "convergence" lands here.
Eval form (`? status_item(section, h, score, why).`) works but the curated
section ordering and color rendering are why the verb exists.
Magic-word impact: `status` carries convergence, blocked, advancing, broken
into the agent's first screen. Keep.

#### `context` — K
What: search + bounded read + neighborhood for a goal.
Justification: highest-progress cold-agent action; replaces 3 sequential
tool calls. The eval form is a multi-rule composite (see `views.dl`
`context_readable` / `context_hit` / `context_span` / `context_neighbor`)
— not eval-composable in practice. Strongest KEEP.

#### `prime` — F → `help agent`
What: prints the skill briefing (~5KB of agent guidance).
Justification: load-bearing onboarding text, but not a *command*. Fold into
`anneal help agent` (or `anneal help workflows`). Keep callable as hidden
alias for harness preloaders that already invoke it.
Lost: nothing — the content moves intact; only the verb name changes.
Magic-word impact: none. The vocabulary lives in the help topic.

#### `schema` — K
What: catalog of stored relations, derived predicates, primitives.
Justification: keystone for language discovery. 114 rows on .design.
Cold-agent learning experiment (2026-05-20) confirmed this is *the* killer
introspection surface. The eval form `? schema(name, kind, signature, ...).`
is the underlying primitive — but a one-word command is right here.

#### `describe` — K
What: teaching card for a runtime name.
Justification: progressive-disclosure anchor. Already shows Kind, Signature,
Common joins, Example, Source. Absorbs cookbook recipes and examples
(see folds below). The eval form `? describe("X", doc).` exists but loses
the curated card rendering.

#### `verbs` — F → `schema --kind=verb`
What: lists saved verbs with their queries and doc.
Justification: `schema` already lists verbs (the schema row with
`kind=verb`). A separate command fragments introspection. The dedicated
verb output is one rich text block per row but the same information lives
in `describe <verb_name>`.
Replacement: `anneal schema --kind=verb --format=text` for the list,
`anneal describe <name>` for the teaching card per verb. Update `schema`
to accept a `--kind=verb|stored|derived|primitive` filter (cheap).
Lost: the bulk `verbs` dump in one call. Acceptable — it was already
truncated to one-line summaries; agents who needed depth read
`describe NAME` anyway.

#### `examples` — F → `describe NAME`
What: returns the `examples(name, example)` rows for one name.
Justification: `describe` already shows `Example:` lines (one per name).
A separate command duplicates the surface. The cold-agent audit found
`examples()` is empty for primitives and derived predicates — only verbs
populate it, and `describe` shows those.
Replacement: extend `describe NAME` to show all `examples(NAME, e)` rows
under `Examples:` (currently shows one). The `examples` primitive remains
in the dialect; cut the verb.
Lost: nothing. The verb was a thin shim over `describe`.

#### `cookbook` — C (cluster cut)
What: lists 7 prelude recipes (question + template + when + save command).
Justification: every recipe is **(a)** already a `Common joins` line in
the relevant `describe NAME` card, or **(b)** a one-liner an agent would
write from `schema` + `help eval`. The `Save:` lines are pure cargo cult
— they teach a fake save when Edit/Write on `anneal.dl` is shorter and
already the right move.
Replacement: per-predicate `Common joins` in `describe NAME` (already
shipped for diagnostic, search, upstream, top_work, blocked, entropy,
undischarged per Slice 3 gate). Move the `When to reach for it` text into
the `describe NAME` `When:` line.
Lost: the question-shape browsing surface ("how do I find broken refs
in one area?"). Real but small loss; `describe diagnostic` already shows
the area-join pattern explicitly.
Magic-word impact: the word "cookbook" itself disappears. Good — it was
a noun in a tool whose pitch is verbs-and-language.
Cluster removed: `@cookbook` annotation, `cookbook(...)` rules in
`views.dl`, `cookbook` primitive in the runtime, `cookbook` verb, the
describe cross-ref.

#### `save` — C
What: writes a `@verb(...)` block to `anneal.dl` from `name`, `query`,
`args`, `doc`.
Justification: **project owner flagged this directly.** Agents have Edit/Write.
`anneal.dl` is plain text with a stable, well-documented annotation
grammar. `save` is a second authoring path that:
- duplicates Edit/Write's job
- silently writes to a tracked file
- needs `--force` collision handling that wouldn't exist if agents just
  read the file before editing
- requires teaching a third syntax (CLI args mapping to verb args)
Replacement: cookbook → describe Common joins shows the eval pattern;
agents edit `anneal.dl` to add `@verb(name: "X", query: "...", ...)`.
Where a template is useful, label it `Add to anneal.dl:` rather than
framing it as a separate save workflow.
Lost: one-shot save ergonomics. Honestly accepted.
Magic-word impact: none — "save" was a generic verb, not part of the
convergence vocabulary.

#### `vocab` — F → `schema` or `describe runtime`
What: lists observed statuses, edge kinds, namespaces, frontmatter fields.
Justification: small, observation-driven. The eval form
`? vocab_row(category, value, source).` is the predicate behind it. The
21-row output is genuinely useful when guessing filter literals (the
cold-agent audit specifically cited it). But it's introspection — should
live alongside `schema` rather than as a peer command.
Replacement: fold into `schema --observed` or add a `Vocabulary:` section
to `describe runtime`. Keep the underlying `vocab` predicate; cut the
verb.
Lost: discoverability of literal-string vocabulary on a separate command.
Mitigation: the cold-agent audit's recommendation #6 (predicate-finder
search over schema) absorbs this if `schema --search NEEDLE` lands.
Magic-word impact: minor — "vocab" is generic.

#### `sources` — H (hide, keep callable)
What: lists linked adapters with file patterns and capabilities.
Justification: adapter debugging surface. Not first-screen. One adapter
today (markdown); the multi-adapter future (`anneal-mdx`, `anneal-code`)
will eventually justify visible surface but not now.
Replacement: keep the verb in the prelude (eval form
`? sources(name, recognizes, capabilities, doc).` works) but drop from
default help.
Lost: zero — agents who need adapter info know to look.

#### `eval` — K
What: arbitrary Datalog query (`-e '? query.'`).
Justification: the power surface. The whole Code Mode bet. Non-negotiable
KEEP.

#### `search` — K
What: ranked content search across handles and spans.
Justification: retrieval primitive. Hand-writing a search query against
the multi-field stemmed index would be wasted work. The verb implements
`TopK{...search(...)}` aggregation; rewriting that for every query is
exactly the friction Code Mode is trying to avoid.

#### `read` — K
What: budgeted content spans for one handle.
Justification: retrieval primitive with budget management. Same as
`search`: the `TakeUntil{budget:..., sum: tokens}` shape is correct as a
saved verb.

#### `handle` — K
What: one handle plus its incoming/outgoing edges.
Justification: retrieval primitive. Compact, frequent, replaces the eval
form `? *handle{id: "X", ...}, *edge{from: "X", ...}, *edge{to: "X", ...}.`
which is three queries and a UNION. The H alias stays callable.

#### `work` — F → `status` + eval
What: top-scoring handles worth improving.
Justification: `status` already exposes work candidates in its `work`
section. The dedicated verb is `? top_work(h, energy), *handle{id: h, ...}.`
— two-predicate eval. Killing `work` and pointing agents at `status`
shrinks the noun catalog. Heavy work-shape questions belong in eval.
Replacement: `anneal -e '? top_work(h, energy), *handle{id: h, summary: s}.'`
or simply `anneal status` for the curated view.
Lost: a verb-shaped shortcut. Acceptable — `status` is the right first
move and eval handles the rest.
Magic-word impact: "work" remains in `status` output. Vocabulary intact.

#### `areas` — F → `eval` recipe (and `status --by-area` future)
What: per-area health grade + frontier (composite view).
Justification: useful second-step view. The eval form needs two queries
(`? area_health(area, grade, ...).` and `? area_frontier(area, h, ...).`)
because the verb coalesces both. But "areas" leaked
into-areas-as-a-thing teaching when it should be a derived predicate
view. On .design `Health (0)` is empty by default — the verb's composite
isn't even pulling weight on small corpora.
Replacement: documented eval recipe in `describe area_health` /
`describe area_frontier`. Future: `status --by-area` if a single-screen
view stays valuable.
Lost: the composite output. Mitigation: the two eval queries are short
and the predicates are well-named.

#### `blocked` — F → `handle` or eval
What: explains why one handle looks stalled (potential + entropy + handle).
Justification: takes a HANDLE argument like `handle` does. The
information overlaps strongly with `handle <H>` plus the entropy fields.
Folding the entropy/potential summary into `handle` rendering (a
`Blockers:` section) gives one verb per handle inspection.
Replacement: `anneal handle <H>` shows entropy/potential as part of its
metadata block. Eval form
`? potential(h, e), entropy(h, src), *handle{id: h}, h = "X".` works.
Lost: a dedicated verb name. Acceptable — the question "why is X
blocked?" lives naturally on the handle inspection surface.

#### `diagnostics` — F → `eval`
What: full diagnostic stream (errors + warnings + suggestions + info).
Justification: shipped as the Slice 2 D1 resolution. But the *whole point*
of relation-pattern calls (Slice 1) was making `? diagnostic{...}.` the
first move. `diagnostics` is `? diagnostic{...}.` with one less character
saved and one more noun to memorize. The Code Mode bet says the language
wins; this is the place to put it on the line.
Replacement: `anneal -e '? diagnostic{code: c, severity: s, subject: h,
file: f, line: l, evidence: e}.'` — or with omissions,
`anneal -e '? diagnostic{severity: "error"}.'`.
Lost: a curated table renderer. Eval-text renders the same rows.
Magic-word impact: "diagnostic" stays as a *predicate name* and lives in
schema, describe, vocab. The noun is intact in the language; the command
is the cargo.
Counter-argument considered: D1 just landed. Reverting hours after a slice
ships looks chaotic. **Rejected**: the audit is *for* deciding what was
wrong to add. Slice 2 was driven by typed-flag observation 1; the Slice 1
fix (relation-pattern calls) made the verb redundant.

#### `broken` — H (hide, keep callable as emotional shortcut)
What: error-only diagnostics.
Justification: high-frequency "did I break it?" question. **Conceptually
identical to `? diagnostic{severity: "error"}.`** — but the emotional
weight of the word "broken" is real. Keep callable as the one-word post-edit
check; remove from default help; teach the eval form in `describe diagnostic`.
Replacement: `anneal -e '? diagnostic{severity: "error", subject: h}.'`
Magic-word impact: "broken" remains usable. Hiding from help saves the
catalog space without taking the word away from agents.

#### `trend` — H
What: handles whose status changed since the latest snapshot.
Justification: returns 0 rows when history is empty (which is most of the
time on dev corpora). The session-resume / diff story is unsolved (see
the 2026-05-26 audit D2 decision); pinning a command before the ritual
lands teaches the wrong noun. Keep callable; drop from help.
Replacement: eval form `? trend_row(h, prior, current).` is identical.
Lost: nothing on most corpora today.

#### `init` — K
What: writes `anneal.dl` from inferred structure.
Justification: bootstrap. Genuinely a one-shot setup action with file IO,
not a query. Keep visible — first-time users need it.
Counter-bias check: is init actually used? `init --dry-run` produces a
sensible 40-line scaffold; the migration path from `anneal.toml` legacy
goes through here. Documented in CLAUDE.md. Genuine KEEP.

#### `help` — K
What: meta CLI affordance.
Justification: every Unix CLI has one. KEEP, and it absorbs `prime`
content under `anneal help agent`.

### Hidden compatibility commands (12)

#### `check` — H (CI alias only)
What: gate-oriented diagnostic check (exit 1 on errors).
Justification: CI muscle memory. v0.12 made it a hidden alias for
`diagnostics --gate`. With `diagnostics` cut, `check` becomes the CI gate
verb. Keep callable; hidden from help. Document its single purpose.
Replacement (for agents): `anneal -e '? diagnostic{severity: "error"}.' && exit 1 if rows`
— but `anneal check` is shorter and the right shape for CI scripts.
Lost: nothing.

#### `get` — C
What: human-facing handle view with snippet, indented edges, Try hint.
Justification: covered by `handle` (which already has the right shape).
The v0.11.2 audit agreed `get → handle` merge with a render mode; the
render mode never materialized. Cut `get` outright; `handle` is the
surviving verb. Lost rendering (snippet field) folds into `handle` as
a one-line `Snippet:` row.
Replacement: `anneal handle <H>` + add `Snippet:` line.

#### `find` — C
What: substring match on handle identifiers.
Justification: **broken** — `find --format=text` errors with "unexpected
argument" (see 2026-05-19 broken-surface finding 1). The `find FM` test
on .design returned 0 matches despite `FM`-prefixed handles being a real
pattern in other corpora; the find verb in `views.dl` does string-contains
on `h` (the id), not namespace match. Confused identity-substring with
namespace-prefix.
Replacement: `anneal -e '? *handle{id: h, namespace: "FM"}.'` for the
namespace case (the actually-useful query), `anneal search TEXT
--identity` for substring (Codex's preferred fold). Until `search
--identity` lands, the eval form is the recovery.
Lost: a memorable command name. Acceptable — it was buggy.
Magic-word impact: none — "find" is generic.

#### `impact` — F → `handle --impact` (or eval)
What: reverse dependency traversal (Direct + Indirect partitioning).
Justification: high-value workflow ("what breaks if I change this?").
The 2026-05-19 audit promoted it to runtime verb; the 2026-05-26 audit
flagged "fold into handle" as D3. **Recommend D3 resolution: fold into
`handle <H> --impact` as a flag on the existing verb.** Same handle
argument, related question, additive flag. The Direct/Indirect
partitioning becomes part of the rendering.
Replacement (eval): `anneal -e '? downstream("H", h).'` works (tested:
23 rows on .design) but loses the Direct/Indirect split which depends on
edge-distance.
Lost: nothing if `handle --impact` lands. Without it, `impact` is the
strongest candidate to *keep* among hidden compat — the eval form is
materially worse than the verb.

#### `map` — C
What: graph rendering (text + DOT).
Justification: niche, low cold-agent traffic, can flood context window.
The 2026-05-19 audit recommended hide; 2026-05-26 said deprecate/remove.
Cut entirely. If someone genuinely wants DOT output, eval over `*edge`
plus a shell pipeline works.
Replacement: `anneal -e '? *edge{from: a, to: b, kind: k}.' --format=json |
jq ...` for programmatic graph dumps.
Lost: the text-rendered summary (947 nodes / 89 edges / by-kind / top
namespaces). The summary lines are tiny: agents wanting them can write
the eval queries.

#### `health` — C
What: corpus-wide health summary (counts + pipeline + convergence rate).
Justification: collapsed into `status` per the 2026-05-19 convergence
("One arrival command, not two"). Cut.
Replacement: `anneal status` carries the same content. If counts or
pipeline aren't shown there yet, that's a status improvement, not a
reason to keep `health`.
Lost: zero.

#### `diff` — C
What: counts deltas (handles/obligations/edges) since last snapshot.
Justification: overlaps `trend`. With `trend` hidden, `diff` has even
less to offer. The session-resume ritual is unsolved (D2 from
2026-05-26); keeping two unsolved-ritual commands is worse than zero.
Replacement: future "resume" surface, when designed. Until then, neither
verb belongs in default help, and `diff`'s capability is fully expressible
in eval via `at("snapshot:last") { ... }`.
Lost: a count delta in one screen. Acceptable.

#### `obligations` — F → eval
What: outstanding / discharged / mooted obligations for linear namespaces.
Justification: tiny shipped value (0 rows on most corpora without
`linear()` policy); the 2026-05-26 audit said "collapse into eval recipes".
The predicates `obligation`, `undischarged`, `discharged` live in the
language; the verb is a thin renderer.
Replacement: `anneal -e '? undischarged(h), *handle{id: h, namespace: ns}.'`
plus `describe undischarged` for the teaching card. The teaching gap
(undischarged returns 0 without `linear` policy) is fixable in `describe`
with a `Requires: linear() policy.` line — not by keeping a verb that
also returns 0.
Lost: zero on corpora without obligation policy.
Magic-word impact: "obligation" stays in the language (predicates,
describe, schema). The command goes.

#### `garden` — C
What: maintenance tasks with fix/context/verify advice.
Justification: 2026-05-19 called it the most surprising finding; 2026-05-26
reversed — "Nice metaphor, but `work`/`status` should absorb maintenance
advice. Extra magic word dilutes convergence." **Concur with 2026-05-26.**
The "garden" metaphor is appealing but adds a parallel maintenance
vocabulary (LINK / STALE / TIDY task categories) on top of the diagnostic
vocabulary (E*/W*/S*/I*) already in the language. Two taxonomies for one
concept. Drop garden; advance the diagnostic suggestions (`S*` codes) to
carry fix/context/verify fields if needed.
Replacement: enhance `S*` diagnostics with `fix`/`context`/`verify`
evidence fields; surface via `describe S001_orphaned` etc.
Lost: a metaphor and a set of task category names. The underlying advice
rows fold into diagnostics, which is the canonical vocabulary.
Magic-word impact: "garden" goes. "Convergence" + "diagnostic" already
carry the philosophy; "garden" added a parallel one.

#### `orient` — C
What: file-anchored reading list with Foundation + Overflow.
Justification: 2026-05-19 wanted to promote the file-anchored part;
2026-05-26 said deprecate. `context "<goal>"` covers the same flow with
a different anchor (goal vs file). The Foundation/Overflow distinction
is rendering, not capability.
Replacement: `anneal context "<text from file>"` for goal-driven flow,
or `anneal handle <file> --upstream` if a file-anchored verb is ever
needed (it isn't today — tested: agents reach for `context` in the wild,
per the 2026-05-26 audit traces).
Lost: a second orientation model. Removing it is the point.

#### `query` — C (cluster cut)
What: 5 typed subcommands (handles/edges/diagnostics/obligations/suggestions)
implemented as flag-grammar.
Justification: parallel query language made of flags. Exactly the surface
v0.12's relation-pattern syntax was designed to obsolete. Its own help
text ("too specific for status, too broad for get, intentionally out of
scope for find") describes its slot by negation — the canary the
2026-05-19 audit caught. Cut.
Replacement: `eval`. Each subcommand maps to a short pattern call.
Lost: zero. Eval is shorter and composes.

#### `explain` — C (cluster cut)
What: 5 typed provenance subcommands.
Justification: duplicated by `--explain` on `eval` and runtime verbs.
The flag-based form is what shipped in v0.11; the subcommand form is
dead surface. Cut.
Replacement: `anneal -e 'query' --explain` for derivation, or
`anneal handle <H>` for the structural-fact case. The `--explain` flag
gives the same provenance trees.
Lost: zero.

## Cross-cutting clusters (taken out together)

### Cluster 1: cookbook (whole pattern)

Removes:
- `cookbook` verb declaration in `views.dl`
- `@cookbook(...)` annotation parser + AST node
- `cookbook(name, question, query, doc, when, args, source)` primitive
- The 7 `@cookbook(...)` declarations in `prelude/views.dl`
- "Cookbook" cross-refs in `describe` output
- `anneal help cookbook` topic (if any)

Preserved as:
- `Common joins:` line in `describe NAME` (already implemented for ~8
  predicates per Slice 3 gate; extend to remaining predicates).
- Optional `Add to anneal.dl:` templates showing copyable `@verb(...)`
  declarations where a project extension is genuinely useful.

Rationale: the cookbook is a teaching surface for *predicate composition*.
The teaching card surface (`describe`) is the natural home; making
recipes browsable by question (cookbook's unique value) is a one-shot
search problem solved by `describe` cross-refs + `schema --search`.

### Cluster 2: examples primitive surface

Removes:
- `examples` verb in `views.dl`
- The fact that `examples()` only populates for verbs (cold-agent audit
  finding #4) becomes irrelevant.

Preserved as:
- `Example:` block on `describe NAME` showing all `examples(NAME, e)`
  rows, not just one.
- The `examples` primitive in the runtime (other queries may use it).

### Cluster 3: typed-flag-grammar query surfaces

Removes:
- `query` command + its 5 subcommands (handles, edges, diagnostics,
  obligations, suggestions)
- `explain` command + its 5 subcommands
- The `--filter`/`--code`/`--severity`/etc. flag dialects on those
  subcommands

Preserved as:
- `eval` with relation-pattern calls (Slice 1 shipped)
- `--explain` on eval and runtime verbs (already shipped)

## What stays

Default-help visible (9 + `help`):

```
arrival:    status, context
language:   schema, describe, eval
retrieval:  search, read, handle
setup:      init
meta:       help
```

Hidden but callable (3):

```
broken          (emotional shortcut for diagnostic{severity:"error"})
check           (CI gate alias)
sources         (adapter debugging)
```

Hidden as compat alias for muscle memory (1):

```
prime           (now an alias for `anneal help agent`)
```

**Total callable surface: 13 commands.** (10 visible + 3 hidden + 1 alias.)

The 22 commands removed entirely: cookbook, save, vocab, verbs, examples,
work, areas, blocked, diagnostics, trend, get, find, impact, map, health,
diff, obligations, garden, orient, query, explain.

(`impact` is the one to revisit if `handle --impact` does not land; it has
genuine workflow value the eval form does not match.)

## Migration plan

### v0.13 — visible surface reduction (no behavior breaks)

Goal: shrink default `--help` and hide drift; nothing yet removed.

- Default `--help` lists only KEEP commands.
- Hide from help: `prime`, `vocab`, `verbs`, `examples`, `cookbook`,
  `save`, `work`, `areas`, `blocked`, `diagnostics`, `broken`, `trend`,
  `sources`, plus all 12 compat commands.
- Extend `describe NAME` to show all `examples(NAME, e)` rows and optional
  `Add to anneal.dl:` templates where durable project vocabulary is useful.
- Add `Common joins:` to remaining predicates not yet covered.
- Add `--impact` flag to `handle` (resolves 2026-05-26 D3).
- Add `--upstream` flag to `handle` (replaces `orient --file=X`).
- Add deprecation footers to hidden commands (one line on `--help`
  pointing at the replacement).

Outcome: default `--help` shows 10 commands; everything still callable;
no recipe breaks.

### v0.14 — cookbook cluster + save removal

Goal: remove the v0.12 additions that the audit identified as accretion.

- Remove `cookbook` verb, `@cookbook` annotation, `cookbook` primitive,
  cookbook rules in views.dl.
- Remove `save` command (recipes show `Add to anneal.dl:` copyable
  blocks; `describe` shows the same).
- Remove `examples` verb (the primitive stays for internal use).
- Remove `verbs` verb (`schema --kind=verb` + `describe NAME` cover it).
- Remove `vocab` verb (fold into `schema --observed` or
  `describe runtime`).
- Update SKILL.md, README, CLAUDE.md, master spec §29-§37 to match.

Outcome: cluster cuts land. Surface drops by 5 visible + 0 hidden.

### v0.15 — typed-flag-grammar removal

Goal: cut the `query` and `explain` subcommand grammars.

- Remove `query` (+ 5 subcommands).
- Remove `explain` (+ 5 subcommands).
- Remove `find` (broken; eval over `*handle` covers it; `search
  --identity` covers substring).
- Remove `get` (`handle` + snippet field covers it).
- Remove `health` (`status` covers it).
- Remove `map` (eval + jq covers programmatic dumps).
- Remove `diff` (no replacement; wait for session-resume ritual).
- Remove `obligations`, `garden`, `orient`, `work`, `areas`, `blocked`,
  `diagnostics`, `trend` (eval or other verbs cover each).

Outcome: 13 commands callable. The minimum surviving surface lands.

### v1.0 — locked surface

Lock the 13-command surface as the v1 commitment. Future verbs land in
`views.dl` as `@verb` declarations, not as new top-level commands. Adapter
crates (anneal-mdx, anneal-code) may surface new `sources` entries but
not new commands.

## Counter-arguments considered

### Kept despite weak case: `init`

Counter: `init` is rarely invoked after corpus setup. Agents could
hand-write `anneal.dl` from `describe runtime` examples.
Rejected because: the inferred scaffolding (adapter discovery,
frontmatter fields, status partitioning) needs corpus inspection — not
something agents will reliably reconstruct. The `--dry-run` output is
genuinely useful as a teaching surface. KEEP.

### Cut despite reasonable case: `broken`

Counter: "did I break it?" is the most emotionally important post-edit
workflow. `broken` is one word and lands fast.
Resolved by: keeping `broken` callable as a hidden alias. The word
survives; the help-screen real estate doesn't. This is the right balance.

### Cut despite reasonable case: `impact`

Counter: reverse-dependency inspection is high-value. The eval form
(`downstream(...)` primitive) loses the Direct/Indirect partitioning.
Resolved by: folding into `handle --impact`. If that lands, `impact` is
clean cut. If it doesn't land in v0.13, keep `impact` callable and
revisit.

### Kept despite cluster-cut precedent: `search`, `read`

Counter: both could in principle be eval. `search` is
`? search(query, h, span, score, reason, field, low_confidence).` and
`read` is `? read(h, budget, span_id, text, ...).`.
Rejected because: both are the **idiomatic shape** of the underlying
primitives. The verbs add `TopK{...}` and `TakeUntil{...budget...}`
aggregation; rewriting that aggregation for every search/read query is
exactly the friction Code Mode tries to avoid. These are the right
saved verbs.

### Almost left in: `vocab`

Counter: the cold-agent learning audit (2026-05-20) cited vocab as a
killer feature — it tells you what literal goes in a filter.
Resolved by: the vocabulary content moves into `schema --observed` or a
`describe runtime` section. The capability survives; the command-shaped
discoverability becomes a flag/section. If post-v0.13 evidence shows
agents still hand-reach for `vocab`, restore it as a top-level alias.

### Almost cut: `context`

Counter: it's a composite of `search` + `read` + neighborhood — three
predicates an agent could write themselves.
Rejected because: it's the highest-progress single action available. The
multi-rule composite in `views.dl` (`context_readable` / `context_hit` /
`context_span` / `context_neighbor`) is genuinely not eval-composable as
a one-liner. This is the *exemplar* of a verb that earns its keep.

## What the result looks like

Post-audit `anneal --help` Commands section:

```
Commands:
  status    Convergence landing — work, blocked, broken
  context   Cold-agent orientation: search + read + neighborhood for a goal
  schema    Catalog of relations, predicates, and primitives (use --kind=verb)
  describe  Teaching card for a runtime name
  eval      Datalog query over corpus facts (alias: -e)
  search    Ranked content search
  read      Bounded content read for one handle
  handle    Handle view with edges (use --impact, --upstream for traversal)
  init      Generate anneal.dl from inferred structure
  help      Print this message or the help of the given subcommand(s)
```

That's the language-first surface the v2.0 master spec was always
asking for: arrival (`status`, `context`), language
(`schema`/`describe`/`eval`), retrieval (`search`/`read`/`handle`),
setup (`init`), and meta (`help`). Nine commands plus help. Every
question a verb used to answer now lands on a predicate the agent
discovers via `schema` / `describe`, composes in `eval`, and saves to
`anneal.dl` by Edit/Write.

The dialect is the surface. The corpus runtime ships.
