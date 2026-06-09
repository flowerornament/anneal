---
status: current
date: 2026-06-09
authors: [claude]
bd: anneal-jkt4
relates:
  - 2026-06-09-the-convergent-corpus-runtime.md   # the synthesis this operationalizes (Part VI moves 1-3)
  - 2026-06-08-trust-invariant.md                  # xy45 — the disposition gate, folded in
  - 2026-06-08-currency.md                         # the proof that clarifying an axis makes features simple
---

# anneal — establishing the dimensional foundation — 2026-06-09

The working spec for the next arc. The synthesis (`the-convergent-corpus-runtime`)
named the target; this establishes the **foundation** that gets us there:
find and fix the **axes**, simplify the codebase, remove what no longer makes
sense, and leave a clean, **evidence-backed** base for the core goal. Not
backward-compatibility constrained. Framed as **one coherent transition**, not a
pile of micro-slices — the axes *are* the map.

## Why this is the frontier

The whole arc taught one lesson, twice: **clarifying an axis is what makes the
features on it correct and simple.** Currency was tangled with lifecycle until we
separated them — and the separation *caught two soundness bugs* and shrank the
design. The open bd queue is the same disease unaddressed: `dqfq` (field-name
inconsistency), `bmq` (file-local vs corpus-level), the recency family, the
143-predicate long tail of uneven evidence. These are **symptoms of unclean
axes**, not independent chores. Treating the cause (the axes) dissolves the
symptoms *and* clears the ground for the clustering keystone (a clean 9th axis,
not a tangled add-on). This is **anneal annealing its own vocabulary** — pointing
its convergence discipline at itself.

## The core goal this foundation serves

Re-findability + trust for amnesiac agents over a churning corpus — i.e.
**provenance + navigation**, presented oracle-honestly. anneal is also the
**prototype/proving-ground for Herald's substrate**; a clean axis foundation here
is what lets that substrate be trusted. Every axis and predicate must earn its
place against this goal — *with evidence*.

## The method (one transition, four movements)

### 1. Establish the axes as first-class
Name the orthogonal dimensions, define each precisely, and **assign every
predicate to exactly one**. An axis is defined by: the **question** it answers,
its **oracle** (what makes its answer earned), its **disposition**
(GATE/REPORT/TREND/PRE-FLIGHT), and its **monotonicity**. A predicate that can't
be placed on one axis is a tangle to resolve or a cut.

| axis | question | oracle | disposition | state |
|---|---|---|---|---|
| **relevance** | matches my query? | text × query | REPORT | clean |
| **currency** | displaced? | `Supersedes` edges | REPORT (marked GATE-able) | **just cleaned** |
| **lifecycle** | draft / operative / retired? | `status` field | REPORT / PRE-FLIGHT | clean (just split from currency) |
| **recency** | authored / changed / observed *when*? | dates · mtime · snapshots | REPORT / TREND | **TANGLED — next untangle** |
| **importance** | central? | degree / cites | REPORT | clean |
| **convergence** | settling? | snapshot deltas | TREND | clean-ish |
| **structure** | organized / connected? | `edge` + kinds | REPORT | broad; `area` is proto-cluster |
| **obligations** | owed? | obligation/discharge facts | GATE? (verify) | under-exercised |
| **topic** *(coming)* | same subject? | labels + community detection | REPORT (never asserted edge) | the clustering keystone |

The deliverable is this table made *true and enforced*: each axis precise, each
predicate placed, tangles named.

### 2. Evidence — exercise or cut
A predicate earns its place only if **a verb, a real query, or a consumer need
exercises it**; otherwise it is a cut candidate. The reduction is an *evidence
pass*: the 143 derived predicates have uneven evidence, and we don't currently
know which are load-bearing. Default verdict, per the surface-evolution ethos:
**CUT**. Removal is the primary act; additions must justify against the goal.

### 3. Simplify and remove
Concrete targets (not exhaustive — the transition is unbounded by design):
- **Untangle recency** — the next currency/lifecycle-style win: separate
  *authored age* (`freshness`) from *change recency* (`changed_within`, retire
  git-mtime as a currency/age proxy) from *history movement* (`flux`, snapshots);
  collapse the overlaps; one clear predicate per sub-notion.
- **Dissolve the symptom-debt** — `dqfq` (field-name consistency), `bmq`
  (corpus-level vs file-local), and kin become axis-cleanups, resolved by getting
  the axis right, not patched in place.
- **Cut dead vocabulary** — unexercised prelude predicates, abandoned families,
  redundant variants.
- **Continue codebase simplification** — the `pcwd` decomposition tail and
  `orpd` "deeper reduction" fold in: the cleaner the substrate, the clearer the
  axes (and vice versa).

### 4. The disposition gate (xy45), applied uniformly
`xy45` becomes the standing rule and a CR-D in the master spec: **every surviving
predicate/surface carries a disposition on a named axis, and presents only the
authority its oracle earns.** This is the gate every survivor must pass and every
new predicate must answer.

## The clean foundation (acceptance — by shape, not task count)

We are done with this arc when:
- the axes are **named, precise, and orthogonal**, with every predicate placed and
  no known tangle (recency resolved);
- the vocabulary is **evidence-backed** — every predicate is exercised or cut;
- every surface is **disposition-typed** (xy45 uniform);
- the symptom-debt beads are **dissolved**, not patched;
- the result is **smaller** (fewer predicates, fewer verbs, the language as the
  power surface) and **ready for clustering** as a clean ninth axis.

Then — and only then — the **clustering keystone** lands cleanly: `topic` as the
ninth axis (reconciling `area`), unlocking topical-navigate + unmarked-currency.

## Non-goals (what we are NOT relitigating)
The substrate is sound and stays: the planned executor, the `ir/`/`vm/` split,
the machine gates, the Source-trait substrate/adapter/surface architecture. This
arc is about the **vocabulary and the axes over** that substrate, plus the
ongoing code simplification — not a re-architecture.

## Evidence discipline (how we stay honest)
- **Exercise-or-cut** is verified against real queries / verbs / consumer use
  (murail is the dogfood corpus), not asserted.
- Axis changes are **differential-gated byte-identical** where they touch
  behavior, **and perf-gated** (the byte-identical-misses-perf lesson).
- Each removal must **delete a manual practice or a real complexity** — subtractive,
  per the synthesis. If a slice only adds metadata ceremony, it doesn't ship.
- The corpus stays its own witness: `anneal check` clean, and the prelude/specs
  themselves trend toward settled (anneal annealing itself).

---

# The axis map — first pass (jkt4.1)

Assigning all 187 relations (12 stored · 32 primitive · 143 derived) against the
live `schema`. **First finding: the vocabulary is three categories, not one flat
axis set.** Only one category is "the dimensions"; the other two are diagnostics
and infrastructure that *cut across* them.

## A — the retrieval / orientation AXES (the dimensions)

| axis | predicates (primitive **bold** · derived) |
|---|---|
| **relevance** | **search · match** · (hit selection feeds the ranker) |
| **currency** | currency_current · currency_current_head · currency_successor · currency_superseded · currency_disposition · hit_currency_disposition(_known) · orientation_replaced · re_opened |
| **lifecycle** | **active · settled · terminal** · operative · status_of · lifecycle_status_candidate · orientation_retired_status · frontmatter_adoption_high · aspirational_code_status · asserts_code |
| **recency** | **freshness · changed_within · git_mtime · flux** · recent_recency · snapshot_history_exists/_present |
| **importance** | **cite_count · in_degree · out_degree · impact · neighborhood · upstream · downstream** · hub · incoming_edge · outgoing_edge |
| **convergence** | advancing · holding · drifting · flow · status_flow · regressed · recently_advanced · entropy · entropy_priority · primary_entropy · potential · potential_subject · potential_weight · effective_potential_weight · frontier · blocked · blocker · ranked_work · work_candidate · status_population · previous_status_population · status_handle_count · status_drifting_reason · status_item · confidence_gap |
| **structure** | **edge · pipeline_position(_for)** · area* (8) · namespace* (9) · section_ref(_edge/_total) · file_parent_dir · file_prefix · handle_file · parent_dir_* · prefix_pair_candidate · same_concern_pair · top_pair · pipeline_stall · max_pipeline_* · next_pipeline_status · forward_dependency_to_next_status |
| **obligations** | **obligation · discharged · undischarged · discharge_count** · undischarged_obligation · multiple_discharge |
| **topic** *(coming)* | reconcile the `area*` family out of structure into its own axis |

## B — the COMPOSITION layer (the ranker) — and the best evidence the axes are real
`ranked_anchor`/`recent_frontier` are **not axes** — they are weighted sums *over*
the axes, and they say so explicitly: `anchor_currency_score`,
`anchor_recent_score`, `anchor_inbound_score`, `anchor_status_score`,
`anchor_curated_score` → `anchor_total` (+ `anchor_primary_why`, `anchor_signal`,
`anchor_eligible`, `anchor_subject`; `recent_active_boost`/`_inbound_boost`/
`_curated_penalty`). **The ranker already decomposes into per-axis scores.** That
is strong evidence the dimensional model is *correct* — the system is already
thinking in axes, just not declaring them. Establishing the axes = formalizing
what `anchor_*_score` already does implicitly, and letting the composition be a
clean weighted sum the disposition gate can reason about.

## C — DIAGNOSTICS (the `check` surface — cross-cutting, disposition-typed)
broken_reference · implausible_ref · stale_reference · spec_code_drift ·
orphan(ed_handle) · missing_frontmatter_file · pipeline_stall · multiple_discharge
· status_broken · diagnostic · incident · stub · confidence_gap — plus the
**S-named duplicates** `s001_orphaned` · `s003_pipeline_stall` ·
`s004_abandoned_namespace` · `s005_pair_count`/`s005_top_pair`. Each diagnostic
references an axis (broken_reference→structure, drift→coherence, orphan→structure,
abandoned→lifecycle); the disposition (GATE/REPORT/TREND) is the per-diagnostic
contract.

## D — INFRASTRUCTURE (config · introspection · profile — not dimensions)
config plumbing: configured_active/terminal/lifecycle/pipeline_status ·
configured_asserts_code* · used_lifecycle_status · lifecycle_config_gap ·
potential_weight_override · overridden_potential_weight_source. introspection/
output: **describe · schema · predicates · verbs · examples · source_of · sources
· read · read_full · token_estimate**. corpus profile: profile_code/doc/issue_corpus.

## The tangles — all six resolved (evidence-backed, 2026-06-09)
1. **recency** — **RESOLVED (jkt4.2 shipped).** Authored-age (`authored_age`
   wrapper over `freshness`) vs change-recency (`changed_recently`, lower
   authority) vs history-movement (`flux`); `git_mtime` rejected as an age/currency
   oracle after the murail simulation showed it degraded (87% of files share one
   bulk-commit timestamp). See `2026-06-09-recency-axis.md`.
2. **S-check wrapper pattern** — **RESOLVED: it earns its keep.** Each wrapper is
   the diagnostic's *named evidence chain* (drillable: `? s003_pipeline_stall(…)`),
   consumed by its `diagnostic(...)` row — deliberate pattern, declared in
   CR-D104. One exception: `s001_orphaned` was an *inverted* deprecation
   (canonical `orphaned_handle` delegated to the deprecated name, "through v0.13"
   at v0.19) — fixed in jkt4.3.
3. **config-status sprawl** — **RESOLVED: exercised.** The chain
   `configured_*_status → used_lifecycle_status → lifecycle_status_candidate →
   lifecycle_config_gap` terminates in the W005 diagnostic — same named-evidence
   pattern. Keep.
4. **potential_weight family** — **RESOLVED: override plumbing cut (jkt4.3).**
   `potential_weight_override` / `overridden_potential_weight_source` /
   `effective_potential_weight` + the `potential_weight.override` config key had
   zero real-corpus usage; CR-D21/CR-D22 shadowing is the retune path. Base
   `potential_weight` stays (load-bearing).
5. **currency/lifecycle residue** — **RESOLVED: both place cleanly.** `re_opened`
   is a snapshot-backed *transition* → **convergence** (TREND), feeding
   `drifting`. `orientation_retired_status` is status-string membership →
   **lifecycle**. The honest subtlety is now CR-D104 rule 3: `status: superseded`
   (declared, lifecycle) and a `Supersedes` edge (structural, currency) are two
   oracles on two axes, allowed to disagree.
6. **`*_pair`/concern family** — **RESOLVED: exercised.** The chain feeds the
   spec-mandated S005 (CR-D50); `top_pair` is its queryable evidence name. Keep.

## Exercise-or-cut — the evidence pass (jkt4.3, executed)
**CUT:** `work_candidate` (deprecated through v0.14, zero consumers anywhere) ·
`s001_orphaned` (inverted deprecation, above) · the potential_weight override
plumbing ×3 + config key. **KEEP (exercised or spec-mandated):** the S005 pair
chain (CR-D50) · s003/s004 wrappers (their diagnostics) · the config-status chain
(W005) · `stub` (CR-D47) · `profile_*` (CR-D58) · `implausible_ref` (W004).
**Correction:** `incident` was mislabeled diagnostics in the first-pass map — it
is the *incidence* predicate (structure), feeding `hub`.

## jkt4.1 verdict — capstone landed
The dimensional model **holds and was already latent in the ranker** (category B
is the proof). The capstone is **CR-D104 (§27.4 of the master spec)**: the nine
axes declared (question / oracle / disposition / monotonicity), the four
categories (axis / composition / diagnostic / infrastructure), and three rules —
one-predicate-one-axis (else tangle-or-cut), compositions-decompose (the
`anchor_*_score` shape), and two-oracles-may-disagree (axes stay separate). With
CR-D103 (authority) + CR-D104 (placement), every new predicate answers *which
axis, what oracle, what disposition* before it ships. The per-predicate
assignment above is the living map; the spec carries the durable law.
