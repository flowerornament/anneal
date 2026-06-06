---
status: draft
updated: 2026-05-16
author: design-review-subagent
depends-on:
  - 2026-05-13-corpus-runtime.md
description: >
  Critical design review of anneal v2.0 master spec after Phase 11 close
  but before v0.11.0 tag. Surfaces tensions between CR-D decisions,
  premature pins, implicit decisions that need labels, architecturally
  suspect cuts, and pre-1.0 regret candidates. Drives the v0.11.0 polish
  burn-down decided on 2026-05-16. Folded findings move to bd issues or
  spec amendments; this document is retained as the source of truth for
  the audit.
---

# Design review: anneal v2.0 master spec — 2026-05-16

The spec held up surprisingly well across 12 phases. The CR-D label discipline is the reason — almost every implementation surprise got pinned in spec before code locked in. But the sediment is uneven: late decisions (CR-D57..D72) are denser, more defensive, and occasionally contradict earlier framings that were never revised.

## Top 3 most worth changing

**1. The Datalog framing is dishonest and should be revised, not the language.** §17 line 997 still says "Modern Datalog." §4/§5/§8.1 say "stratified Datalog dialect with aggregation — semantics every Datalog engine in the relevant class supports" (line 369). This is no longer true. By CR-D9/CR-D35/CR-D44/CR-D46/CR-D52/CR-D54/CR-D63, the language has:

- sealed primitives that perform I/O (read/read_full/match/search)
- capability gating mid-evaluation (CR-D14/D63)
- self-description primitives (`predicates`, `verbs`, `describe`, `source_of`, `sources`, `schema`) that mirror the runtime
- actor-scoped fact visibility filters that change which rows a rule sees (CR-R10)
- `@verb`/`@doc` directives that aren't part of Datalog at all
- source-extensible facts via Rust traits

No "Datalog engine in the relevant class" supports any of this. The promise on line 369 — that the dialect could be re-targeted at souffle/crepe/cozo — is false today and almost certainly will never be true. The spec should either (a) demote that claim to "the rule layer is Datalog; the runtime is not" or (b) accept this is its own language. Either way, line 997's "Modern Datalog" is the wrong opening to Part IV.

**2. CR-D52 (retrieval provider boundary) is the right idea but cut wrong.** `ContentProvider` resolves `read`/`read_full`. `SearchProvider` emits `SearchHit` candidates. `Ranker` calibrates. But `match` (line 513) is also a relational content primitive with its own scan semantics (CR-R8) and its own provenance — and it's neither a ContentProvider nor a SearchProvider concern. The spec is silent on whose surface owns it. In `retrieval.rs` the implementation appears to be: nobody — `match` is in primitives.rs directly, not behind a trait. Either (a) extend CR-D52 to a third `MatchProvider` (so adapters with native code search like ripgrep can plug in), or (b) explicitly fold `match` into `SearchProvider` with a `mode: Regex|Lexical` discriminator and rename. Today the seam is ad-hoc.

**3. CR-D36 (soft lifecycle primitives) directly contradicts CR-D35 (sealed engine primitives) and the resolution rule is confusing.** Line 551: "Substrate-only engine primitive predicate names in CR-D9 are sealed... must not define, shadow, or union with them." Line 561: "Lifecycle predicates (`terminal/1`, `active/1`...) are runtime-provided defaults, not sealed substrate contracts. If no loaded unqualified rule defines the predicate, the default primitive relation is available. If the prelude, project, include, or inline layer defines the same unqualified predicate, CR-D21 shadowing applies." This is two opposite rules for predicates that look identical at the call site (`active(h)`). An agent reading `predicates()` cannot tell which one shadowing applies to. The "All other CR-D9 primitives are sealed unless a later CR-D* explicitly marks them soft" footnote (line 576) makes this worse — soft/sealed is a per-predicate flag the spec carries inline. Make this a column in §11's predicate table, or split §11 into "§11.1 Sealed primitives" and "§11.2 Soft lifecycle primitives." The current shape is a load-order bug waiting to happen.

## Tensions found

- **CR-D14 vs CR-D63 vs CR-D61 — three overlapping gating models.** §16 lines 892–989 stacks: capability gating (CR-D14), policy action gates (CR-D63), fact visibility capabilities (CR-D61), visibility closure (CR-D62), trail-private hook (CR-D64), and a meta-rule "visibility before derivation" (CR-R10). There are three mostly-orthogonal authorization layers (capability flag, policy action, fact visibility) plus closure rules. The spec never tells you which one fires first when they overlap (e.g., a `team` handle in a `read_full` call by an MCP actor without `read_full` capability — capability error or policy denial?). CR-D63 line 927 partially answers ("Capability-required errors remain distinct from policy denials: missing `read_full`... reports the missing capability before project policy is considered") but fact visibility's ordering relative to both is unspecified. Likely outcome: integration tests pin behavior by accident.

- **CR-D54 (trail four-way split) vs CR-D67 (trail projection safety).** The four-way split is real in `trail.rs` (lines 297–447 confirm: Recorder/Redactor/Summarizer/Store all separate traits). But CR-D67 ("Trail projection safety") is a runtime invariant about loading persisted entries — that's neither a Recorder nor a Store concern as cleanly cut. It's bolted onto Store. Probably fine, but the four-way split sold itself as "different decisions"; CR-D67 shows there are at least five.

- **CR-D4 vs CR-D57 vs CR-D68.** §5 introduced `Source` with rich `SourceContext`. CR-D57 then carved out `SourceDriver`. CR-D68 then carved out the *refresh transaction* as an `anneal-core` concern. Three decisions, three months apart in spec time, build up a stack that reads more like discovered necessity than designed shape. The current shape is correct (verified in `driver.rs`), but the spec should consolidate §5 into a single trio diagram (Source / SourceDriver / refresh) rather than three sequential definitions where each amends the prior.

- **CR-D45 vs CR-R4 vs CR-D60.** CR-R4 (Steele's criterion) says project verbs are syntactically indistinguishable from prelude. CR-D60 says output_schema is a "canonical JSON string" until anneal-lang grows object literal syntax. CR-D45 says the executable context contract is the lowered Datalog program. So project verbs are *not actually* syntactically indistinguishable today — they get JSON-string schemas; prelude verbs get the same; but neither matches the spec's object-literal examples in §33.1. CR-R4 is true on a technicality. Either CR-R4 needs a "today's encoding is JSON-string; Phase 7+ extends" caveat, or §33.1's examples need to switch to JSON-string form to match what ships.

- **CR-D9 line 510 search signature has 7 columns; CR-D31 frames diagnostic evidence as `*meta` on the file handle.** These are inconsistent encoding philosophies — search rows go wide-relational, diagnostic evidence goes narrow-keyed-into-meta. Not wrong but worth a §10 paragraph saying "we use both patterns and here's when."

## Decisions that should be CR-OQ

- **CR-D42 (default lexical Ranker weights).** Numeric field weights (`identifier`: 1.0, `title`: 0.95, `body`: 0.82, `frontmatter:*`: 0.88, other: 0.75) are pinned in the spec. These are guesses with no empirical backing — anneal-ml1 in bd already says "default lexical Ranker is too literal." Lock the *shape* (per-field weighting + clamping + tie-break order), open-question the *values*.

- **CR-D71 (60% per-hit budget allocation).** The "v2.0 derives `context_read_budget` as 60% of the requested `--budget`" (line 1967) is a magic number. Either justify it (fixture-pinned) or open-question it as "context budget allocation TBD; current heuristic is 60% per-hit, no neighborhood reserve."

- **CR-D43 (low-confidence threshold default 0.5).** Same pattern. Defaults are policy, not contracts.

- **CR-D61 (three-level visibility ordering public/team/private).** Locked too early. Real-world authorization often wants attribute-based, not ordinal. Spec already hedges line 950–951 ("Hosts may define narrower labels"). The fact that you needed that hedge is the signal — make it OQ.

- **CR-D45 (executable context lowering).** Line 2008 literally says "this pins the agent-visible behavior with today's parser while preserving CR-R4's stronger typed verb contract for Phase 7." That's the definition of a transitional decision. It should be CR-OQ-pinned-by-fixture, not CR-D-locked.

## Implicit decisions that should be CR-D

- **Engine primitive evaluation order.** `eval.rs` runs primitives in a particular order relative to stratified rule evaluation. The spec says "IR fixpoint" (line 280) and "stratified" (CR-D18) but doesn't say *when* a primitive call is evaluated relative to the stratum it sits in. anneal-251 in bd ("Runtime aggregates should re-evaluate same-stratum derived dependencies") is the bug-shaped evidence. There's an unstated CR-D about primitive-call-as-rule-body-atom timing.

- **Whether `@verb` is a directive or a fact.** §17 line 1010 lists `@verb` under `directive`. But CR-D60 says verbs flow through the parser as JSON-string-bearing structured annotations, and `verbs(name, query, doc, output_schema)` (line 518) treats them as facts the introspection primitives read. They're both. The spec needs a CR-D saying "directives are also reified as introspection facts."

- **Source-extensible engine primitives.** Adapter authors today *cannot* add a new engine primitive — only stored facts. The sealed namespace (CR-D35) closes the door. This is an unwritten CR-D: "engine primitives are substrate-only; adapters extend through facts, providers, and policy, never through new primitive predicates." Should be stated. (See pre-1.0 regret below.)

- **`schema()` arity confusion.** bd issue anneal-qke ("introspection primitive arity discoverability is poor") and anneal-3ye ("predicates() primitive returns docstring in arity field") indicate the introspection primitives drifted from spec or were never specified tightly. CR-D44 is close but doesn't cover the `signature`/`arity` encoding for `schema()`. Add an explicit format.

- **`config_facts` ordinal compatibility.** CR-D40 says ordered config carries an `ordinal`. anneal-zk5 says ConfigFacts serde/accessors must honor it. There's an implicit "serde round-trip preserves ordinals" CR-D the runtime now relies on.

## Architecturally suspect cuts

- **`anneal-legacy` as "transition-only" (CR-D32).** Transition crates have a tendency to outlive their plans. There is no spec-pinned deletion date or migration completion gate. Add to CR-A: "anneal-legacy is removed before v1.0" or "by milestone X" — otherwise it becomes architecture.

- **`anneal-lang` boundary (CR-D51, CR-R9).** The crate exists and is private. Good. But CR-R9 condition #2 — "at least one non-CLI consumer needs parser-only access" — is satisfied today by MCP verb introspection (consumed via core) and arguably by the introspection primitives themselves. The gate is murky. Define the consumer more sharply: "an external crate outside this workspace links anneal-lang without anneal-core."

- **The CR-D35 sealed namespace.** Combined with the inability to add new engine primitives from adapters, this means anneal-host (Phoenix routes, Ash resources, Oban jobs) cannot expose host-native graph traversal as a primitive — only as facts. For Oban-job graphs with millions of edges, that may be a real performance ceiling. This is the most likely "we got this wrong" candidate (see below).

- **`*meta` as the catch-all bag (CR-D31).** Diagnostic evidence, parent_dir, resolved_file all live in adapter-qualified meta. This will sprawl. A `*meta` row with `key: "md.implausible_ref"` carrying JSON-encoded value is exactly the "smuggling structure through strings" pattern the spec criticizes elsewhere (CR-D40, CR-D44). Worth a `*evidence` relation with typed slots.

## The "is it Datalog" question — honest read

It is Datalog at the *rule layer* (Horn clauses, stratified negation, aggregation, fixpoint) and **not Datalog anywhere else**. The substrate is a runtime that:

- exposes I/O-performing relations (`read`, `search`, `match`, `read_full`) gated by capabilities,
- mediates fact visibility per-actor before rule evaluation (CR-R10),
- reifies itself through introspection primitives queryable from inside the language,
- captures execution into `*trail` that is itself a queryable relation,
- accepts source-extensible facts via a Rust trait boundary,
- defines verbs as first-class objects with output schemas and capability requirements.

This is a *programmable corpus runtime* with a Datalog rule sublanguage. That is what the spec title says. The body has not caught up. Concrete fix: rewrite §17 line 997 from "Modern Datalog" to something like "Stratified Datalog (rule layer) over a runtime substrate of I/O-bearing primitives, fact visibility, and reified self-description. The rule syntax is portable; runtime semantics are not." Strike line 369's portability claim, or qualify it.

If the language eventually deserves a name, CR-R9's "second consumer + semantics pinned" gate is the right gate. The honest spec-side admission today is: **the language is a substrate-coupled dialect; the substrate is the spec**.

## The pre-1.0 regret candidate

**CR-D35 (sealed engine primitives) is the single highest-blast-radius decision.** It is the easiest thing to relax (open a primitive registration trait) and the hardest thing to reverse course on if relaxed too early (every adapter primitive becomes runtime ABI). Right now it forecloses:

- adapter-native graph traversal (Oban DAGs, Phoenix routes, Ash relationships) — must round-trip through facts even when adapters have efficient native traversal
- content-source-native search ranking signals as primitives (vector search providers would have to either ride `SearchProvider` and lose richness, or fake themselves as facts)
- introspection extension (host-app introspectors that want to expose `mounted_routes(...)` or `job_state(...)` as queryable primitives)

The CR-D36 escape hatch (soft lifecycle primitives) shows the door already cracks under pressure — `terminal/active/settled/obligation` get to be shadowed because corpus-specific lifecycle is too universal a need to seal. The next pressure (host adapters in 2.1+) will crack it again. Better to design the primitive-extension surface now and gate it on "no I/O primitives from adapters in v2.0; structural-only" than to discover three adapters from now that CR-D35 is wrong.

Second runner-up: **CR-D42's locked field weights** — those are statistical guesses lacking a fixture corpus, and anneal-ml1 already names the smell.
