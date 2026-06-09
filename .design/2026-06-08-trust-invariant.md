---
status: current
date: 2026-06-08
authors: [claude]
bd: anneal-xy45
relates:
  - 2026-06-06-disposition-typed-witnesses.md   # the disposition framework this generalises
  - 2026-06-08-currency.md                        # the first concrete instance
  - 2026-05-13-corpus-runtime.md                 # master spec — fold this in as a numbered CR-D
---

# CR-D: the trust invariant — no confident answer over degenerate input — 2026-06-08

This crystallises the principle that currency (`anneal-z4x3`) just proved into a
standing **design gate**. **Folded into the master spec as `CR-D103` (§3, beside
the `CR-R12` degenerate-input rule); registered in Part XV.** Retained here as the
working record of how it was derived.

## The decision

> **A surface may present a result with only as much authority as its oracle
> earns. It presents as a GATE only where it has a *clean oracle on
> non-degenerate input*. On degenerate input it must *signal the degeneracy* —
> not answer with flat confidence.**

For a tool whose mission is letting amnesiac agents *trust* a shared knowledge
state, **a confident wrong-or-empty answer is the worst failure class** — it is
silent, plausible, and upstream of the consumer's reasoning, so no amount of
downstream capability recovers from it.

## Degenerate-input taxonomy

The cases where a confident answer would be a lie (the silent-wrong-answer
cluster — `anneal-v4cd`, `4sgy`, `f1v4`, `m6xy`):

| degenerate input | the lie if answered confidently | honest response |
|---|---|---|
| empty / tiny corpus | "here is the state" (there is none) | declare the premise (empty) |
| unclassified status everywhere | "this is current/terminal" | report "no status signal" |
| no snapshot history | "advancing / holding / drifting" | PRE-FLIGHT: declare no baseline |
| wrong / unresolved root | results for the wrong corpus | error on the premise, not the query |
| score-saturated ties | "the top hit" (arbitrary among equals) | signal the tie, don't pick |
| no clean oracle (e.g. unmarked supersession) | "superseded" / "current" as fact | REPORT hint, never an asserted edge |

## The required response — disposition, honestly

Every surface carries exactly one **disposition**, and the disposition *is* the
contract (from `2026-06-06-disposition-typed-witnesses.md`):

- **GATE** — clean pass/fail oracle on non-degenerate input. May block.
- **REPORT** — graded / human-judged; informs, never blocks.
- **TREND** — slope over snapshots; needs a baseline.
- **PRE-FLIGHT** — witnesses a premise before building on it.

The rule for a new predicate/verb is one question, asked at design time:

> *What is this surface's disposition, does it have a clean oracle on
> non-degenerate input, and what does it do when the input is degenerate?*

If it can't answer cleanly, it must **signal**, not succeed.

## Currency as the first instance

Currency (`anneal-z4x3`) is this invariant on the retrieval surface, and it
already embodies the gate:
- **marked supersession** → clean oracle → GATE-able (`superseded` / `current_head`).
- **unmarked supersession** → no clean oracle → **REPORT hint only, never an
  asserted edge** (the `suspect` disposition).
- **no history / no siblings** → **`unknown`** → signal, don't fake.

Every future surface — `navigate`, hub-ness, the eventual TMS — is designed
against this gate. It is also why anneal's confidence is *consumer-safe*: anneal
is an architecture-witness others run as a gate (murail's `check-design`), so a
false-confident anneal result false-fails a downstream gate. The trust invariant
is what makes anneal's authority earned rather than asserted.
