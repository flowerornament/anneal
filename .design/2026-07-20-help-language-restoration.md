---
status: current
date: 2026-07-20
authors: [morgan, codex, claude]
depends-on:
  - 2026-05-13-corpus-runtime.md
  - 2026-05-16-help-reference-spec.md
  - 2026-05-26-surface-evolution-framework.md
description: >
  Restores anneal's product language to terminal help, unifies help-name
  resolution with runtime self-description, and makes vocabulary drift a
  tested contract rather than a prose convention.
---

# Help Language Restoration — 2026-07-20

## Problem

Anneal's runtime still knows its philosophy, but the first terminal projection
stopped teaching it. In v0.21.6, top-level help contains none of
`convergence`, `frontier`, `potential`, or `entropy`; it describes a mechanical
ladder that could belong to any corpus query tool. The README and
`describe convergence` still carry the product identity.

The loss is wider than copy. `anneal help convergence` and other runtime names
error even though `anneal describe convergence` has a complete teaching card.
Eight of nine visible command-help pages have no examples, and none has the
`See also` links required by HR-D4. Project `@verb` help is already the positive
counterexample: it projects the resolved registry entry and teaches invocation,
schema, capability, source, and query together.

This is the failure CR-D82 and CR-D102 anticipated. Help became a parallel
documentation layer, then its product language drifted away from the runtime.

## Product Thesis

The canonical terminal-help thesis lives once, as the `Product Thesis` section
of the shipped `skills/anneal/SKILL.md`. It names anneal as a convergence
assistant, identifies disconnected intelligences as its audience, and connects
exposed uncertainty to movement toward settledness.

`anneal help agent` includes that section in place. Top-level help extracts and
renders the same section. Tests compare the projections to the canonical text;
similar hand-maintained copies do not satisfy the contract.

The active magic-word tier is:

- product identity: `convergence`, `frontier`, `settledness`, `handle`;
- convergence mechanics: `potential`, `entropy`, `obligation` / `discharged`,
  `flow` / `drifting`;
- trust mechanics: `provenance`, `trail`, `disposition`.

Top help carries the product-identity tier. Agent help carries both product and
mechanics. Runtime teaching cards own semantic depth; status owns the live
instrument reading. `lattice` remains lifecycle/configuration vocabulary, and
`crystallization` remains historical framing rather than being restored by
nostalgia alone.

## Help Surfaces

### First Screen

`anneal help`, `anneal --help`, and `anneal help top` remain root-free and
bounded to one terminal screen. They render:

1. the canonical product thesis;
2. all nine visible commands, grouped by intent with one-line purposes;
3. a compact convergence move (`status`, `describe convergence`, `frontier`);
4. pointers to agent and command help;
5. the existing root and global-option contracts.

The rendered result is at most 60 lines at 80 columns. It does not restore the
old encyclopedic `CORE CONCEPTS` wall.

### Static Command Help

Every static command stays root-free and retains its existing usage, argument,
option, provenance, and output text. Each page additively gains:

- two copy-runnable examples (the existing eval tour may keep its larger set);
- a `See also` line;
- when the command name is also runtime vocabulary, an explicit collision
  pointer such as `Also: anneal describe search (runtime verb).`

The collision pointer is required for `status`, `context`, `search`, `read`,
`handle`, `schema`, `describe`, and hidden `check`. Resolution may choose a
winner; it may not hide the other meaning.

### Semantic Help

Semantic help is corpus-scoped because project rules and verbs can change the
loaded vocabulary. It delegates to existing projections rather than creating a
third prose source.

| Input | Winner | Required disclosure |
|---|---|---|
| `help`, `--help`, `help top` | static first screen | none |
| `help agent` | shipped agent briefing | none |
| `help <static-command>` | static command help | hint matching runtime vocabulary |
| `help <retired-name>` | retired recovery message | taught replacement |
| `help <project-verb>` | resolved `VerbRegistry` entry | hint any additional describe matches |
| `help <topic-or-axis>` | `describe <name>` | byte-identical card |
| `help <predicate-or-source>` | `describe <name>` | byte-identical card |
| `help <unknown>` | unknown-name error | point to schema and describe runtime |

Project `@verb` identity deliberately wins over a same-named axis, predicate,
or topic because bare invocation would execute the verb. The help output must
still disclose additional describe matches; precedence is not permission to
silently shadow them.

The undocumented `help runtime` alias for top-level help is retired by direct
replacement: `help runtime` now projects `describe runtime`. Outside a marked
or explicit corpus it emits a teaching message explaining that semantic help
is corpus-scoped and points to root-free `anneal help` / `anneal help top` plus
`--root PATH`. It must not panic or emit an opaque root error.

## Expected Deltas

Intentional user-visible changes are limited to help:

- top and agent help regain the canonical product thesis;
- static command help gains examples, related pointers, and collision hints;
- `help runtime` changes from root-free top help to corpus-scoped runtime card;
- known runtime names change from an unknown-help error to their existing
  describe output;
- unknown semantic names retain a non-zero teaching error;
- a colliding project verb gains one additive pointer to other runtime meaning.

Direct `describe <name>` output and non-colliding project-verb help remain
byte-identical. No query, evaluation, extraction, ranking, or non-help rendering
semantics change.

## Gates

1. `help convergence`, `help runtime`, and representative predicate/axis/source
   names are byte-identical to direct `describe` in text and JSON modes.
2. Non-colliding project-verb help is byte-identical before/after; a synthetic
   project verb colliding with an axis shows the verb help plus an explicit
   describe pointer.
3. Every static command page has examples and `See also`; command/runtime name
   collisions are disclosed.
4. Top help is at most 60 lines and contains the exact canonical thesis plus
   all nine visible commands.
5. The vocabulary regression checks the active tier across top help, agent
   help, `describe convergence` / runtime cards, and status rendering.
6. `help runtime` outside a corpus returns the named teaching message.
7. Unknown-name teaching behavior remains non-zero and actionable.
8. A same-doc-state differential keeps status, schema, direct describe, eval,
   search, and project-verb execution byte-identical.
9. `just check` passes outside a Git worktree.

## Consequence

Help is again an instrument for acquiring anneal's mental model, not merely a
flag reference. The command surface stays small; the language behind it becomes
reachable through one predictable noun. Future vocabulary additions earn
their place through runtime teaching cards and automatically become reachable
through `help NAME`, while product identity remains a single projected source.
