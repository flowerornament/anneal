---
status: superseded
superseded-by: 2026-05-13-corpus-runtime.md
date: 2026-05-13
depends-on:
  - 2026-05-03-language-redesign.md
  - 2026-05-07-engine-spike-and-parity-protocol.md
  - 2026-05-13-engine-spike-results.md
description: >
  Reframe of the v2.0 product story. Folded into 2026-05-13-corpus-runtime.md
  along with the substrate/adapter split. Kept here for the workflow
  evidence (cold-agent simulation on large-corpus) and the peer-review notes
  that drove the reframe.
---

# v2.0 Reframe: anneal as Programmable Corpus Runtime for Agents

This document supersedes the **product framing** of `2026-05-03-language-redesign.md`
without invalidating its architecture. Sections of that spec that
remain authoritative: Part II (engine/prelude/project layering),
Part III (language grammar, types, aggregation, negation, time
travel), Part IV §15-§18 (prelude layout, convergence vocabulary,
diagnostic ID rules), Part VI (handle model). Sections superseded
here: Part I (motivation), Part V (CLI and I/O — specifically the
"seven verbs as engine commands" frame), and the "convergence as
opt-in" default in Part VII.

The trigger for this reframe is a peer review and a workflow
simulation:

- Peer review (codex, 2026-05-13) on `2026-05-13-engine-spike-results.md`
  argued that shipping "one language" without search/read/schema/help
  as equally first-class primitives "replaces 14 commands with a
  smaller but harder shell — agents will still flail; they'll just
  flail in Datalog."
- Large Corpus-corpus simulation (cold-agent task: "find the most urgent
  thing blocking v17 conformance and read enough to make progress")
  showed v1.1 takes 5 commands with red herrings; v2.0-as-specced
  hits a dead end on verbs alone and forces composite Datalog;
  primitives-first with `search` resolves in 2 commands.

---

## Part I: The Reframe [CV-F]

### §1 What changes

The original v2.0 spec frames the product as **language-first**:
collapse a 14-command CLI into one Datalog dialect plus seven verbs.
This document reframes the product as:

> **anneal is a programmable knowledge-corpus runtime for agents:
> searchable content, typed relations, explainable views, and saved
> verbs.**

The Datalog dialect from `2026-05-03-language-redesign.md` Part III
is still the rule layer. What changes is which primitives the engine
exposes and how the user-facing surface composes them.

### §2 Why it changes [CV-D1]

**Definition CV-D1 (Cold-agent test).** Given a real corpus and a
goal, a cold agent (no prior session memory of the corpus) reaches
the answer in ≤2 tool calls plus optional `--explain`.

The original spec's seven-verb surface fails this test. Its `find`
is identity search; its `blocked` answers a health question; its
`work` answers a prioritization question. None answers "where is the
relevant document I haven't named yet?" An agent without
localization-by-content has to enumerate handles or compose composite
Datalog from prelude knowledge it doesn't yet have — which forces
multiple round-trips.

A primitives-first surface that puts content search alongside graph
queries makes the cold-agent test trivially satisfiable on day one,
not as a follow-up.

### §3 What stays the same

The engine/prelude/project layering from `2026-05-03-language-redesign.md`
§4 is unchanged. The Datalog dialect is unchanged. The convergence
vocabulary in §16 is unchanged. The diagnostic ID rules (LR-R1..R3)
are unchanged. The seven verbs from §19 are kept, but ontologically
demoted (see §CV-V below).

---

## Part II: Primitives — what the engine ships [CV-P]

### §4 Stored relations (engine-populated) [CV-D2]

**Definition CV-D2 (Stored primitives).** The engine-populated
relations any rule may join on. Additions from the original spec
marked **NEW**:

```
*handle{id, kind, status, namespace, file, line, date, area, summary}
*edge{from, to, kind, file, line}
*meta{handle, key, value}
*concern{name, member}
*config{key, value}
*snapshot{at, id, key, value}
*content{handle, span_id, lines, text, tokens}      // NEW
*span{id, handle, start_line, end_line, summary}    // NEW
*search_hit{query, handle, span_id, score, reason}  // NEW (computed, not stored)
```

`*content` and `*span` are the addressable-content layer. A handle
is an identity; a span is a citable region within it. Agents act on
spans, not whole files — `read(handle, budget)` returns spans, and
`search(text)` returns `*search_hit` rows with span ids that compose
with the rest of the query language.

### §5 Function primitives (engine-implemented) [CV-D3]

**Definition CV-D3 (Function primitives).** Engine-provided
predicates that need Rust-native traversal, IO, ranking, or content
access. Additions from the original spec marked **NEW**:

```
// From original §8 — unchanged
upstream(h, anc)               // transitive depends_on
downstream(h, desc)
impact(h, x, depth)
freshness(h, days)
flux(h, days: N) = delta
pipeline_position(h, n)
pipeline_position_for(s, n)
cite_count(h, n)
in_degree(h, n)
out_degree(h, n)
discharge_count(h, n)
terminal(h)
active(h)
obligation(h)
discharged(h)
token_estimate(h, n)

// NEW — content retrieval
search(query, hit)             // hit: *search_hit row; returns ranked candidates
read(handle, budget, span)     // budget-bounded slice; emits *span rows
read_full(handle, content)     // entire file (use sparingly)
match(pattern, handle, line)   // ripgrep-style regex over content

// NEW — self-description
schema(name, kind, signature)  // list relations and function predicates with arity
describe(name, doc)            // doc string for any predicate or verb
source(name, file, lines)      // where a predicate is defined; links to .dl source
verbs(name, query, doc)        // enumerate all verbs (engine + prelude + project)
examples(name, example)        // worked examples per predicate

// NEW — composition helpers
top_k(k, key, body)            // bounded selection: top k by key from body's set
rank(handle, key)              // assign rank within body; ties broken deterministically
```

The composition helpers (`top_k`, `rank`) recognize that pure
Datalog set semantics don't model "give me the 25 most relevant
hits" cleanly. These are first-class primitives, not afterthoughts.

### §6 Provenance is universal [CV-D4]

**Definition CV-D4 (Provenance contract).** Every output record can
be expanded with `--explain` into a derivation tree showing:

- which search hits (with score, reason, matched fields) reached the
  handle
- which content spans were consulted
- which edges and rules joined to produce each fact
- which status/meta values participated

This is broader than the original spec's `_derivation` field. It
applies to *every* primitive — `search` results explain their
ranking, `read` results explain their span selection, derived facts
explain their rule chain. The runtime instruments the IR to produce
derivation by construction, not by per-rule companion relations
(which the engine-spike found don't scale).

---

## Part III: Verbs under Steele's criterion [CV-V]

### §7 Steele's criterion [CV-R1]

**Rule CV-R1 (Verb extensibility).** User-defined verbs in
`anneal.dl` must be **syntactically indistinguishable** from
engine-shipped verbs in the prelude. Same discovery (`anneal verbs`),
same help (`anneal describe <verb>`), same output envelope (NDJSON +
optional `--explain`), same callable shape from rule bodies, same
documentation surface (worked `examples`).

If a project adds:

```dl
# anneal.dl
@verb(
  name: "conformance-blockers",
  query: ? *concern{name: c, member: h}, c starts_with "C-conformance", not settled(h).,
  doc: "OQs and files declaring conformance concerns that aren't yet settled."
)
```

then `anneal conformance-blockers` works in that corpus with all the
same affordances as `anneal blocked` — `--explain`, NDJSON, help,
discovery via `anneal verbs`. There is no privileged distinction
between built-in verbs and project verbs at runtime.

### §8 The starter verbs

The original spec's seven verbs are retained as the **starter
verbs** shipped in the prelude. They are not engine commands; they
are saved expressions in `convergence.dl` and `views.dl`. A project
that prefers different vocabulary replaces or extends them in
`anneal.dl`. The count is not load-bearing — seven is what the
prelude happens to ship, not a design target.

---

## Part IV: Self-description as runtime affordance [CV-S]

### §9 Help is data [CV-D5]

**Definition CV-D5 (Self-description).** Every predicate, verb, and
primitive that the runtime knows about is reachable as data:

```
anneal schema           # list every relation and function predicate, with arity
anneal predicates       # like schema but rule-defined only
anneal verbs            # all verbs (engine, prelude, project) with query
anneal describe <name>  # the doc string for any of the above
anneal source <name>    # file:lines where <name> is defined
anneal examples <name>  # worked examples per predicate
```

A cold agent's first move can be `anneal describe convergence` or
`anneal source release_blocker`. The agent reads the rule definition
directly. This is the Host Corpus-shaped move: the runtime teaches itself
to the agent.

The original spec's §22 help dialog (LR-D10) becomes one entry in
this surface — `anneal help` is just `anneal describe runtime`.

### §10 Onboarding: lattice-on default

The original spec's §27 init detection defaults to "graph mode" when
frontmatter is sparse. This reframe inverts that:

`anneal init` always scaffolds a minimal lattice in `anneal.toml`
(`raw → draft → current → stable`) and writes a starter `anneal.dl`
referencing the prelude's `release_blocker` and `blocked` rules. The
first thing a new user sees is the convergence vocabulary in action,
not "no lattice detected." Graph mode requires an explicit opt-out
flag.

The rationale: the convergence model is the differentiator. Hiding
it behind a configuration gate guarantees that most first-time users
will never see what makes anneal *anneal*.

---

## Part V: Spans, reads, and trails [CV-T]

### §11 Bounded reads with provenance [CV-D6]

**Definition CV-D6 (Read primitive).** `read(handle, budget, span)`
returns spans of the handle's content totaling no more than `budget`
tokens. Each span carries `(id, lines, text, summary, refs)` — a
stable id agents can cite, line range, content, an engine-generated
summary, and the `*edge` rows that originate inside that span.

Agents act on spans, not whole files. A 2000-token budget on a
20,000-token file returns the engine's best 2000-token slice plus
the structure to ask for more. This is the qmd lesson absorbed
properly: retrieval returns *citable, composable, bounded* content,
not opaque blobs.

### §12 Trail capture [CV-D7]

**Definition CV-D7 (Trail).** A session's path — `search` →
candidates → `read` → derived conclusion → verification query — is
the unit of handoff between agent sessions. The runtime writes a
`*trail{session_id, step, expr, summary}` relation by default; a
session-end `anneal trail` summarizes the path and writes it to
`.anneal/trails/`.

Trails are forward-looking — probably v2.1. But the runtime IR must
accommodate them without retrofit. The Bush ("As We May Think") and
Naur ("Programming as Theory Building") insight: agents handing off
to future agents need *paths*, not just *facts*. A trail is more
durable than a session's chat transcript.

---

## Part VI: Acceptance test [CV-A]

### §13 Workflow-completion gate [CV-R2]

**Rule CV-R2 (SP-DR1 capstone).** Phase 1 cannot ship v2.0 surface
to users until the cold-agent workflow gate passes. Specifically:

> Given the task "find the most urgent thing blocking v17 conformance
> in the large-corpus corpus and read enough context to make progress," a
> cold agent (no prior session memory) reaches the answer in
> ≤2 tool calls plus optional `--explain`.

The two tool calls in the target shape:

1. `anneal -e '? search("v17 conformance"), hit.'` → ranked candidate
   handles with reasons.
2. `anneal read <handle>` → bounded content with span ids and
   provenance.

Optional third: `anneal -e '? search_hit ranked above ...' --explain`
to see why a candidate ranked.

This replaces "MVS-1..9 pass" as the primary SP-DR1 gate. MVS still
matters — substrate validation — but workflow-completion is product
validation. If MVS passes and the workflow gate fails, we shipped
a smaller-but-harder shell. If the workflow gate passes and one MVS
sub-criterion is marginal, we shipped a usable tool with known
caveats.

### §14 Other workflow gates

Phase 1 should pin additional cold-agent workflows as acceptance:

| Workflow | Target |
|---|---|
| "What's the corpus state?" | 1 tool call (`anneal`) |
| "Where is X?" | 2 tool calls (`search` + `read`) |
| "What does X depend on?" | 2 tool calls (`anneal H --upstream` or equivalent) |
| "What changed in the last week?" | 1 tool call (`anneal trend --at=--7days`) |
| "Why is this fact in the output?" | 1 tool call (`--explain` on the prior call) |
| "Extend the vocabulary for my corpus" | Write 5 lines in `anneal.dl`; new verb available next invocation |

These gates are written as the spec's *product* acceptance, not
engine acceptance. Engine acceptance (MVS) is necessary but not
sufficient.

---

## Part VII: MCP as primary surface [CV-M]

### §15 MCP, not CLI-first

The original spec's LR-P5 says "MCP is transport, not new
semantics." Correct, and it should also be the *primary* surface,
not a follow-up. If the audience for v2.0 is agents, then:

- MCP tools expose every primitive (`search`, `read`, `eval`, `verbs`,
  `describe`, `schema`, etc.) with proper schemas.
- The CLI is sugar on the same primitives for humans and shell
  scripts.
- Both surfaces are generated from one runtime contract.

The bd issue `anneal-35s` (MCP transport) is promoted from P3 to P1
and folded into the v2.0 milestone. The bd issues `anneal-7t8`
(`anneal search`) and `anneal-d6r` (`anneal context`) similarly
promote.

---

## Part VIII: Scope changes for Phase 1 [CV-Sc]

### §16 Promotions

The agent-ergonomics epic (`anneal-2gf`) is no longer orthogonal to
v2.0. Its three sub-items promote:

- `anneal search` (content retrieval primitive) — P3 → **P1**, in
  v2.0 milestone
- `anneal context` (context annotations) — P3 → **P2**, in v2.0
  milestone (annotation surface; nice-to-have but not blocking)
- MCP transport — P3 → **P1**, in v2.0 milestone

### §17 Additions to Phase 1 closure

`anneal-apa` (Phase 1 closure work) gains the workflow-completion
acceptance test (§CV-R2) as a required gate. Without it, Phase 1
implementation can pass MVS internal checks and still ship a tool
that fails the cold-agent test.

### §18 Demotions

The "seven verbs as the v2.0 surface" framing is demoted to "the
prelude ships seven starter verbs as worked examples; the surface
is the language plus the runtime affordances." This is more honest
about what the surface *is* and avoids locking the verb count by
design committee.

---

## Part IX: What this is not

This reframe does not:

- Change the language grammar (Part III of `2026-05-03-language-redesign.md`
  stands).
- Redesign the convergence vocabulary (the standard library still
  ships `potential`, `entropy`, `blocked`, `advancing` per §16).
- Remove the Datalog interpretation requirement (Phase 1 still
  needs the dynamic IR per the engine-spike results).
- Affect the engine-viability conclusion (ascent for primitives
  remains the architectural call).
- Block on the trail-capture system (§CV-D7 is forward-looking, not
  v2.0).

It changes:

- The product story (corpus VM, not collapsed-CLI).
- The primitives list (adds search, content, span, schema,
  describe, source, top_k).
- The verb model (Steele's criterion; project verbs == built-in
  verbs).
- The onboarding default (lattice-on; graph mode is explicit opt-out).
- The SP-DR1 capstone (workflow-completion, not MVS coverage).
- The agent-ergonomics scope (search, context, MCP fold into v2.0).

---

## Part X: References [CV-Ref]

### Internal

- `2026-05-03-language-redesign.md` — the v2.0 language spec this
  reframe builds on (architecture preserved, product reframed)
- `2026-05-07-engine-spike-and-parity-protocol.md` — SP-DR1
  decision rubric (capstone gate updated by §CV-R2)
- `2026-05-13-engine-spike-results.md` — peer-reviewed engine-
  viability findings; architectural revision (ascent for primitives)
  carries forward
- `anneal-spec.md` Parts I-III — convergence model preserved

### External
- qmd — `https://github.com/jamesrisberg/qmd` — semantic search +
  bounded retrieval; the lesson absorbed: content as addressable
  spans, not opaque blobs
- Cloudflare Code Mode — `https://blog.cloudflare.com/code-mode/` —
  programmability as the agent surface, not menu APIs
- Host Corpus eval design (internal) — runtime self-description so agents
  can teach themselves the model

---

## Labels

### CV-F (Framing)
- CV-F1: Reframe from language-first to corpus-VM-for-agents (§1)
- CV-F2: Cold-agent test motivates the change (§2)

### CV-D (Definitions)
- CV-D1: Cold-agent test (§2)
- CV-D2: Stored primitives, including `*content`, `*span`, `*search_hit` (§4)
- CV-D3: Function primitives, including `search`, `read`, `schema`, `describe`, `source`, `top_k` (§5)
- CV-D4: Universal provenance contract (§6)
- CV-D5: Self-description (§9)
- CV-D6: Bounded reads with span ids (§11)
- CV-D7: Trail capture (§12)

### CV-R (Rules)
- CV-R1: Steele's criterion for verb extensibility (§7)
- CV-R2: Workflow-completion gate replaces MVS as SP-DR1 capstone (§13)

### CV-V (Verbs)
- CV-V1: Verbs are saved templates; engine-shipped and project-shipped are equivalent (§7-§8)

### CV-S (Self-description)
- CV-S1: Help is data (§9)
- CV-S2: Lattice-on default (§10)

### CV-T (Trails)
- CV-T1: Spans as the unit of content (§11)
- CV-T2: Trail capture as v2.1 direction (§12)

### CV-A (Acceptance)
- CV-A1: Cold-agent workflow gate (§13)
- CV-A2: Additional workflow targets (§14)

### CV-M (MCP)
- CV-M1: MCP is primary surface alongside CLI (§15)

### CV-Sc (Scope)
- CV-Sc1: Agent-ergonomics epic folds into v2.0 (§16)
- CV-Sc2: Phase 1 closure gains workflow gate (§17)
- CV-Sc3: "Seven verbs" demoted to prelude content (§18)
