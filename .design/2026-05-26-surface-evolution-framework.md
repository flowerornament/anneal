---
status: current
updated: 2026-05-26
author: claude (open for codex review and convergence)
depends-on:
  - 2026-05-13-corpus-runtime.md
  - 2026-05-19-compatibility-surface-retire-audit.md
  - 2026-05-21-code-mode-ergonomics.md
  - 2026-05-25-reduction-audit.md
  - 2026-05-26-remove-focused-command-surface-audit.md
description: >
  Methodology for deciding whether commands and features earn their
  place on anneal's surface. Grounded in the v0.11 audit arc (additive)
  and the v0.12 → v0.13 reduction arc (subtractive). Both shipped real
  changes; the framework distills what made cuts land cleanly and what
  let adds drift. Locks in evidence-based criteria so future cycles
  do not re-litigate first principles.
---

# Surface Evolution Framework

## Why This Exists

Between v0.11.0 and v0.12.0, anneal's default-help surface grew from 17 to 23
commands. Every individual addition was locally justified by an audit
observation — `cookbook` taught composition, `save` promoted queries,
`examples` helped cold readers, `areas` made convergence visible by area. The
cumulative effect was a tool the project owner described as "becoming way too
complex," and a Code Mode story that had drifted toward CLI sprawl.

Between v0.12.0 and v0.13.0, four reduction slices (`cebadb4`, `dc468a5`,
`a7778e2`, `ee537e2`) shrank the visible surface back to 9 commands without
losing capability. The cuts landed cleanly because they followed a discipline:
independent audits with opposite framings, empirical eval-form verification,
two-reviewer convergence, and teaching recovery messages on every removal.

The reduction proved the cuts were possible. This framework locks in the
discipline so the next addition cycle does not undo the work. **The cost of
the framework is the discipline of using it. The cost of not having it is
the additive-gravity problem repeating.**

## Principles

Five claims, each grounded in research-graph entries codex's audit already
cited.

1. **Code Mode is the bet.** The Datalog dialect is the surface. A capability
   expressible via `anneal -e + describe` does not need a separate command
   unless evidence shows the command is materially better. Burden of proof
   is on keeping. Cite:
   *notation-shapes-thought-not-merely-expresses-it*.

2. **ACI simplicity reduces agent error rates.** Fewer, deeper actions beat
   many narrow ones. Each surfaced verb is a thing agents must learn, error
   on, and route around. Cite:
   *aci-actions-should-be-simple-and-minimally-optioned-to-reduce-agent-error-rates*.

3. **Progressive disclosure fails without active prevention of accumulation.**
   Reviews drift toward "what's missing?" because that's where each
   participant is looking. Counter-discipline is required, not optional.
   Cite:
   *progressive-disclosure-of-complexity-fails-when-feature-accumulation-is-not-actively-prevented*.

4. **Notation shapes thought.** Magic words ("convergence", "frontier",
   "settledness", "handle") teach the philosophy when reinforced; dilute it
   when surrounded by neutral verbs. Surface evolution that drops or
   weakens magic words is more expensive than the LOC count suggests.

5. **Fields take off when they find their language, not better tools.** The
   right answer to a missing convenience is rarely "add the command"; it is
   "make the language the convenience." Cite:
   *fields-take-off-when-they-find-their-language-not-when-they-find-better-algorithms*.

## Decision Rule

For each command (existing or proposed), one verdict:

### KEEP if any of:

- **(a) Engine primitive the language depends on.** `eval`, `search`, `read`
  qualify because their absence breaks the language. `init` qualifies because
  bootstrap predates the language.
- **(b) Cold-agent measurement shows materially better discoverability AND
  use** than the eval form. "Materially" means ≥2x reduction in tool calls
  to a correct answer on a representative cold-agent task. Measured via the
  sub-agent experiment shape established in
  `.design/2026-05-20-datalog-learning-path.md`.
- **(c) Composite workflow that no single eval expresses.** `status` qualifies
  because it composes work, blockers, broken, and pipeline counts. `context`
  qualifies because it composes search, read, and neighborhood.

### FOLD if:

- **(a) Capability is real but absorbable** into describe modes, eval recipes,
  or a verb-identity flag on an existing command, AND
- **(b) Absorption preserves usability** for the target agent — measured by
  the same cold-agent test, no regression.

### CUT if:

- **(a) Eval composition is ≤2x the command length**, AND
- **(b) describe + schema can teach the pattern**, AND
- **(c) No real cold-agent flow breaks** — measured against the sub-agent
  cold-agent test.

**Default verdict is CUT.** A command must justify itself to survive. This
inverts the historical bias.

## Evidence Types

Four kinds of evidence, collected for every verdict.

| Evidence | How to collect | Decision threshold |
|---|---|---|
| **Eval form** | Write the exact equivalent query. Run it on at least two real corpora (`.design/`, `/path/to/large-corpus/.design/`). | Length ≤2x command tokens → composable. |
| **Cold-agent trace** | Sub-agent simulates the workflow without the command. Same task, with and without. | ≥2x reduction in tool calls → command earns its keep. |
| **Cluster footprint** | Count LOC across parser, primitive, prelude rules, output rendering, docs. Identify whether removal is one file or a cluster of files. | >100 LOC → proportionally stronger justification needed. |
| **Real usage signal** | Documented evidence agents reach for the command. project owner's `check --area=language` observation is the prototype. | Repeated observed reach AND eval-form fumble → cold-agent test must justify keep. |

## Feature Justification Template

Every new feature proposal and every retain-vs-cut decision in a reduction
audit fills this out. A proposer who cannot complete it does not ship.

```
Proposed: <command name + one-line scope>

Eval equivalent:
  <exact query string, runnable on .design or large-corpus>

Length comparison:
  Command tokens: N
  Eval tokens:    M
  Ratio (M/N):    <decimal>
  Composable:     [Y/N — Y if ratio ≤2x]

Discoverability without the command:
  describe NAME teaches the eval form?     [Y/N + which describe target]
  help eval grammar tour shows the shape?  [Y/N + which section]
  Tool-calls (cold → correct query):       [N, measured]

Cluster footprint:
  Parser/AST changes:    N lines
  Primitive/predicate:   N lines
  Prelude rules:         N lines
  Output rendering:      N lines
  Docs/teaching:         N lines
  Total:                 N lines
  Removable in one PR?   [Y/N]

Real-world signal:
  Agent reach-for evidence:    <quote/link/none>
  Eval-form fumble evidence:   <quote/link/none>

Magic-word impact:
  Words reinforced:  <list>
  Words diluted:     <list>

Verdict:    KEEP / FOLD / CUT
Rationale:  <2-3 sentences citing satisfied criteria>
Reviewer:   <name + date of independent second review>
```

The template's value is the questions it forces the proposer to answer. A
team member writing a feature PR finds out at template-fill time whether the
feature passes the framework. No commit needed.

## Process Integration

Three concrete changes that make the framework operational.

### 1. Feature PR Template

Add the Justification block to `.github/PULL_REQUEST_TEMPLATE.md` (or the
equivalent). Required for any PR that adds a command, prelude predicate,
annotation, or top-level surface. Reviewers reject PRs without it.

### 2. Quarterly Remove-Focused Audit

Same shape as the audits that produced D1-D5. Sub-agent runs the framework
against the existing surface, command by command. Codex independently does
the same. Convergence is the strong signal; disagreements get resolved by
empirical cold-agent test.

Cadence: every minor release (~quarterly). The audit is cheap (a few hours
of sub-agent + reviewer time) and inverts the additive drift that
accumulates between releases.

### 3. Magic-Word Inventory

Annual: enumerate every magic word in active use ("convergence", "frontier",
"settledness", "handle", "lattice", "potential", "entropy", "discharged",
"area", "pattern", etc.). For each:

- Count usages across surfaces (status output, describe cards, help eval,
  spec, README, SKILL.md).
- Verify ≥3 surfaces reinforce it. Words appearing in only one place
  get reviewed for retirement or elevation.
- Track new candidate words and decide whether they earn the magic-word
  tier or stay as descriptive vocabulary.

Words are an asset. Diluted vocabulary teaches less. This inventory keeps
the philosophical surface honest.

## Validation Case: The v0.12 → v0.13 Reduction Arc

This framework is not theoretical. It is the methodology that produced the
reduction arc, distilled into reusable form.

| Slice | Commands cut | Framework rule satisfied |
|---|---|---|
| `cebadb4` | 13 hidden | D5 default-CUT applied across surface |
| `dc468a5` | cookbook cluster | KEEP criteria failed for cookbook verb, primitive, annotation, 7 prelude rules |
| `a7778e2` | save | KEEP criteria failed; Edit/Write absorbs the workflow |
| `ee537e2` | broken, trend, diff, work, blocked, diagnostics, areas, sources | D5 applied per-command; folds and cuts decided |

Each cut followed the loop: independent reviewers ran the framework, the eval
form was tested empirically, convergence was the green light, teaching
recovery messages went out with the removal. The framework reverse-engineers
what made those slices land cleanly.

## What Survives the Framework Today

The 9-command target (post-hmpr execution) passes the framework:

| Command | Rule | Why it survives |
|---|---|---|
| `status` | KEEP-c | Composes work/blockers/broken/pipeline; no single eval expresses |
| `context` | KEEP-c | Composes search + read + neighborhood for one goal |
| `search` | KEEP-a | Engine primitive |
| `read` | KEEP-a | Engine primitive |
| `handle` | KEEP-a,c | Stored-relation projection + neighbor edges; --impact folds reverse deps |
| `schema` | KEEP-a,c | Catalog introspection; required for discovery |
| `describe` | KEEP-a,c | Teaching surface; required for discovery |
| `eval` | KEEP-a | The language itself |
| `init` | KEEP-c | Bootstrap when no anneal.dl; predates the language |
| `help` | KEEP-c | Meta |

Hidden compat exceptions (2):

| Command | Rule | Justification |
|---|---|---|
| `check` | KEEP-exception | CI muscle memory; documented as `diagnostics --gate` alias |
| `prime` | KEEP-exception | Agent skill loader contract; documented as `help agent` alias |

Every other command in v0.12.0 either failed the framework (cut), was
absorbed (folded), or is gone via teaching recovery. The framework gives
the same answer the audits converged on.

## Self-Critique (Open for Codex)

Three places the framework could be wrong and warrant pushback before
landing as durable methodology:

1. **Quantitative thresholds (2x ratio, 100 LOC, ≥3 magic-word surfaces) are
   somewhat arbitrary.** They are reasonable starting points based on the
   v0.12 reduction data, but they have no first-principles derivation. A
   stricter or looser threshold might produce different verdicts on edge
   cases. Worth empirical calibration over the next 2-3 reduction cycles.

2. **The Feature Justification Template adds process friction.** That is the
   point — friction at proposal time prevents friction at maintenance time.
   But the template could devolve into checkbox theater if proposers
   complete it without honest answers. Mitigation: independent reviewer
   must validate each filled-out field, not just acknowledge presence.

3. **Adding a framework to fight feature accumulation is itself meta-feature
   accumulation.** The most honest version of the user's complaint about
   "evaluative process led to adding more features" applies to this doc
   too. The framework's existence is justified only if it actually reduces
   net additive drift over the next 2-3 cycles. If it does not, retire it
   and find a lighter discipline.

## Proposed CR-D

**CR-D102 (Surface Evolution Framework).** Anneal's command surface and
top-level features are evaluated using the framework specified in
`.design/2026-05-26-surface-evolution-framework.md`. The Feature
Justification Template is required for any PR adding a command, prelude
predicate, annotation type, or top-level CLI surface. Quarterly
remove-focused audits run the framework against the existing surface.
Annual magic-word inventories track vocabulary health. Quantitative
thresholds (2x eval-form ratio, 100 LOC cluster footprint, ≥3 magic-word
surfaces) are starting points subject to calibration after 2-3 reduction
cycles. Rationale: the v0.12 → v0.13 reduction arc demonstrated that
disciplined removal is possible; this framework locks in the discipline
so future addition cycles do not undo the work.

## Acceptance for v0.13

This doc is `status: current` once codex reviews and either converges or
substantively dissents. Convergence is documented in a Codex review section
at the bottom (mirroring the pattern from
`.design/2026-05-19-compatibility-surface-retire-audit.md`). Once landed:

- Feature Justification Template ships in `CONTRIBUTING.md` or PR template
- CR-D102 added to master spec
- Quarterly audit cadence enters the project rhythm
- Annual magic-word inventory becomes a planned activity

## Codex Review Section

*Reserved for codex's independent review. Pushback expected on the
quantitative thresholds, the Feature Justification Template's likelihood
of honest completion, and the meta-question of whether this framework is
itself the additive-gravity problem in process form.*
