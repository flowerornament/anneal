---
status: draft
date: 2026-07-27
authors: [claude, codex, morgan]
purpose: >
  anneal as a lens onto a corpus. Establishes the durable abstraction — a lens is
  a TYPED OBSERVATION PLAN with a declared oracle, a valid parameter grammar, a
  result manifest, and adjacent affordances — and rejects the tempting
  alternative that every verb is a preset over a free product of coordinates.
  Separates relation from operator, and separates work budget from result limit.
  Each decision records its options, its evidence, and its verdict. Companion to
  2026-07-26-zoom-levels.md, which governs resolution.
relates:
  - 2026-07-26-zoom-levels.md
  - 2026-05-13-corpus-runtime.md
---

# Observation plans: anneal as a lens — 2026-07-27

## 1. The problem, measured — and the part not yet measured

anneal's own corpus:

```
Cites        1178   (94%)
DependsOn      53
Supersedes     19
```

There is no single "the graph." There are three **stored edge strata** with
different sizes, shapes, and uses, plus **derived relations** (`topic_sibling`,
`impact`, `lineage`) that are not strata at all.

What this measurement establishes, exactly:

- **Corpus-wide edge mass is 94% `Cites`**, so any corpus-level summary that
  reports a single edge count leaves the two sparse strata **numerically
  submerged**.
- Prior navigation work established that raw transitive `Cites` walking is
  hub-routed on this corpus.

What it does **not** establish: that a given handle's *local* neighbourhood is
dominated by nondiscriminative citations. Global mass does not license a local
claim (§6). Until that is measured (§7 Q2), the surface problem is that the
strata are **unnamed and numerically submerged** — not that any particular
vantage is drowned.

## 2. The correction that shapes everything

A first draft claimed the existing flags were presets over five free coordinates:

```
--impact   ≟  layer=depends, hops=∞, reversed
--lineage  ≟  layer=supersedes, hops=∞
```

**Both are false**, and the code says so:

- `DEFAULT_IMPACT_TRAVERSE = ["DependsOn", "Supersedes", "Verifies"]`, configurable
  via `impact.traverse`. It is a reverse transitive closure over a *configured
  mixed kind set*; a single path may mix kinds. No single-stratum projection
  produces it.
- `lineage` carries a `normalized_root`: it normalizes short and version handles
  to file handles *before* walking, because raw `Supersedes` direction differs by
  handle kind, then walks both directions and derives currency dispositions over
  a branching structure with possibly multiple heads.

These are **semantic operators with invariants**, not knob settings.

> **The durable abstraction:** a lens is a **typed observation plan** with a
> declared oracle, a valid parameter grammar, a result manifest, and adjacent
> affordances. Sum type, not product.

## 3. Plan catalog — an initial family, not an inventory

These are examples of the shape, not a complete census of existing verbs
(`status`, `search`, `context`, and base `handle` are also plans and are not
listed here).

```
Neighborhood { vantage, stored_edge_filter, orientation,
               max_hops, work_budget, result_limit }
Impact       { vantage, effective_impact_policy, reverse_closure,
               work_budget, result_limit }
Lineage      { normalized_file_vantage, file_supersession, both_directions,
               branching_heads, currency_disposition }
Representation { object, declared | map | focus | full }     → zoom-levels.md
SetView      { population, selection, dimensions, aggregation } → zoom-levels.md
```

Field notes that carry real invariants:

- **`effective_impact_policy`**, not "dependency policy" — the default set
  includes `Supersedes` and `Verifies`, and naming it "dependency" would
  re-encode the very simplification §2 kills.
- **`file_supersession`** — the relation *after* normalization, retaining
  branching and multiple-head semantics. Both-direction closure alone misses the
  DAG/head invariant.
- **`stored_edge_filter`** — the generic plan projects *stored strata*. If derived
  relations ever become eligible it needs a typed `traversable_relation` whose
  domain, codomain, and orientation contract are declared.
- **`work_budget` and `result_limit` are different things** (§7 Q4).

**`AxisView` is deliberately absent from the executable catalog.** CR-D104 axes do
not share a row schema — an axis is a question/oracle/disposition family, not a
projection parameter. It may exist as an introspection concept; it does not get a
typed output contract here until one is designed.

## 4. Decisions

### D1 — Does a surface ever get to give navigational advice?

**Options.** (a) Advise. (b) Report only. (c) Three altitudes.

**Evidence.** Advice presupposes a goal. A bare `handle` call declares none, so
"instead" compares operators for an unstated task, may promote an operator with
zero rows here, and hides which criterion fired. That is the flat-confidence
failure CR-D103 exists to prevent, one level up.

**Verdict — (c), strictly separated:**

```
observation   Cites: 31 outgoing targets
disposition   <criterion-backed limitation, with evidence>          [v1: absent]
affordance    available: topic_sibling(…), 6 candidates here
```

**Teach availability, not preference.** Advice becomes earnable only when
(i) intent is declared, (ii) alternatives were evaluated **in the same
observation/snapshot** — otherwise the comparison is stale, the inconsistency the
companion design already ruled out — (iii) utility was measured against that
intent, and (iv) provenance is surfaced.

**The local oracle is unresolved and must not be faked.** An earlier example read
*"top target receives 42%"* — but for one handle's outgoing citations each
distinct target typically receives one edge, so that is not local degree
concentration; it is not a well-defined statistic at this vantage. Candidate real
oracles, all different from global top-k share:

- fraction of this handle's targets that are **globally nondiscriminative hubs** —
  substrate support already exists via `topic_nondiscriminative_target`, so
  `local_nondiscriminative_target_ratio(h)` is buildable
- popularity/IDF distribution of this handle's targets
- two-hop candidate fanout through shared targets

Until one is defined and calibrated, the disposition line is **absent**, not
approximated.

### D2 — Can a layer be labelled "hub-shaped"?

**Options.** (a) One taxonomy. (b) Metric profile. (c) Profile now, dispositions
when calibrated.

**Evidence.** The candidate labels are **not one axis** — a layer can be
simultaneously acyclic, concentrated, and fragmented. Forcing one label repeats
the error the companion design made once with its content "ladder."

| property | measure |
|---|---|
| acyclicity | SCC / cycle test — exact |
| concentration | top-k share, HHI, Gini — in and out separately |
| chainness | fraction with in ≤ 1 and out ≤ 1 **plus largest-path share or component-conditioned path coverage** |
| fragmentation | weak-component count, largest-component share, isolate share |

Chainness needs the path term: `in ≤ 1 ∧ out ≤ 1` alone would call a forest of
isolated edges chain-like, and fragmentation being a separate metric does not
prevent that misleading disposition.

**Verdict — (c).** Emit a profile; derive zero or more dispositions from stated
criteria. Every profile declares **scope**: node universe (all handles vs
layer-participating), handle domain, direction, and global-vs-local-at-vantage.

Degenerate states are first-class values: `empty`, `insufficient_sample`,
`heterogeneous_domain` (mixed handle domains needing normalization — *not*
"incomparable", which concludes more than was tested; heterogeneous domains can
be legitimate typed graphs). *"No clear shape"* means **valid metrics, no
disposition crossed.**

**When a topology plan is eventually built it begins with exact metrics, not
labels.** `top-5 inbound share 71%` beats `hub-shaped`, and thresholds require
calibration across anneal, murail, and synthetic counter-shapes first.

**But v1 exposes no profile surface at all** (§5). Nothing consumes a profile
while dispositions are uncalibrated, which makes a flag for it shelf-ware by our
own rule, and the "emit when a limitation is derived" path cannot fire when no
criteria exist. The metrics-first discipline governs the plan whenever it is
built; it does not license shipping a surface now.

### D3 — Decompose the semantic verbs, or keep them whole?

**Evidence — Falkoff & Iverson, *The Design of APL*.** APL's stated principle is
economy through small orthogonal primitives, yet its designers gave *every*
nontrivial two-argument logical function a dedicated symbol, because common
operations should be directly expressible and composed forms are opaque. The
stated lesson:

> the right answer can differ **per feature**, based on how frequently a case is
> used and how legible its composed form would be.

**Verdict.** `--impact` and `--lineage` stay whole — frequently reached for, and
per §2 not expressible as projections at all. A generic neighborhood plan may sit
beside them but **must not claim to subsume them**. Test for any future preset:
**frequency of use × opacity of the composed form**.

### D4 — Should a lens configuration persist across calls?

**Evidence — Kay, *Early History of Smalltalk*.** *Any state distinction the user
must track is a mode they can get trapped behind.* A carried lens is exactly such
a distinction, and agents — which lose context between sessions — are the worst
possible mode-holders.

**Verdict — modeless, load-bearing.** No session, no carried vantage. Every answer
self-describes; adjacency is taught in the answer, not remembered by the tool.

### D5 — What may an answer print, given a context budget?

**Evidence — agent-computer interface research.** Feedback must be
*"precision-optimized, not coverage-optimized"*, because *"what a human skips, an
LM must process."* But iterative interfaces *"cause agents to exhaust context
budgets polling through results one by one."*

**Verdict — compact default, drill-down for the rest, judged by expected workflow
cost.** The naive bar — *"costs fewer tokens than the queries it replaces"* — is
wrong arithmetic: it ignores how often the information is needed. A 10-token
always-on line is worse than a 100-token drill-down if one call in twenty needs
it. The bar is:

```
always_on_tokens_per_call
  vs
need_probability × drilldown_tokens  −  polling/failure cost avoided
```

measured over **representative agent trajectories**, not per-call size. **JSON is
not free** because it is structured — additive fields face the same test.

**Corollary — the comma rule.** Separators in rendered lines are commas, and if a
line needs a stronger separator to stay readable it is doing too much and must be
split or shortened. A chunking separator such as `·` makes an overpacked line
*look* organised while costing the same tokens; the comma exposes the density
instead of hiding it. This is the per-line counterpart of the per-call budget
test above, and it caught a genuinely overpacked `handle` line the first time it
was applied.

Concretely: `status` gains one stratum-count line with exact counts and no
adjective; `handle` gains one compact local adjacency line naming nonzero
relations by semantic role. CR-D106 is satisfied by **one line; a standing manual
does not satisfy it.**

### D6 — How should direction be named?

**Evidence.** Storage orientation is meaningless to a reader because meaning is
relation-relative, and `lineage`'s normalization proves raw direction is not even
stable across handle kinds within one stratum.

**Verdict, scoped.** **Curated navigation renderers** expose semantic roles only:
`cites` / `cited-by`, `dependencies` / `dependents`, `older` / `newer`. The
language and introspection surfaces keep their exact predicate names
(`incoming_edge`, `outgoing_edge`) — `-e` stays raw, and renaming primitives
would break the substrate contract.

### D7 — Is `layer` the missing primitive?

**Evidence.** `topic_sibling` is a **derived relation** constructed *from* the
`Cites` stratum by a **coupling operator with hub exclusion**. Same edges,
different relation, different operator. `impact` is best described as a named
closure operator whose output is itself observable as a derived relation.

**Verdict — the missing thing is a DISTINCTION, not a primitive:**

```
relations  denote tuples      — stored Cites; derived topic_sibling, impact
operators  act on relations   — projection, traversal/closure, coupling,
                                ranking, aggregation
plans      pair a compatible relation with an operator, plus normalization
           and a manifest
```

`layer` names **one class of base relation**. Writing "relation/operator" as a
single hyphenated primitive would collapse two dimensions into one noun and
become the next free knob; the spec keeps them separate deliberately.

### D8 — How far does "lens" hold?

**Evidence — Ashby.** The observed system depends on which variables the observer
selects; no single variable set is the uniquely true description. A strong warrant
for named, provenance-bearing views over any claim to show "the graph."

**Verdict.** *Lens* is **product and explanatory language** meaning "a typed
observation plan." It breaks as an implementation algebra: optics implies
continuous focus and freely composable filters; typed relations have discrete
domains, required normalization, and invalid compositions. The substrate says
*view* or *plan*, and internal coordinates are **not** published as observable
semantics before the valid grammar is known.

## 5. What changes at the surface

The whole interface is **three things**: one output contract, one locating block
of two lines, and two flags.

```
contract  the three-line manifest (scope, shown, how) appears when a plan
          returns a lossy representation, or a strict-or-unknown subset of
          its declared eligible population. See zoom-levels §2 — that
          definition is normative and this is a reference, not a restatement.

status    Graph line, exhaustive over every nonzero stored stratum:
          1178 Cites, 53 DependsOn, 19 Supersedes   (no adjective)
handle    two lines: what this is, and what is attached — exhaustive over
          every declared semantic role, zeros omitted
read      --budget selects instead of truncating; --focus Q added
search    the same three-line manifest
-e        unchanged — raw predicates, raw direction names
```

**Separators are commas.** If a line needs something stronger to stay readable,
it is doing too much and must be split — the rule caught an overpacked `handle`
line on its first use.

**Cut from earlier drafts, and worth naming as cuts:** `--map` (that is simply
the view `--budget` picks), `--profile` in v1 (nothing consumes it while
dispositions are uncalibrated, which makes it shelf-ware by our own rule), and
any surfaced `layer` — the adjacency line already reports the strata and the
named operators already traverse them, so a generic stratum walk remains
unproven demand (§7 Q3).

## 6. Rejected

- **A free product of coordinates.** §2. Invalid combinations exist, and a grid
  implies they are meaningful.
- **`narrow` / `widen` / `deeper` as move names.** They smuggle an order:
  switching `Cites` → `DependsOn` is not narrowing, and a union is wider in edge
  count but not in meaning. Use relation-relative verbs: *switch relation*,
  *follow dependents*, *follow newer*, *expand hops*, *change representation*.
- **"94% citation, hub-shaped."** Pairing an exact statistic with an unearned
  category transfers the number's authority to the adjective.
- **Global mass licensing local claims.** Applies to this document's own §1.
- **Dismissing `Cites`.** Utility is operation-relative: citation structure is
  independent editorial signal, already load-bearing for authority ranking,
  currency coupling, and `topic_sibling`.
- **Unbounded generic walks.** See Q4 — a render limit does not bound computation.
- **Inheriting impact's mixed-kind privilege.** A union of strata creates paths no
  single semantic relation licenses. `impact` does this deliberately under a named
  configured policy; a generic union must not.

## 7. Open questions

1. What concentration threshold, if any, survives calibration across anneal,
   murail, and synthetic counter-shapes — or is it corpus-relative, meaning
   metrics-only is the permanent answer?
2. **What fraction of handles have hub-dominated *local* neighbourhoods?** Until
   measured with a defined local oracle (D1), the local failure mode is asserted.
3. Does the generic neighborhood plan earn a surface at all, or do the named plans
   plus direct adjacency cover the real demand?
4. **Which execution and rendering caps make a generic dense-stratum walk both
   bounded and useful?** Safety comes from *maximum* bounds, and four controls are
   distinct: `max_hops`, `work_budget` (visited nodes/edges or time), `result_limit`,
   and a deterministic truncation/selection policy with a manifest. A walk may
   visit 100k nodes and render 20 — `result_limit` alone bounds nothing.
5. ~~Can `axis` compose with traversal?~~ **Resolved: not generically.**
   Generic axis composition would reopen the free product, since axes have
   heterogeneous predicates and result shapes. A *named, designed* plan may carry
   an axis-grounded annotation only by declaring: which axis-owned predicates
   supply it, its applicability domain, output fields including unknown/null
   behaviour, cardinality and manifest, and same-snapshot evaluation. That is a
   **new sum variant, not decoration** — and actual compositions stay
   consumer-pulled.
