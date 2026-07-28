---
status: draft
date: 2026-07-26
authors: [claude, codex, morgan]
purpose: >
  Resolution for a corpus. anneal can honestly zoom STRUCTURE without a model;
  it can honestly excerpt CONTENT only relative to a declared lens. Establishes
  ONE manifest contract — scope, shown, how — as the honesty contract for reduced
  views, since provenance proves quotation fidelity and not coverage fidelity.
  Object reduction and set aggregation share the outer shape and differ in the
  claims they carry inside it: same labels never imply same measures. Explores
  interfaces, reduces them to one contract and two flags, and records
  prerequisites as a dependency graph rather than a ladder.
relates:
  - 2026-05-13-corpus-runtime.md
  - 2026-07-20-help-language-restoration.md
---

# Resolution: zooming a corpus honestly — 2026-07-26

## 1. Two failures

**Scale is invisible.** An agent handed `2026-05-13-corpus-runtime.md` cannot
tell whether it is the corpus spine or a stray note. The facts exist — 1 of 146
files, 1 of 60 in its area, 108 sections, 23,394 tokens — but no surface reports
them.

**Reduction lies.** `read --budget 300` on that document returns *the first 300
tokens*, cut mid-word. It admits this in a trailing hint, which is good. But a
fragment of the opening is not a low-resolution view of the document; it is a
different document that happens to start the same way.

## 2. The governing distinction

The tempting framing — extractive is honest, abstractive is not — is **wrong**,
and getting it wrong would misdirect every later decision.

> **Exact-span provenance proves quotation fidelity, not coverage fidelity.**

A specificity-ranked sentence can be perfectly traceable and still caricature the
document. Conversely, paraphrase is not intrinsically dishonest: an
author-declared summary, or a clearly labelled model interpretation with linked
evidence, carries calibrated authority.

anneal's actual commitment is narrower and stronger:

> The current runtime has **no oracle that earns semantic paraphrase**, so it
> emits declared abstractions or source spans, and **names the selection lens**.

That is a statement about this runtime's oracles, not a claim that paraphrase
violates CR-D103. If anneal ever acquires a model, the surviving constraint is
labelling and evidence-linking — not prohibition.

### The honesty contract: one manifest, three lines

Provenance alone is insufficient, so any answer that **reduced or selected**
states what it covered, what it showed, and by what rule. One shape everywhere;
fields that do not apply are absent rather than empty.

```
scope   108 sections, 23,394 tokens
shown   19 headings, 23 subtrees and 22,614 tokens omitted
how     uniform heading depth fitted to 400 tokens, --span-id to expand
```

**When the manifest appears, precisely.** Not "whenever a plan projects fields" —
every surface is a typed projection, so that trigger would demand a manifest
everywhere and explain nothing. The rendered manifest appears when a plan returns
**a lossy representation, or a strict-or-unknown subset of its declared eligible
result population**. Projecting a chosen set of fields is not a reduction.

That boundary imposes an obligation on the surfaces that carry no manifest: they
must be **exhaustive over their declared population**, or they are undisclosed
selections masquerading as complete answers. So `status`'s Graph line names
*every nonzero stored stratum*, and `handle`'s adjacency names *every declared
semantic role*, zeros omitted. If either ever showed a subset, it would owe a
manifest like anything else.

`how` also carries authorship where authorship is the claim — *declared in
frontmatter, coverage author-asserted* versus *prefix of body text, not a
summary*.

**Separators are commas, and that is a constraint, not a style.** If a line
needs a stronger separator than a comma to stay readable, it is doing too much
and must be split or shortened. A visually chunking separator hides density; a
comma exposes it. This rule caught an overpacked line on its first application
(§4 A).

**`scope` and `shown` are different claims.** A map makes all 108 sections
*addressable*; it does not show them and does not cover their content. Calling
that `coverage=108/108` would smuggle the quotation-vs-coverage conflation back
in through the instrument built to prevent it. The correct claim is
**structurally complete**, never *fully covered*.

*(An earlier draft of this document made exactly that error. The manifest caught
its own author, which is the argument for having one.)*

**Origin determines which contract a value may claim.** The same text under the
wrong view instruments the lie rather than removing it:

```
scope   the document, 23,394 tokens
shown   the author's purpose statement, one sentence
how     declared in frontmatter, coverage author-asserted
```
```
scope   the document, 23,394 tokens
shown   first 300 tokens, 23,094 omitted
how     prefix of body text, not a summary, --budget 23394 for all
```

Note what the first concedes: a declared summary's semantic coverage is
**author-asserted, not mechanically proven**. Calibrated authority, not a weaker
claim dressed as a stronger one.

This generalises CR-D106 from *"name the adjacent set"* to **"state what you did
to the thing you are showing, and on whose authority."**

## 3. Structure vs content

**anneal can honestly zoom structure without a model. It can honestly excerpt
content only relative to a declared lens.**

```
STRUCTURAL — containment, model-free, genuinely ordered
  corpus → area → file → section → text

CONTENT — lenses, NOT a ladder; incomparable reductions of one object
  declared  author-written purpose/note, explicitly labelled   [prerequisite, §6]
  map       adaptive heading-tree cut — the query-free construction
  focus     query-conditioned complete spans
  full      every byte
```

**Radius** (`neighborhood`, `upstream`, `--impact`, `--lineage`) already exists
and stays orthogonal: *how far from here*, not *how close in*.

### The map construction

The strongest model-free reduction. Treat the section tree as level-of-detail:
retain every top-level branch; expand uniformly by depth while the *complete*
next depth fits; collapsed nodes state descendant-section and omitted-token
counts.

> **Policy, in one sentence:** every top-level branch is shown; deeper levels
> appear only when the complete next depth fits, and collapsed branches name
> what they omit.

This earns *"the whole document, smaller"* in the **structural** sense —
every omitted subtree keeps an addressable representative — and makes no claim
about body content. An outline that silently stops is not equivalent.

If title plus top-level headings will not fit, emit the manifest and branch count
and require a larger minimum budget — never silently omit branches while claiming
structural completeness.

### What we deliberately reject

- **Global term specificity as a representativeness oracle — never.** It rewards
  rare jargon, not thesis-bearing content. Under a declared focus,
  query-conditioned paragraph relevance may *select* excerpts, but it earns only
  **relevance-to-focus, never document coverage**. Focus is not blanket
  permission, and whether the ranker's existing specificity component transfers
  to paragraph altitude is an open empirical question (§8), not a granted one.
- **Inbound degree as a coverage rule.** Conflates importance with
  representativeness.
- **Lead sentences as automatic resolution.** In a spec corpus, sections open
  with tables, code fences, and list stubs. A possible `openings` sampling lens
  is a **deferred experiment**, outside this proposal's recommended interfaces —
  and if ever built, never called a summary.

## 4. Interfaces

| | proposal | verdict |
|---|---|---|
| **A** | **Position + role annotations** on answers agents already get | **adopt** |
| **B** | Unified `--zoom N` dial | reject — conflates incomparable axes; integers carry no meaning |
| **C** | A `zoom` verb | reject — CR-D105; agents reach for `status`/`handle`/`read`, not a navigation noun |
| **D** | **Containment relations + typed representation capabilities** | **adopt as substrate** |
| **E** | Named lenses on `read` (`--map`, `--full`) | **reduced to one flag** — see the v1 boundary below; only `--focus Q` is a new capability |
| **F** | **`--budget` selects within the auto chain** instead of truncating | **adopt, corrected** |
| **G** | **Faceting for result sets** | **adopt** — same three-line manifest |

**The whole interface is three things:** answers carry the manifest, `handle`
gains one locating block of two lines, and `read` gains `--focus` while `--budget` changes
meaning. Everything else in this document is the reasoning that justifies those
three and the longer record of what not to do — which is worth keeping, but is
not interface.

### A. Position and role are two clauses

The motivating failure mixes magnitude with importance. *1 of 146* and *108
sections* answer **scale**; *is this the spine* is **role**, which needs signals
the ranker already instruments.

```
1 of 60 in "(root)", 108 sections, 23,394 tokens, authoritative
cited-by 16, cites 3, dependencies 2
```

**Two lines, not one — and the comma rule proved it.** A single line held eight
comma-separated items and collapsed into soup; it had read acceptably only under
a chunking separator that was hiding the density. Split, each line is one
coherent thought: what this is, and what is attached. Zero-valued relations are
omitted entirely, so absence carries the zero.

### D. Containment is ordered; representation is typed

D exposes **structural containment** (genuinely ordered) plus **typed
representation capabilities and their manifests**. It must *not* expose
`resolution(handle, level)` across `declared/map/focus/full` — that would encode
the total content order §3 denies. Only structural containment and the explicit
auto pair `full > map` are ordered.

### F. Budget selects within a declared chain

The obvious phrasing — coarsest that fits — is **backwards**: the declared
summary is nearly always coarsest and would win every budget, making `--budget`
useless. Correct rule: the **finest view that fits *within the explicit auto
chain* `full > map`**.

The qualifier is load-bearing. "Finest" is meaningful only along a chain declared
ordered; it must never become a global objective, or a future planner will
compare `map` against `focus` as though information value were totally ordered.

```
read <handle> --budget 300
  today:     first 300 tokens, cut mid-word
  proposed:  map, 19 headings shown, 23 subtrees collapsed,
             full structural coverage, manifest attached
```

**Why `--map` is not a flag in v1, deliberately.** Map is the planner-selected
fallback *within* the declared `full > map` chain, not an independently
selectable preference. If `full` fits the budget the caller declared, the plan
has no earned reason to discard it — asking for less than you can afford is a
preference nobody has demonstrated. Explicit map demand can become a named
selector later, once someone wants it; until then, adding the flag would ship a
choice with no consumer. This is a stated v1 boundary, not an accident of the
chain.

**Focus requires a declared query.** Without one, anneal refuses to guess which
body content represents the whole, rather than silently picking.

### G. Sets face; they do not zoom — same contract, different claims inside it

A result set has no single content level; it has a **distribution** over
containment and kind, so bytes, authorship and origin do not apply to it. That
does **not** justify a second schema: the outer coordinates hold, and the claims
carried inside them differ. **Same labels never imply same measures.**

```
scope   evaluated 1,204 candidates, matched 843
shown   20 ranked rows, 823 not rendered
how     default confidence, top 20, area/kind facets over matched by count and max score
```

**`facets`, not `grouped`.** Grouping would imply rows were rolled up, which
§ below forbids; the ranked rows are untouched and the facets are computed
*beside* them. And the facet population is named explicitly — **over matched**,
not over the 20 rendered — because which set an aggregate summarises is exactly
the claim a reader would otherwise assume wrongly.

*(This example was four physical lines until `how` was shortened. The three-line
constraint caught its own illustration — the second time a rule in this document
has failed against itself first.)*

All three cardinalities survive: `scope` carries **evaluated and matched**,
`shown` carries **rendered and omitted**, and `how` carries the selection policy
**and the facet dimensions and aggregation** — without which the adopted
faceting capability would be missing from its own manifest. When the total was
never counted, `scope` reads `matched unknown` and `shown` reads
`rest unknown`.

**`unknown` and absence are different.** `unknown` is for a claim that *applies*
but was not earned; **absence** is for a claim that does not apply at all.

**Three cardinalities stay distinct**, and conflating them is the trap:

| | |
|---|---|
| **candidate** | what the engine actually evaluated |
| **matched** | rows satisfying the declared filter — claimable **only if fully counted** |
| **rendered** | the top-k or budgeted rows shown |

`matched=843` is honest when the total is a trustworthy byproduct of the *same*
evaluation; otherwise the manifest must say `matched=unknown`. **`rendered` is
the only universally earned claim** — printing *"20 hits across 3 areas"* reads
as a statement about all matches when it may describe only the shown rows.

**Atomicity applies here too:** never run a second count later and present it as
though it described the ranked rows' snapshot. Same rule that made `check`
compute its error rows and non-error count from one fixpoint.

**Never auto-roll rows into groups** — it destroys which members matched and lets
one representative stand for a set. Deliberate replacement can be opt-in later.

One contract derives from CR-D106, carrying type-specific claims. The concern
that motivated two schemas survives in a sharper form: **identical labels do not
imply identical measures**, and each plan must say what its `scope` counts.

### One or two axes, from the user's side

Keep two internally; never ask the user to choose. **The verb types the move**:
`read` changes the representation of one object; `handle`/`status` position it in
containment. Both print their adjacent moves. *Zoom* stays the design metaphor
without becoming a CLI noun.

## 5. Adjacent axes, deliberately not merged

- **Epistemic resolution — already shipped.** `disposition → contributing signal
  set → predicate rows → source spans` is variable-resolution explanation, live
  today in `ranked_anchor` and the drift oracle. The **existence proof** that the
  metaphor is native. Name it; do not merge it with document reduction.
- **Time** is a window over changing objects, not a scale.
- **Convergence** is a measure rendered *over* structural scopes; it composes
  with roll-up rather than forming another scale.
- **Area is containment only.** Directory hierarchy is declared structure, not
  discovered theme — which is why a 575-file consumer corpus graded every area A.
  Semantic scopes, if wanted, should be author-declared collections; community
  detection introduces another oracle.

## 6. Prerequisite for the `declared` lens only

`handle.summary()` resolves `purpose → note → body fallback`, and the three are
**indistinguishable in the output**. The value on our own master spec is plainly
a body fallback, truncated mid-word — not an authored purpose. A lens labelled
`declared` would therefore ship derived text: the conflation class this codebase
spent 2026-07-26 eliminating.

**Fix:** a neutral structured relation beneath the flat projection —
`handle_presentation(handle, text, origin, source_span)` — from which
`declared_summary` is derived *only* from origins whose contracts earn it, and
`opening_excerpt` from body. The name matters: calling it `summary_candidate`
would encode the conflation into the new substrate by labelling a body fallback a
summary candidate. The flat `handle.summary` projection may be preserved for
compatibility; the planner reads the structured relation.

Two rules, the second with teeth:

- `purpose` and `note` are *candidates* for a declared abstraction, with origins
  kept distinct unless a field contract explicitly grants both summary intent.
- **A body fallback is never a summary.** It is an opening excerpt — partial
  coverage, truncation policy disclosed, rendered as `view=opening_excerpt`.
  Carrying `origin=body_fallback` while rendering `view=declared` would
  *instrument* the lie rather than remove it.

Until this lands, `declared` is a lens anneal cannot truthfully offer. **It
blocks nothing else** — map, focus, faceting, and the `full > map` auto chain are
all independent of it.

## 7. Dependency graph, not a ladder

Semantic dependencies, by track:

```
containment      D ──→ A
                 D ──→ G   (set scope facts)
query-free       map ──→ auto(full > map)
declared         handle_presentation origins ──→ declared lens
focused          paragraph spans + scorer contract ──→ focus
```

Nothing above crosses tracks. A delivery order may still be chosen — D, A, G,
map, auto, then declared and focus — but that is **sequencing preference, not
semantic dependency**, and must be labelled as such so no future reader infers an
ordering between incomparable lenses.

## 8. Open questions

1. Does the map cut need a **minimum useful budget** below which it refuses, and
   what is it in practice?
2. Does the ranker's specificity component transfer to paragraph altitude under a
   declared focus, or is paragraph relevance a different scorer? Empirical.
3. Does faceting belong on `search` and `context` alike, given `context` already
   returns a curated bundle?
4. Is there a corpus-level map — the corpus as a heading tree — or is that the
   area rung with better naming?
5. ~~Can the set manifest ever state a true population?~~ **Resolved (§4 G):** a
   true `matched` count does not require rendering the full set, but it does
   require an oracle that evaluated the same selection predicate over the full
   candidate population *in the same snapshot*. Otherwise `matched=unknown`.
   `rendered` is the only universally earned claim.
