---
status: current
date: 2026-06-09
authors: [claude, morgan]
reviewed-by: codex (adversarial, 2026-06-09 — REVISE-THEN-LOCK, 8 findings folded; final open-leash review requested post-rewrite)
relates:
  - 2026-06-09-the-convergent-corpus-runtime.md   # Part VI/VII — this is the arc after the foundation
  - 2026-06-09-dimensional-foundation.md           # the axes this arc stress-tests
  - 2026-06-09-topic-axis.md                       # pairwise coupling — reused across the corpus boundary
  - 2026-06-08-currency.md                         # the axis the drift oracle instantiates
  - 2026-05-13-corpus-runtime.md                   # §5 Source · §53 federation · CR-D8/D41 identity · CR-R6 closure · CR-D103/104
  - 2026-06-01-spec-code-coherence.md              # W006 — the embryo of the drift oracle
---

# Code and corpus: the joint graph — 2026-06-09

The scoping spec for the arc after the dimensional foundation. Principles and
data-model shape are endorsed; the oracle audit and extraction sim produce the
evidence before any adapter code.

## 1. The goal

A doc corpus alone is documentation hygiene. A code corpus alone competes
with rust-analyzer. The goal is the **joint graph**: design intent and
implementation as first-class handles in federated corpora, with cross-edges
that carry the trust story:

```
which spec governs this module?          which code realizes this decision?
the spec says X — does the code?         the decision was superseded — does
the code changed — did the spec?         the code still implement it?
```

These cross-layer questions are where agents get burned worst, and nothing
else answers them. The second adapter (`anneal-code`) is the stepping stone
that makes the far side queryable; the cross-relation is the product.

anneal is already half-built for this. Specs cite code paths **as `Cites`
edges to `external`-kind handles today**; W006 attaches existence/history
probe metadata to those handles; `asserts_code` sits on the lifecycle axis;
every fact carries `corpus` + `revision` (CR-D8/D41); federation is schema-
ready (§53). The arc completes a reach the system has already started.

## 2. The keystone: cross-corpus currency

The structural fact this arc is designed around:

> **Code at HEAD is the current *artifact* — it is what runs. A spec is
> current only by maintenance, which is the exception. The default state of
> a design assertion about code is "true as of its authoring," not "true."**

"HEAD is current" is a **PRE-FLIGHT artifact premise, not a lifecycle or
currency claim**. Dead, feature-gated, generated, vendored, test, and
deprecated-but-present code all sit at HEAD; code can also intentionally lag
a freshly updated spec, locally inverting the asymmetry. Code lifecycle
therefore still derives from source evidence (export, reachability,
deprecation, test/generated class); the asymmetry sets the *default reading
of cross-edges*, not the status of any handle.

In a living project, almost the entire design corpus is stale relative to
the code — murail and herald are the named cases. **Mass-staleness is the
normal input, not the degenerate case**, and a stale spec linked to live
code *gains* false authority from the link. So the governing rule, an
instance of CR-D103:

> **No cross-boundary presentation without a currency qualification.**
> Every cross-edge surfaces with its drift disposition, and navigation
> across the boundary routes through current heads — exactly as `--lineage`
> does within a corpus. The honest day-one answer on a real corpus is
> "most assertions have drifted"; surfaces lead with the aggregate drift
> profile and drill down, so the truth is consumable rather than a wall of
> red.

## 3. The data model

Three pieces, each an extension of existing machinery rather than an
invention.

### 3a. Assertions are dated edges

A spec→code edge means "this spec referred to this code *as of an assertion
time*." The assertion time is modeled, not assumed:

- **`*edge` gains nullable `date` + `revision` (CR-D8 amendment candidate).**
  The edge already carries `file` + `line`, so `git blame` (`-w -M -C`) on
  the citing line yields a **machine-verified timestamp + revision for the
  exact assertion** — better provenance than any handle-level date, with the
  right semantics: the line's last substantive edit is the last time an
  author re-asserted the claim. Population is verified-or-null — the field
  carries only earned authority. (Accepted second-order noise: a mechanical
  sweep re-dates a line without re-verifying it; an author still touched the
  claim.)
- **Fallback ladder** when the edge date is null: the citing handle's `date`
  (weaker — frontmatter dates can be editorial, copied, or absent), else an
  explicit **`assertion_date_unknown`** disposition. "A date exists
  somewhere" never silently becomes "this assertion is dated."
- **Within-corpus payoff**: a dated `Supersedes` tells lineage *when*
  displacement was declared; dated `Cites` gives true citation age. The
  amendment is not cross-corpus-specific.
- Population cost/policy (eager at extraction vs generation-incremental vs
  lazy at drift-time) is decided by audit measurement, not in advance.

### 3b. Resolution is derived, not stored (CR-R6, extended)

CR-R6 already settled the within-corpus version of this problem: edges store
reference *attempts*; consumers derive closure by joining. The cross-corpus
resolver is the same pattern across the boundary:

- The markdown side already stores the attempt: a `Cites` edge to an
  `external` handle whose id is the path string.
- The code corpus stores real handles whose `file` field names their path
  (CR-D8), corpus-tagged (CR-D41).
- **The resolver is a derived relation in the federated runtime**: join the
  external handle's path against code handles' `file` (normalized), e.g.
  `code_ref(spec, path, code_handle, disposition)`. Sources stay isolated
  (a `Source` never sees another corpus — §5); resolution happens where
  joins happen.
- **Non-resolution is information, not failure**: an unresolvable path feeds
  the drift dispositions (gone/moved/never-existed) rather than erroring.
- **Granularity**: file-level first — it is what specs actually cite
  (verified on murail). Item-level resolution arrives with item handles,
  mirroring markdown's existing file/section pattern (CR-D41 identity
  qualifies across corpora).

### 3c. The drift oracle is the currency axis, with git as the oracle

The question "has the referent changed since the assertion?" is not a new
axis — it is **currency applied to the referent**, with git history as a
machine oracle instead of author-declared `Supersedes`:

- a rename **is** supersession of the path identity, machine-verified;
- routing through a confident rename chain to the current path **is**
  lineage head-routing;
- commits-since-assertion is the displacement evidence.

The disposition vocabulary (the oracle's only public surface — move-detection
mechanics stay behind it):

| disposition | meaning | authority (CR-D103) |
|---|---|---|
| `referent-intact` | exists; 0 commits since assertion | clean |
| `referent-drifted(n)` | exists; n commits since assertion | clean |
| `referent-present-undated` | exists; no assertion date | existence-only, REPORT |
| `referent-moved → head` | confident rename chain; drift recursed on the successor | REPORT-strong |
| `referent-moved-ambiguous` | candidates only | UNKNOWN/REPORT — never routed |
| `referent-gone` | deleted, no successor | REPORT |
| `referent-unknown` | never in history, or history unavailable | UNKNOWN — signalled |

Only the first two are clean; every degraded input presents as
UNKNOWN/REPORT — never as `intact`, never GATE. The naive form
(`git log <path> --since`) is move-blind — the documented W006/`3nw5` class —
and the shipped probe is HEAD-set-membership only, far weaker than the drift
question. **Move-awareness is a day-one requirement of the oracle, not an
enhancement.**

## 4. The authority ladder, across the boundary

"Principled but loose" = the existing disposition ladder stretched over the
corpus boundary. No total-linkage ceremony (the traceability-matrix failure
mode); partial coverage, honestly signalled:

| rung | mechanism | authority |
|---|---|---|
| **declared** | spec cites a code path; code doc-comment/attr cites a spec or CR-D label | strong — author-declared, machine-checked, drift-qualified (§3c) |
| **derived** | pairwise topic coupling *across* corpora (spec and module sharing citation targets/identifiers) | REPORT — suspect hint, never an asserted edge (the topic-axis design, unchanged) |
| **diagnostic** | drift checks: referent moved/gone/drifted; superseded spec with live code-edges; code with no governing design | GATE/REPORT per check — surfaces disagreement, never auto-fixes |

## 5. The axis hypotheses (what the extraction sim tests)

The nine axes are defined by questions and oracles, not formats — that is
the foundation's bet, and code is its first real test. Current grading:

| axis | code oracle | grade |
|---|---|---|
| lifecycle | `unstable`/`stable`/`deprecated` attrs + export/reachability/test/generated class | plausible — attrs cover public API only; class evidence supplies the rest |
| currency | `#[deprecated(note)]` as declared supersession | useful but weaker than `Supersedes`: deprecation ≠ displacement; note-parsing is REPORT unless it names a resolvable successor |
| topic | shared dependencies (coupling) | strong — with the nondiscriminative-target policy from day one (`Vec`/`Option` are the LABELS.md of code) |
| importance / structure | call/use graph, containment | strong — natural code axes |
| relevance | names, docs, signatures | expected fine |
| recency | `since=` (stabilization, not authoring) · git | **open hypothesis** |
| convergence | stability transitions per release | **open hypothesis** — settling may be release-cadence, not snapshot drift |
| obligations | TODO/FIXME/tracking issues | weak unless explicitly in scope |

The simulation answers these; the spec does not.

## 6. The proof plan

The loop, unchanged: simulate → design → adversarial review → lock →
implement → gate; the coordinator verifies the landed feature against the
*simulation's* expectations.

1. **The oracle audit (`anneal-903i`) — first, before anything.** Runs on
   existing metadata + git only (the external handles and their citations
   exist today). For every spec→code citation on anneal + murail: citing
   handle/date/status · target path · current existence · commits since
   assertion on the exact path · rename/move evidence · history status ·
   final disposition bucket. Plus, riding along: **blame-derived assertion
   dates** (coverage %, divergence from handle dates, per-file cost — the
   evidence for the §3a schema amendment), **resolver-join hit-rate**
   (path normalization against the filesystem), and a cheap **reverse-edge
   scan** (CR-D labels in code comments/attrs) to measure the reverse shape.
   **Stop-rule: if moves cannot be classified honestly, the arc halts here
   and the oracle is redesigned before `anneal-code` grows the graph.**
2. **The extraction sim (`anneal-bqqc`) — second.** rustdoc-JSON over a real
   external crate: what handles/edges/lattice actually come out; per-axis
   verdicts against §5; **scale as a hard output** — handle/edge/content-byte
   counts, load time, fixpoint time — measured before any adapter design is
   blessed (the correctness-gates-miss-perf lesson, applied prospectively).
3. **Then** the adapter design (against `Source` §5) and the resolver design
   (§3b made concrete), each through the loop.

**Flagship sequencing**: first lock on the **self-corpus** (anneal `.design/`
+ `crates/`, where CR-D labels already cross the boundary — anneal annealing
itself, one layer deeper) **plus one external Rust crate** for source-
agnostic extraction. **murail and herald join as hostile validation only
after the oracle is honest** — running them first would drown the review in
false moved/gone classifications. `eygi` (the perf lever) rides alongside;
std-scale corpora make the remaining perf priorities evidence-driven.

## 7. Open questions (true unknowns — the sims grow this list)

- **Recursion** (`kh6p` #1): transitive deps may be the first real demand
  for recursive *rules* over query-local recursion. Answered with the
  consumer, not in advance.
- **Edge-date population policy**: eager / generation-incremental / lazy —
  decided by the audit's cost measurement.
- **Cross-corpus mega-target policy**: the topic cap likely needs its
  corpus-relative form sooner here.
- **Where dispositions live**: per-hit annotation (teaching its follow-up
  query, the v0.20 pattern) vs composable derived predicates — likely both;
  confirmed against audit data.
- **Item-level granularity**: handles vs spans for code items; arrives with
  the adapter, shaped by what specs actually cite.
