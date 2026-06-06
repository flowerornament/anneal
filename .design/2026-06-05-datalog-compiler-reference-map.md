---
status: draft
date: 2026-06-05
epic: anneal-kftp
authors: [codex]
reviewers: [claude]
relates:
  - .design/2026-06-01-perf-architecture-arc.md
  - .design/2026-06-02-pass-contracts.md
  - .design/2026-06-04-plan-ir-reconciliation.md
  - .design/2026-06-04-runtime-architecture.md
---

# Datalog compiler reference map — 2026-06-05

The Plan/IR middle-end should not be designed from latent Datalog folklore.
This note records a first reading pass over external Datalog/compiler systems:
what each source actually says, what mechanism matters for anneal, and what
anneal should avoid copying.

The test for usefulness is narrow: each reference should sharpen the kftp arc's
implementation reviews. If a source does not help answer "did the plan capture
meaning once?" or "did execute get dumber?", it belongs in background, not in
the active design spine.

This is a reference/rationale note, not a contract change. It alters no locked
kftp slice; it supplies the review checklist for the slices.

## What was read

Primary papers/docs read for this pass:

- Souffle provenance:
  <https://souffle-lang.github.io/pdf/toplas20.pdf>,
  <https://souffle-lang.github.io/provenance>
- Datafrog docs/source overview:
  <https://docs.rs/datafrog/latest/datafrog/>
- Souffle synthesis/RAM:
  <https://souffle-lang.github.io/translate>,
  <https://www.souffle-lang.com/pdf/cc.pdf>
- FlowLog:
  <https://arxiv.org/abs/2511.00865>,
  <https://pages.cs.wisc.edu/~hangdong/flowlog.html>
- Typed multi-level Datalog IR:
  <https://www.pl.informatik.uni-mainz.de/files/2024/10/datalog-ir.pdf>,
  <https://2024.splashcon.org/details/splash-2024-oopsla/110/A-Typed-Multi-Level-Datalog-IR-and-its-Compiler-Framework>
- Flan:
  <https://www.cs.purdue.edu/homes/rompf/papers/abeysinghe-popl24.pdf>,
  <https://popl24.sigplan.org/details/POPL-2024-popl-research-papers/88/Flan-An-Expressive-and-Efficient-Datalog-Compiler-for-Program-Analysis>
- Differential Datalog / Differential Dataflow:
  <https://mihaibudiu.github.io/work/ddlog.pdf>,
  <https://www.microsoft.com/en-us/research/publication/differential-dataflow/>
- Flix:
  <https://mhyee.com/publications/2016-flix.pdf>,
  <https://doc.flix.dev/lattice-semantics.html>
- Formulog and Making Formulog Fast:
  <https://arxiv.org/abs/2009.08361>,
  <https://arxiv.org/abs/2408.14017>,
  <https://2024.splashcon.org/details/splash-2024-oopsla/97/Making-Formulog-Fast-An-Argument-for-Unconventional-Datalog-Evaluation>
- egglog:
  <https://arxiv.org/abs/2304.04332>
- Nemo:
  <https://arxiv.org/abs/2308.15897>
- Provenance semirings:
  <https://www.cs.ucdavis.edu/~green/papers/pods07.pdf>
- Cascades/Volcano query optimization:
  <https://www.sigmod.org/publications/dblp/db/journals/debu/Graefe95a.html>,
  <https://15721.courses.cs.cmu.edu/spring2017/papers/14-optimizer1/graefe-icde1993.pdf>
- Ascent / Rust macro Datalog:
  <https://s-arash.github.io/ascent/>,
  <https://conf.researchr.org/details/CC-2022/CC-2022-research-papers/8/Seamless-Deductive-Inference-via-Macros>

Local research-graph anchors used:

- **Data independence separates logical model from physical storage enabling
  essential logic to be designed without performance contamination.**
- **Query compilation and caching separates the parse/optimize cost from the
  execute cost enabling repeated queries to amortize planning overhead.**
- **Propagator networks provide provenance for computed conclusions.**
- **Relations are sets of tuples with no ordering on rows or significant
  ordering on columns.**

## Active references

These are the systems/papers that should affect active kftp design reviews.

### FlowLog

FlowLog is the most directly relevant recent paper for the current kftp shape.
Its headline is incrementality, which anneal explicitly defers. The active
lesson is narrower: FlowLog uses an explicit relational IR for each rule that
separates recursive control, such as semi-naive evaluation, from the rule's
logical plan. That boundary lets FlowLog keep Datalog-aware logical
optimizations while delegating execution to Differential Dataflow primitives.

Mechanism to take seriously:

- A per-rule relational IR is not optional decoration. It is the place where
  join/project shape, subplan reuse, sideways information passing, and Boolean
  specialization live.
- Recursive control is separate from rule logic. Semi-naive iteration manages
  deltas and fixpoints; the rule plan describes the relational computation to
  run inside that control.
- Optimizer choices are structural and robustness-oriented because recursive
  Datalog workloads are volatile. Relying on a runtime DB optimizer to rediscover
  the same plan each iteration is called out as wasteful.

Anneal consequence:

- This strongly supports kftp's current split: `StratumPlan` / `RuleGroupPlan`
  / `AtomPlan` should own meaning and schedule; `execute` should be a boring
  recursive-control loop over planned rule bodies.
- The planned migration should not stop at "slot frames are faster." The
  architectural win is that recursive control stops knowing predicate names,
  field names, soft-primitive status, or join readiness.
- Future optimization surfaces should be plan-level transformations, not eval.rs
  special cases.

Do not copy blindly:

- FlowLog is an incremental engine atop Differential Dataflow. Anneal should
  not make incremental maintenance a prerequisite for the one-shot executor.
  Learn the boundary; defer the streaming substrate.

### Typed multi-level Datalog IR

Klopp, Erdweg, and Pacak's OOPSLA 2024 paper is the best reference for extension
discipline. It argues that a Datalog compiler framework needs a typed
multi-level IR so frontends can lower dialect features progressively toward
core Datalog while preserving executability.

Mechanism to take seriously:

- IR extensions should carry static semantics, not just syntax. Their type
  system is bidirectional, flow-sensitive, bipolar, and uses three-valued
  variable-binding contexts because Datalog correctness depends on which
  variables are bound at each atom.
- Progressive lowering lets higher-level constructs exist long enough for
  optimization and then lower toward core features only where needed.
- Binding state is part of the IR contract. It is not something the executor
  should infer from atom order at runtime.

Anneal consequence:

- `plan()` folding resolution into planning is plausible, but only if the plan
  records binding/slot facts explicitly enough that the executor never asks
  "is this variable bound yet?" by name.
- `RuleBodyPlan.execution_atoms` is a step toward this, but not the whole
  contract. Future slices should make bound-input/output slots explicit on
  every `AtomPlan` and should reject unsupported shapes at plan time.
- Primitive calls, time scopes, aggregates, and soft-primitive overrides are
  anneal's local IR extensions. Their static semantics must be catalog/plan
  facts, not inline eval branches.

Do not copy blindly:

- Anneal is not trying to target multiple external Datalog backends today. The
  useful lesson is typed, extension-aware lowering; a public multi-backend IR
  would be premature.

### Flan

Flan reconciles expressiveness and performance by embedding Datalog in Scala and
using multi-stage programming to generate specialized code. The important
reading result is not "generate code"; it is that a streamlined operator
interface can support aggregates, functions, negation, lattices, binary and
multi-way joins, and alternate index structures without a tangled executor.

Mechanism to take seriously:

- Specialized code is one path, but the deeper lesson is specialization
  granularity: a generic abstraction left in the hot path costs performance and
  clarity.
- Rule evaluation can be described through variable-order / multi-way join
  mechanics. The rule plan chooses a variable/atom order, then the executor
  runs the nested lookup/check sequence.
- Index/store choice is a separate axis from logical plan shape.

Anneal consequence:

- The current "execution atoms in greedy readiness order" is a modest version
  of Flan's planned variable/order idea. It is correct to move scheduling into
  the plan and keep store/index mechanics behind tuple-store APIs.
- If anneal later needs a real join optimizer, it belongs in plan() over an
  explicit rule body model, not in `eval_body`.
- General aggregates and primitives should be represented as operator nodes
  with typed inputs/outputs. That keeps future execution strategies open.

Do not copy blindly:

- Flan's multi-stage Scala codegen is not a Rust requirement. Anneal can get
  the architectural win with an in-memory planned executor first.

### Souffle provenance

Souffle's provenance work directly informs `--explain`. The TOPLAS provenance
paper augments bottom-up Datalog evaluation with proof annotations, storing for
each tuple enough data to answer provenance queries lazily and construct
minimal-height proof trees. Souffle's docs describe proof-tree explanations and
note that efficient provenance requires extra information tracked during
evaluation.

Mechanism to take seriously:

- Provenance is evaluation data. It is not reconstructable honestly from final
  rows alone.
- A tuple annotation can be small, but it must connect a derived tuple to a
  generating rule and a proof height / proof fragment strategy.
- Provenance overhead is real and measured; the Souffle TOPLAS paper reports
  around 1.31x runtime and 1.76x memory overhead on large Doop workloads for
  its method.

Anneal consequence:

- The planned executor cannot become authoritative for a stratum until it emits
  byte-identical `DerivationNode` data. The 1a/2a provenance-first sequencing is
  correct.
- Provenance payloads should be plan-node typed: rule, stored scan, primitive,
  comparison, negation, aggregate, time scope. Reconstructing from rendered text
  would violate the trust surface.
- Shadow comparison should compare tuple-to-derivation maps, not just row sets.

Do not copy blindly:

- Souffle provenance is primarily debugging mode. Anneal's explanation surface is
  shipped corpus trust infrastructure, so "debug-only cost" is not an excuse for
  semantic drift.

### Souffle RAM

Souffle's synthesis pipeline is the precise Datalog precedent for anneal's
`Plan` layer. Souffle lowers a Datalog program through an AST into a Relational
Algebra Machine (RAM): an abstract machine that expresses relational operations
and fixed-point computations, after which mid-level optimizations and C++ code
generation happen.

Mechanism to take seriously:

- RAM is not the original syntax and not the final backend. It is a relational
  execution IR between declarative rules and generated execution.
- Semi-naive evaluation is specialized with the IDB to produce an imperative
  relational program.
- Relational algebra operations and fixed-point control are represented
  explicitly enough to optimize before code generation/interpreting.

Anneal consequence:

- `AtomPlan` / `RuleBodyPlan` / `StratumPlan` are anneal's RAM-scale artifact.
  They should not merely mirror the AST; they should be the executor-facing
  relational machine.
- Plan nodes should increasingly become relational operations with resolved
  slots and relation ids. The runtime should stop seeing source-level predicate
  names.
- The correct migration question is "can this prelude construct lower to the
  anneal RAM subset yet?" not "can eval.rs special-case this predicate?"

Do not copy blindly:

- Souffle's final target is specialized C++. Anneal's next target is a boring
  Rust VM. RAM is the precedent; C++ synthesis is not the near-term goal.

### Datafrog

Datafrog is the small Rust counterweight to heavyweight compilers. Its docs name
`Relation` as a static ordered list of distinct tuples and `Variable` as a
monotonically growing tuple set inside an iteration.

Mechanism to take seriously:

- Boring tuple collections are powerful. Sorted/distinct state is an invariant,
  not a side effect.
- Recursive evaluation can be expressed as explicit monotone growth in an
  iteration context.
- A Rust Datalog substrate can be small, library-shaped, and still useful.

Anneal consequence:

- Keep tuple-store invariants explicit: relation order, uniqueness, index state,
  and row ids should be testable.
- Resist rebuilding a general Datalog engine abstraction when a compact corpus VM
  plus plan artifacts is enough.

Do not copy blindly:

- Datafrog delegates language, planning, provenance, aggregation, negation, and
  primitives to the host. Anneal needs those as first-class runtime contracts.

### Volcano and Cascades

Volcano/Cascades are not Datalog systems, but they are the operational query
compiler precedent behind anneal's logical/physical separation. Cascades is an
extensible optimizer framework built around memoized search over logical and
physical expressions, transformation rules, implementation choices, enforcers,
and tracing.

Mechanism to take seriously:

- A query optimizer distinguishes logical meaning from physical implementation.
- The memo/search model makes optimization a transformation over plan artifacts,
  not ad hoc runtime branching.
- Enforcers/glue operators are first-class plan nodes when physical properties
  must be satisfied.

Anneal consequence:

- The pass-contract language of logical rows vs tuple substrate has a mature
  query-compiler analogue. `plan()` should be the boundary where source-level
  Datalog becomes a logical/physical execution artifact.
- Future optimizations, such as join order, demand-driven filters, or output
  ordering, should be transformations or enforcers over `Plan`, not hidden
  branches in `execute`.
- This also argues against expanding the executor with "temporary" knowledge of
  predicate names. That is precisely what a plan exists to remove.

Do not copy blindly:

- Anneal does not need a full Cascades memo optimizer now. The near-term lesson
  is representation and boundary discipline, not cost-based search.

## Background references

These should shape vocabulary and future direction, but should not pull the
current slice away from the planned one-shot executor.

### DDlog and Differential Dataflow

DDlog compiles traditional-looking Datalog into Differential Dataflow so input
changes produce output changes incrementally. Its paper stresses that
incremental algorithms require maintained intermediate results and incremental
versions of operations; the compiler/runtime absorbs that complexity. The
runtime maintains temporal indexes and shares indexes to reduce memory.

Use later for:

- Persistent corpus state and file-change recompute.
- Snapshot diffs as update streams instead of CLI cache entries.
- Deletion semantics where alternate derivations decide whether a fact remains.

Do not use now for:

- Justifying a cache around a still-confused one-shot executor.
- Mixing incremental update state into kftp Phase 3 before plan/explain are
  boring and differential-clean.

### Flix

Flix gives the lattice/fixpoint vocabulary that anneal's convergence model has
been using informally. The PLDI 2016 paper separates model-theoretic semantics
from evaluation strategy and defines Flix as Datalog plus lattices and monotone
functions. It explicitly says the semantics defines what the solution is, not
how to compute it.

Use now for:

- Naming convergence ordering, status tiers, potential/entropy, and terminal
  exclusions as lattice-ish semantics where that clarifies tests.
- Keeping monotonicity and finite-height assumptions explicit when defining
  convergence predicates.

Do not use now for:

- Adding a general user-programmable lattice language. Anneal has a domain
  convergence lattice, not a Flix clone.

### Formulog and Making Formulog Fast

Formulog is the external-capability boundary reference. It integrates Datalog,
ML-style functions, and SMT solving through a type system that prevents normal
evaluation and solver interaction from going wrong. The 2024 performance paper
is also a warning against treating semi-naive as universal: for SMT-heavy
workloads, eager evaluation can outperform conventional semi-naive techniques.

Use now for:

- Treating primitives/capabilities as typed plan nodes with provider,
  capability, demand, and memoization semantics.
- Remembering that evaluation strategy is workload-sensitive. Semi-naive is the
  baseline, not a law of nature.

Do not use now for:

- Growing SMT, a functional sublanguage, or eager execution in anneal. The
  current prelude workload has not earned that complexity.

### egglog

egglog is useful because it unifies Datalog and equality saturation while
calling out the dual-representation problem in relational e-matching. It also
shows Datalog-like fixpoint reasoning with cooperating analyses and
lattice-based reasoning.

Use now for:

- A design smell checklist: if a planned feature creates two synchronized
  representations of the same logical data, stop.
- Reinforcing that analyses, equality, and lattice data should live in the data
  model when they become first-class.

Do not use now for:

- E-graphs, congruence closure, or extraction as anneal features. They are not
  part of the current corpus runtime.

### Nemo

Nemo is a modern Rust in-memory rule engine for data-centric analytic Datalog.
The short ICLP demo reports a Rust implementation, in-memory operation, a focus
on large data, and a combination of columnar data structures, multi-way
execution, and rule-planning work.

Use now for:

- Comparing Rust-native storage ergonomics and relation/index APIs.
- Keeping columnar storage as a measured future option if scans dominate.

Do not use now for:

- Replacing anneal's corpus-specific runtime with a general rule engine.

### Provenance semirings

Green, Karvounarakis, and Tannen's provenance semirings work is the database
theory foundation for "provenance as algebra over annotated relations." It
unifies incomplete databases, probabilistic databases, bag semantics, and
why/how-provenance by treating tuple annotations as semiring values, then
extends the idea to recursive Datalog with fixed points.

Use later for:

- Alternate derivations and deletion/incremental maintenance.
- Deciding whether `DerivationNode` should eventually become a compositional
  evidence algebra.

Do not use now for:

- Replacing the practical byte-identical `DerivationNode` contract needed to
  migrate the executor safely.

### Ascent and Rust macro Datalog

Ascent is the Rust-native compile-time peer to keep in view. It embeds
Datalog-like inference in Rust through macros, compiles with the host Rust code,
supports semi-naive fixpoint execution, and uses Rust's trait system for richer
data structures and lattice-like extensions.

Use now for:

- Explaining why anneal did not simply lean on a macro Datalog engine. Anneal's
  program is dynamic: shipped prelude plus project rules plus inline query-local
  rules plus runtime source adapters. That wants a runtime Plan/IR, not a
  compile-time macro expansion.
- Comparing Rust ergonomics for relation declaration and user-defined data
  structures.

Do not use now for:

- Reintroducing a static macro surface. The master spec explicitly keeps the
  dynamic IR as the runtime owner; there is no general external `.dl` loader or
  compile-time prelude baking step.

### Magic sets and demand-driven evaluation

Magic-set transformations are the classic bridge between bottom-up Datalog and
goal-directed query evaluation: rewrite the program so known query bindings
restrict what facts are materialized.

Use later for:

- Narrow `context`/`read`/goal-specific queries that do not need full-corpus
  materialization.
- Query-local rules after the one-shot planned executor is stable.

Do not use now for:

- Complicating authoritative stratum migration. First make bottom-up planned
  execution correct, boring, and explainable.

## Standing kftp review gate

Each planned-executor slice should be reviewed against these questions:

1. Did the plan capture predicate meaning once?
   - relation kind, relation id, field ids, arity, primitive provider,
     capability, demand, and soft-primitive override status should be resolved
     before execution.
2. Did execute get dumber?
   - no name-based predicate dispatch, no runtime greedy readiness scheduling,
     no field-name lookup, no "is this primitive?" rediscovery in the migrated
     path.
3. Did variable binding become a planned slot contract?
   - each atom should know its input slots and output/binding slots; unsupported
     shapes should fail at plan time.
4. Did provenance survive as data?
   - every authoritative planned path must emit byte-identical derivations, and
     shadow gates should compare tuple-to-derivation maps.
5. Did recursive control stay separate from rule logic?
   - strata/fixpoint/delta loops own recursive control; rule plans own the
     relational body computation.
6. Did we avoid dual representations?
   - no internal NamedRow rebuilds, no parallel row models, no output projection
     before the boundary.
7. Did we defer incremental/caching until the one-shot executor is clean?
   - DDlog/FlowLog-style incrementality is a later architecture, not a shortcut.

## Consequences for the next slices

- Slice 2a stored-scan provenance should be treated as a Souffle-style
  provenance annotation problem over tuple row ids, not as a renderer over
  projected rows.
- Slice 2b entropy migration is non-recursive, so it only exercises the simple
  half of the FlowLog boundary: the stratum runner invokes the planned body once
  and does not run a delta loop. The full recursive-control payoff waits for the
  recursive strata / `DeltaPlan` slice.
- Any future join optimization should be a plan transformation. Do not add
  heuristics to eval.
- The typed-IR paper suggests a missing anneal artifact: a small "binding
  contract" structure on `AtomPlan` documenting bound inputs, produced outputs,
  and strictness. That would make plan reviews sharper and executor code
  smaller.
- The next design review should ask whether `PlanCatalog` is already rich
  enough to prevent all runtime rediscovery. If not, enrich the catalog before
  migrating more strata.

## Tensions — where this research challenges current kftp assumptions

The rest of this doc records where each reference reinforces the kftp arc, and
that framing risks confirmation bias. This section deliberately records where the
research challenges current kftp assumptions: items to evaluate, not assume
past. (Tracked as bd anneal-kh6p.)

1. **Recursion necessity — is `DeltaPlan` premature?** The prelude is currently
   100% non-recursive; recursion lives in graph *primitives* (Rust), not Datalog
   rules. Semi-naive deltas and the planned `DeltaPlan` (Phase-3 slice 3) exist
   *for* recursion. If recursion stays a primitive concern, the planned executor
   can be single-pass forever — 1b already asserts `recursive == false` — and
   slice 3 is premature generality, a real simplification toward a dumber
   executor. Formulog's "semi-naive is not universal" points the same way. The
   open question to answer *before* building `DeltaPlan`: does anneal want
   recursive Datalog rules (e.g. code-as-corpus transitive dependencies) ever,
   or is recursion permanently a primitive concern?

2. **Lazy vs eager provenance.** Souffle stores minimal annotations and
   reconstructs proof trees on demand; anneal builds full `DerivationNode`s.
   Slice 1a made trace collection lazy under explain/shadow, but the
   eager-vs-lazy trade was never explicitly evaluated. Confirm the
   default/interpreted path isn't paying eager provenance cost when `--explain`
   is off.

3. **Join model.** FlowLog/Flan point at worst-case-optimal / leapfrog joins,
   sideways-information-passing, and a robustness-first *structural* optimizer;
   anneal's planned order is naive greedy. This is deferred — but is the right
   defer-rationale durable, or just "corpus scale" (which expires with
   code-as-corpus)? Don't assume greedy is sufficient. (Resolved below.)

4. **Provenance as algebra (semirings).** `DerivationNode` is a proof tree;
   provenance semirings are a compositional annotation algebra that handles
   alternate derivations and deletion. If incremental/persistent evaluation ever
   lands, the proof-tree *shape* may be wrong. A real future fork, not
   "obviously future-proof."

### Resolved (kh6p, 2026-06-06)

Four parallel research agents evaluated the tensions above against anneal's code,
the research graph, and the literature. Two changed the plan, one was validated
with evidence, one was a premature worry defused with a cheap hedge.

1. **Recursion / `DeltaPlan` — DEFER (plan change).** The *interpreted* executor
   already has a working, tested semi-naive delta loop; `DeltaPlan` would
   duplicate it into the planned path for recursive strata that do not exist.
   code-as-corpus does not force recursive rules (the adapter is shelved; the
   spike's value was stability-as-lattice, not transitive closure; the Cedar
   precedent — ban recursion, expose an *indexed* transitive built-in — is
   anneal's exact stance). Resolution: the **planned executor stays
   single-pass-only**; recursion stays in the interpreted fallback; keep the
   `recursive`/`deltas` plan slots as inert markers and the
   `PlannedExecutorRecursiveAuthoritative` guard as the explicit contract.
   Revisit only on a real recursive `.dl` rule on a hot path with a measured
   interpreted-fallback bottleneck. **Caveat:** master-spec `CR-D18` commits the
   *language* to stratified positive recursion — this defers the *compiled* path
   only; the interpreted path already honors `CR-D18`.

2. **Lazy vs eager provenance — NO CHANGE (validated).** anneal already builds
   `DerivationNode` only under `--explain`; the default path produces zero
   provenance bytes (the post-arc profile has no derivation bucket) — strictly
   *more* lazy than Souffle's always-on annotations. Adopting Souffle's scheme
   would regress the common case and expand the shadow-parity surface. Minor
   low-priority cleanup: the `TracedBinding` wrapper in the unified `eval_body`
   (empty `Vec` + two map/collect passes) — make trace a type-level distinction,
   not a runtime `bool`.

3. **Join model — DEFER WCO; rationale corrected.** The durable reason it is safe
   to defer is *not* corpus scale (that expires with code-as-corpus) but
   **structural**: every blowup-prone shape (reachability / impact / degree /
   closure) is an engine *primitive* routed around the greedy executor, and the
   prelude's own joins are star/path-shaped on indexed keys with no triangle or
   cyclic joins. The real near-term cliff is the single-column-index **full-scan
   fallback**, not join order — so the next lever, if needed, is **multi-column
   indexing on `*edge`**, then sideways-information-passing, then WCO/leapfrog
   (lowest ROI until a cyclic shape on a large graph exists).

4. **Provenance as algebra — PREMATURE; cheap hedge.** `DerivationNode` is
   single-proof and explain-only (not on the fixpoint/correctness path), and
   there is no incremental maintenance to break — so no lock-in. When incremental
   deletion is built, the entry point is **support counts (DRed / `N`-semiring
   multiplicity) on `DerivedRelation`**, not extending `DerivationNode`, not full
   provenance polynomials. Hedge: this note, plus a comment at
   `insert_with_derivation` that its early-return keeps the first proof only and
   is not deletion-safe.
