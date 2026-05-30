---
status: superseded
superseded-by: 2026-05-26-surface-evolution-framework.md
updated: 2026-05-26
author: codex + sub-agent audits
depends-on:
  - 2026-05-13-corpus-runtime.md
  - 2026-05-19-compatibility-surface-retire-audit.md
description: >
  Remove-focused audit of anneal's command surface after the Code Mode
  ergonomics arc. The question is not "what can be polished?" but "what
  can be removed, hidden, or collapsed into the language?" Optimizes for
  simplicity, expressive power, compact progressive instruction, and the
  convergence vocabulary that teaches agents how to behave.
---

# Remove-Focused Command Surface Audit

## Why This Exists

The language-first redesign started with a blunt claim: **one language, no
commands**. Verbs were supposed to be saved queries and teaching examples, not
separate product surfaces.

The shipped surface drifted back toward nouns. Each slice was locally useful:
`areas` made convergence visible by area, `cookbook` taught joins, `save`
promoted queries, `examples` helped cold readers. Cumulatively, the tool began
teaching agents a command catalog again.

This audit reverses the burden of proof. A command earns visible surface only
if it is a simple-and-deep agent action: easy to invoke correctly, compact
enough to make meaningful progress, and hard to replace with `describe`,
`schema`, `eval`, or direct `anneal.dl` editing.

## Grounding

Research-graph claims consulted:

- **aci-actions-should-be-compact-and-efficient-so-agents-make-meaningful-progress-per-step**
- **aci-actions-should-be-simple-and-minimally-optioned-to-reduce-agent-error-rates**
- **progressive-disclosure-of-complexity-fails-when-feature-accumulation-is-not-actively-prevented**
- **notation-shapes-thought-not-merely-expresses-it**
- **fields-take-off-when-they-find-their-language-not-when-they-find-better-algorithms**

They point in the same direction: give agents fewer actions, make those actions
deeper, and make the language the place where composition happens.

Manual smoke used:

- anneal `.design`
- `/path/to/large-corpus/.design`
- `/path/to/host-corpus/.design` / `/path/to/host-corpus/.design`

Representative evidence:

- `status` gave immediate convergence shape:
  - anneal `.design`: `broken=0 blocked=2 work=0`
  - large-corpus: `broken=0 blocked=31 work=104`
  - Host Corpus: `broken=12 blocked=12 work=25` in the workflow audit
- `context "formal model v17 conformance blocking question"` on large-corpus found
  the canonical formal-model parent document first and read bounded context.
- `describe diagnostic` plus `eval` was enough to write a file-scoped
  post-edit diagnostic query.
- `cookbook` currently teaches useful recipes, but its `Save:` lines are a
  symptom of surface sprawl: agents already have Edit/Write and can edit
  `anneal.dl` directly.

## Minimal Visible Surface

The visible mental model should be:

| Role | Command | Why it survives |
|---|---|---|
| Arrival | `status` | The convergence landing page. It carries the philosophy: what is broken, blocked, unsettled, or moving? |
| Arrival | `context` | High-progress cold-agent action: search + bounded read + neighborhood in one move. |
| Retrieval | `search` | Ranked content retrieval is too important to make agents hand-write. |
| Retrieval | `read` | Budgeted evidence access is a core agent affordance. |
| Retrieval | `handle` | Local graph inspection is common and compact. |
| Introspection | `schema` | Broad map of callable relations before writing `eval`. |
| Introspection | `describe` | The teaching card for one runtime name. Should absorb examples, cookbook recipes, source locations, and common joins. |
| Language | `eval` / `-e` | The power surface. Composition belongs here. |
| Setup | `init` | Bootstrap/migration path for `anneal.dl`. |
| Meta | `help` | Standard CLI affordance. |

This is the surface agents should memorize.

`prime` is useful, but it is onboarding content rather than a core command. It
can become a help topic (`anneal help agent`, `anneal help workflows`) or stay
available while leaving the first screen.

## Command Disposition

| Command | Disposition | Rationale |
|---|---|---|
| `status` | Keep visible | Core convergence landing surface. |
| `context` | Keep visible | Best cold-start action; bundles retrieval and graph context. |
| `search` | Keep visible | Core retrieval primitive; hand-writing search queries is needless friction. |
| `read` | Keep visible | Core bounded-reading primitive. |
| `handle` | Keep visible | Graph neighborhood of one handle is compact and frequent. |
| `H` | Hide alias | Keep callable as muscle memory; stop teaching it. |
| `schema` | Keep visible | Broad language map; complements `describe`. |
| `describe` | Keep visible | The progressive-disclosure anchor. |
| `eval` / `-e` | Keep visible | The language surface. |
| `init` | Keep visible | Setup and migration. |
| `help` | Keep visible | Meta affordance. |
| `work` | Collapse into `status` + `eval` recipe | Useful, but `status` already exposes work and deeper questions compose with `top_work`. |
| `areas` | Collapse into `status` + `eval` recipe | Useful second-step view, but it is a saved query over `area_health` / `area_frontier`. |
| `blocked` | Collapse into `handle` / `eval` / explain | One-handle blocked view is useful but not a separate first-screen noun. |
| `broken` | Hide or keep only as emotional shortcut | Strong post-edit affordance, but conceptually `diagnostic{severity: "error"}`. If one non-minimal command survives, this is the candidate. |
| `diagnostics` | Hide | Full diagnostic stream and `--gate` are useful, but should not widen the default surface. |
| `check` | Hide compatibility | CI alias for diagnostic gate. Keep callable; do not teach. |
| `trend` | Hide until history story is stronger | Convergence-over-time is real, but empty-history output is common. Fold into status/session-resume design later. |
| `verbs` | Collapse into `describe runtime` / `schema` | Project verb index is useful; teach it as introspection content, not a command to memorize. |
| `examples` | Collapse into `describe NAME` | Examples belong on the teaching card. Separate command fragments the learning path. |
| `cookbook` | Cut whole cluster | Recipes are valuable, but they belong as examples and Common joins on `describe NAME`; the separate command, primitive, and annotation add a second teaching system. |
| `vocab` | Collapse into `schema` / `describe runtime` | Prevents hallucinated filters; preserve the content under introspection. |
| `sources` | Hide | Important for adapter debugging and future federation, not first-screen. |
| `save` | Retire | Duplicates agents' Edit/Write tools and creates a second verb-authoring path. Teach direct `anneal.dl` editing instead. |
| `prime` | Hide or convert to help topic | Useful cold-agent briefing, but not a core action. |
| `find` | Deprecate/remove | Covered by `search` and `eval` over `*handle`. |
| `get` | Deprecate/remove | Covered by `handle`, `read`, and `search`. |
| `health` | Deprecate/remove | Collapsed into `status`. |
| `impact` | Decision needed | High-value "what depends on this?" workflow. Either fold into `handle` output or keep as a recipe; do not leave as hidden legacy if important. |
| `map` | Deprecate/remove | Full graph rendering is niche and risky for compact agent use. |
| `diff` | Decision needed | Strong disconnected-agent/session-resume idea, but overlaps `trend`. Needs a named ritual decision. |
| `obligations` | Collapse into `eval` recipes | Obligation language should survive in docs and predicates, not necessarily as command. |
| `garden` | Deprecate/remove | Nice metaphor, but `work`/`status` should absorb maintenance advice. Extra magic word dilutes convergence. |
| `orient` | Deprecate/remove | Covered by `context`; file-anchored variants can be recipes. |
| `query` | Remove | Parallel query language made of flags. `eval` is the query language. |
| `explain` | Remove | Runtime `--explain` on `eval` and verbs is the cleaner provenance model. |
| `predicates` | Internal/collapse | Covered by `schema`. |
| `source-of` | Hide | Useful provenance detail; show through `describe` instead. |

## Product Decisions

### D1. Does `broken` Survive As A Named Ritual? Resolved: No.

Argument to keep: "Did I break it?" is a high-frequency, emotionally important
agent workflow. `broken` is compact and memorable.

Argument to hide: it is exactly `diagnostic{severity: "error"}`. Keeping it
teaches agents another noun instead of the language.

Decision: cut `broken` as a command. Preserve the word in `status` output and in
the diagnostic vocabulary, but make the executable form be:

```bash
anneal -e '? diagnostic{severity: "error"}.'
```

Rationale: the post-edit "did I break it?" ritual is real, but it should teach
the language rather than add another memorized noun. `status` carries the
arrival signal; `describe diagnostic` and `describe runtime` teach the recipe.

### D2. What Replaces `diff` / `trend`? Resolved: Nothing Yet.

There is a real product concept here: disconnected agents need to know what
changed since last session. The current split between `trend`, legacy `diff`,
and snapshot history is not yet one clean ritual.

Decision: cut `diff` and `trend` as commands until temporal queries are honest.
Do not ship a wrapper around incomplete `at(snapshot:last)` / `at(HEAD~N)`
resolution.

Rationale: "what changed since I last worked?" is a valuable future ritual, but
shipping two partial nouns fragments it. Design a future `resume` /
`since-last-session` surface only after the underlying temporal relation works.

### D3. Where Does `impact` Live? Resolved: `handle --impact`.

Reverse-dependency inspection is useful. It may belong inside `handle` as a
downstream/upstream section, or as a documented eval recipe.

Decision: fold into `handle <H> --impact`.

Rationale: impact is a verb-identity argument, not a workflow filter. It asks
for a deeper view of one handle, so it belongs on the handle-inspection surface.
The raw composition remains:

```bash
anneal -e '? downstream("H", h).'
```

### D4. What Is The Agent Briefing Surface? Resolved: `help agent`.

`prime` carries compact instructions and magic words. The concept should
survive, but "prime" as a command may not need to.

Decision: make `anneal help agent` the canonical briefing. Keep `prime` as a
hidden alias for installed skills and old muscle memory.

Rationale: the briefing is documentation, not a corpus action. It should be
reachable through help, while `prime` stays as a non-discoverable compatibility
shim for agents that already know it.

### D5. Which Hidden Runtime Commands Survive? Resolved: Only Compatibility Shims.

The hide-first reduction left several commands callable but unlisted:
`work`, `blocked`, `diagnostics`, `broken`, `areas`, `trend`, and `sources`.
That was useful while docs caught up, but it is not the target shape.

Decision:

- Keep hidden `check` as the CI gate shim.
- Keep hidden `prime` as the agent-briefing alias.
- Cut `work`, `blocked`, `diagnostics`, `broken`, `areas`, `trend`, and
  `sources` as commands.
- Preserve their underlying relations, predicates, examples, and teaching
  recipes under `status`, `handle`, `schema`, `describe`, and `eval`.

Rationale: hidden-but-callable is a migration tool, not a product stance.
Agent-facing power should live in the language and its introspection surfaces.
The research-graph ACI principles point at "simple and deep": few actions,
compact progress per action, and active refusal of locally reasonable feature
accumulation.

## Follow-Up Plan

1. **Hide-first help reduction.** Change default `anneal --help` to the minimal
   visible surface. Keep hidden commands callable while the docs catch up.
2. **Collapse teaching surfaces into `describe`.** Move examples, source
   locations, vocabulary pointers, and verb listings into `describe runtime` /
   `describe NAME` where appropriate. Cut the cookbook cluster entirely.
3. **Retire `save`.** Remove the file-writing path and make old invocations
   teach direct `@verb(...)` editing in `anneal.dl`.
4. **Retire compatibility commands.** Remove or hide `query`, `explain`, `get`,
   `find`, `health`, `map`, `obligations`, `garden`, `orient`, and old
   flag dialects unless a product decision explicitly rescues them.
5. **Resolve D1-D5.** Do not add new commands until these decisions are made.

## Invariant

New feature ideas must first answer:

> Why can this not be a `describe` improvement, a `schema` view, an `eval`
> recipe, or an `anneal.dl` edit?

If the answer is weak, do not add a command.
