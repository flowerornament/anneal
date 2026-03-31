---
status: draft
updated: 2026-03-28
note: >
  Specification for anneal — a convergence assistant for knowledge corpora.
  Helps disconnected intelligences (agents across sessions) orient in a
  shared body of knowledge and push it toward settledness. Derived from
  the formal analogy between graded type systems (TensorQTT §17-18) and
  knowledge refinement. Follows the coloring book principle (Host Corpus C-10).
references:
  # External projects (absolute paths or URLs)
  host-corpus-system-theory: /path/to/host-corpus/.design/specs/system-theory/2026-03-24-system-theory.md
  research-graph: /path/to/research-graph/notes/
  napkin: https://github.com/Michaelliv/napkin
  qmd: https://github.com/jamesrisberg/qmd
  # Large Corpus design docs (absolute — anneal's primary test corpus)
  sample-formal-model: /path/to/large-corpus/.design/formal-model/sample-formal-model-v17.md
  large-corpus-labels: /path/to/large-corpus/.design/LABELS.md
  large-corpus-design-readme: /path/to/large-corpus/.design/README.md
  large-corpus-open-questions: /path/to/large-corpus/.design/OPEN-QUESTIONS.md
  large-corpus-v17-synthesis: /path/to/large-corpus/.design/synthesis/2026-03-25-v17-convergence-synthesis.md
  large-corpus-implementation-architecture: /path/to/large-corpus/.design/synthesis/2026-03-01-implementation-architecture.md
---

# anneal — Convergence Assistant for Knowledge

## Part I: Foundations

### §1 The Problem

Knowledge-intensive projects accumulate documents across many sessions: research captures, synthesis analyses, formal specifications, proofs, implementation specs. Each session is driven by an intelligence — human or AI — that arrives, reads what exists, does work, and leaves. No intelligence sees the full history. The documents are the only shared memory.

Over time these documents develop structure: lifecycle stages, cross-reference labels, versioned lineages, proof obligations. But this structure is implicit, scattered across frontmatter fields, naming conventions, and directory placement. Each arriving intelligence must reconstruct the state of the system from scratch. No tool helps them orient, validates the structure's consistency, or tracks whether the system is converging toward settled knowledge or drifting toward fragmentation.

**anneal** reads a corpus of documents, computes a typed knowledge graph, checks it for local consistency, and tracks convergence over time. It is the shared instrument that disconnected intelligences use to coordinate their work toward crystallization.

### §2 Theoretical Lineage

anneal draws on five bodies of theory. Each contributes a specific primitive.

#### §2.1 Context Physics [KB-F1]

A knowledge corpus is a physical system. Each document has a **convergence state** — how settled it is. Raw captures have high potential energy (much work remains). Verified formal specifications have low potential energy (settled, at rest). The refinement pipeline is energy flowing downhill:

```
raw → digested → decided → formal → verified
 high potential                        low potential
 high uncertainty                      low uncertainty
```

Work (refinement, verification, connection) dissipates potential by crystallizing uncertainty into settled knowledge. Entropy (staleness, changing requirements, new research) reintroduces uncertainty. The system's health is visible in the balance: is potential decreasing faster than entropy introduces it?

The arriving agent's job is to find where potential is highest and do work there. The tool's job is to make the potential landscape visible.

#### §2.2 The One Loop (Host Corpus DY-4) [KB-F2]

Host Corpus's system theory identifies a single dynamic operating at multiple timescales:

```
INTERACT → LEARN → FORMALIZE → DISTRIBUTE → DECAY/EVOLVE
```

In a knowledge corpus, this is the **refinement pipeline**: raw observations are synthesized into analysis, analysis crystallizes into formal specifications, specifications are verified, verified results propagate to downstream documents, and obsolete artifacts decay into the frozen archive.

Host Corpus's three crystallization levels (DY-5) map directly:

| Host Corpus level | Knowledge analog | Example |
|---|---|---|
| Session (minutes) | Raw capture | Research log entry |
| Knowledge (days) | Synthesized analysis | Synthesis document with `status: decision` |
| Artifact (weeks) | Formal specification | Formal model section with machine-checked proof |

*Source: Host Corpus system theory §4, DY-4 through DY-5.*

#### §2.3 Graded Type Systems (TensorQTT §17-18) [KB-F3]

Large Corpus's formal model tracks resource usage as a product of semirings: Usage × Cost × Latency × Precision. Grades propagate through programs via composition rules. The propagator computes the unique minimal grade assignment by Kleene's fixed-point theorem.

The structural insight: **a document's convergence state has the same algebraic structure as a program's resource grade.** The set of convergence states forms a bounded lattice with meet (⊓) and join (⊔). This lattice provides the ordering needed to compare states ("is `decided` above `digested`?"), detect regressions ("a `formal` document now depends on a `provisional` source"), and define terminal states ("superseded is a fixed point").

The full propagation machinery (Kleene iteration to fixpoint) is available as a theoretical extension but not the default behavior. Knowledge dependency graphs are shallow — rarely deeper than 3 hops — so local checks (single-hop comparison of connected handles' states) catch the same issues without the cascade problem where transitively-referenced low-confidence documents drag everything down.

*Source: Large Corpus formal model v17 §17-18, Theorem 18.1.*

#### §2.4 Linear Logic and Obligations [KB-F4]

Some knowledge artifacts create obligations: a synthesis document defining proof obligations (P-1 through P-6) creates resources that must be consumed. This is linear typing in Girard's sense:

- **Linear (usage = 1)**: obligations must be discharged exactly once
- **Reusable (usage = ω)**: most artifacts can be referenced any number of times
- **Affine (usage ≤ 1)**: obligations mooted when their creator is superseded

*Source: Large Corpus formal model v17 §17.2 (usage semiring); Girard (1987) linear logic.*

#### §2.5 The Coloring Book Principle (Host Corpus C-10) [KB-F5]

Host Corpus's core architectural insight:

> Host Corpus defines an abstract space — primitives, axioms, a self-building loop. Any specific deployment is a *selection* within that space.

Applied to anneal: the kernel defines handles, a graph, a convergence lattice, local checks, linearity, and convergence tracking. Any specific project is a **coloring** — which handles exist, which states are valid, which namespaces are linear. Large Corpus's .design/ is one coloring. A startup's docs/ is another. The kernel doesn't change between projects. Only the coloring changes.

*Source: Host Corpus system theory §1.3, C-10.*

### §3 Design Principles

**[KB-P1] Files are truth.** The graph is computed from files on every invocation. No separate storage, no sync problem, no drift.

**[KB-P2] Everything is a handle.** Files, sections, labels, versions — all are names that dereference to content and carry convergence state. One primitive, uniformly treated.

**[KB-P3] Inference first, config second.** The tool infers structure from files. Config overrides or adds what can't be inferred. An empty config is valid.

**[KB-P4] Capabilities over process.** The tool reports state and answers questions. It does not enforce a pipeline or gate transitions. The agent decides what to do.

**[KB-P5] Suggestions surface patterns.** The tool recognizes patterns it supports and proposes them when evidence is sufficient. The user learns the tool's capabilities by being shown what's possible, not by reading docs.

**[KB-P6] Decay is healthy.** Documents that fall out of currency reach a terminal state and stop generating noise. The frozen archive can be enormous without affecting the active corpus.

**[KB-P7] Local checks over global propagation.** Consistency is checked between directly connected handles, not propagated transitively. This avoids false cascades while catching real issues at the boundaries where they matter.

**[KB-P8] Machine-readable by default.** Every command supports `--json` output. The tool is designed for agent consumption first, human readability second.

---

## Part II: Core Model

### §4 Handle — The Primitive [KB-D1]

**Definition KB-D1 (Handle).** A handle is a triple (identity, referent, state) where:

- **Identity** is a string that uniquely names the handle
- **Referent** is the content the handle points to (a file, a range within a file, or a definition site)
- **State** is a value in the project's convergence lattice

Handles are the only objects in the system. Every question anneal answers is about handles.

A handle is a **persistent identity for a chunk of knowledge** — analogous to jj's change IDs, which persist across rewrites. A label like `OQ-64` tracks a specific intellectual question as it moves through the convergence pipeline: created in a synthesis doc, discussed in the formal model, eventually resolved or deferred. The label is the thread that connects all manifestations of that idea.

#### §4.1 Handle Kinds [KB-D2]

**Definition KB-D2 (Handle Kind).** Handles are classified by kind, which determines discovery, resolution, and valid states:

```
HandleKind =
  | File(path)                              — a markdown file
  | Section(parent: Handle, heading)        — a heading range within a file
  | Label(prefix: String, number: Natural)  — a cross-reference (OQ-64, A-10)
  | Version(artifact: Handle, n: Natural)   — a version of a versioned artifact
```

The kind is **inferred from syntax** [KB-P3]:
- Paths ending in `.md` → File
- `§` followed by digits → Section
- `[A-Z][A-Z_]*-\d+` → Label (candidate; confirmed by namespace recognition)
- `v\d+` in versioned context → Version

#### §4.2 Handle Resolution [KB-D3]

**Definition KB-D3 (Resolution).** Given a handle identity string, resolution determines its kind from syntax, then locates its referent:

- File handles resolve by filesystem path
- Section handles resolve to a heading range within a parent file; unqualified `§14` resolves within the current file context, or produces an ambiguity diagnostic if cross-document [KB-OQ2]
- Label handles resolve by scanning all files; the **definition site** is the primary location (configurable per namespace); all other occurrences are **reference sites**
- Version handles resolve by matching versioned artifact naming conventions

Ambiguous handles (e.g., `OQ-64` defined in both LABELS.md and OPEN-QUESTIONS.md) resolve to the configured definition file for that namespace. Multiple conflicting definition sites produce a warning.

Resolution failure (dangling reference) is an error diagnostic.

#### §4.3 Handle Namespace Inference [KB-D4]

**Definition KB-D4 (Namespace).** A handle namespace is a label prefix (e.g., `OQ`, `A`, `FM`) that represents a family of related handles.

Namespaces are **inferred by sequential cardinality**: a prefix with N sequential members across M files, where N ≥ 3 and M ≥ 2, is a candidate namespace. Single-member prefixes at large numbers (SHA-256, AVX-512) are rejected.

Only labels in **confirmed namespaces** are treated as checkable handles. Labels matching the regex but in unconfirmed namespaces are ignored (no broken-reference errors for `GPT-2` or `UTF-8`).

Candidate namespaces are proposed to the user for confirmation. Confirmed and rejected namespaces are persisted in `anneal.toml`.

### §5 Graph — The Structure [KB-D5]

**Definition KB-D5 (Knowledge Graph).** A knowledge graph is a pair G = (H, E) where:

- H is a set of handles
- E ⊆ H × H × EdgeKind is a set of typed directed edges

```
EdgeKind =
  | Cites          — source mentions target (informational; no consistency check)
  | DependsOn      — source builds on target (consistency check: source state ≤ target state)
  | Supersedes     — source replaces target (target becomes terminal)
  | Verifies       — source proves or checks target
  | Discharges     — source consumes target (for linear handles)
```

Edge kind determines **what checks apply** [KB-P7]:

| Edge kind | Existence check | Consistency check | Impact propagation |
|---|---|---|---|
| Cites | ✓ target must exist | — | — |
| DependsOn | ✓ target must exist | ✓ source state ≤ target state | ✓ changes ripple |
| Supersedes | ✓ target must exist | — (target becomes terminal) | ✓ changes ripple |
| Verifies | ✓ target must exist | — | ✓ changes ripple |
| Discharges | ✓ target must exist | — | — |

**Cites** edges are the default. Most references in prose are citations — "see OQ-64 for future work." They create a link in the graph (for navigation and impact analysis) but don't impose consistency constraints. The formal model's convergence state is not affected by whether OQ-64 is open or resolved.

**DependsOn** edges are for genuine dependencies — "this section incorporates the analysis from synthesis/v17.md." The source's convergence state should not exceed the target's. A `formal` document depending on a `provisional` source is a warning.

Edge kind is **inferred from context** where possible (a file in `synthesis/` citing a file in `research-log/` is likely DependsOn) and can be made explicit in frontmatter or via keywords near the reference ("incorporates," "builds on," "extends" suggest DependsOn; "see also," "cf.," "related" suggest Cites).

#### §5.1 Graph Construction [KB-D6]

**Definition KB-D6 (Construction).** Graph construction proceeds by three parallel scans:

**File scan**: Walk directory tree rooted at `root`, skipping excluded directories [KB-D20]. Each `.md` file becomes a File handle. Non-markdown files are scanned for handle patterns in comments (e.g., Agda files with `-- Discharges: P-3`).

**Root inference** [KB-D20]: If no `root` is configured, the tool infers it:
1. If `.design/` exists in the current directory → `root = ".design"`
2. Else if `docs/` exists → `root = "docs"`
3. Else → `root = "."`

**Default exclusions** [KB-D20]: The following directories are always excluded from scanning, regardless of root:
- `.git/`, `.planning/`, `.anneal/` — infrastructure
- `target/`, `node_modules/`, `.build/` — build artifacts
- Any directory starting with `.` that isn't the root itself

Additional exclusions can be configured via `exclude` in `anneal.toml`.

**Scoping model**: anneal operates on the directory you point it at. A repo can contain multiple knowledge corpora (e.g., `.design/` for the project, `tools/anneal/.design/` for a sub-project). Each corpus has its own `anneal.toml` and is scanned independently. Run anneal from the directory containing the corpus, or set `root` explicitly.

**Frontmatter parse**: Extract YAML between `---` fences. The `status:` field becomes the declared convergence state. Other fields (`superseded-by:`, `updated:`, `note:`) contribute metadata.

**Content scan**: Five regex patterns extract edges and handles:

| Pattern | Discovers | Creates |
|---|---|---|
| `^#{1,6}\s` | Section boundaries | Section handles |
| `[A-Z][A-Z_]*-\d+` | Label references (in confirmed namespaces only) | Label handles + edges |
| `§\d+(\.\d+)*` | Section cross-references | Edges |
| Relative `.md` paths | File cross-references | Edges |
| `v\d+` in versioned context | Version references | Version handles + edges |

No markdown AST parsing. No NLP. Five regexes and a YAML parser.

### §6 Convergence Lattice [KB-D7]

**Definition KB-D7 (Convergence Lattice).** A convergence lattice is a bounded distributive lattice (L, ⊔, ⊓, ⊥, ⊤) representing the set of states a handle can be in, ordered by degree of settledness.

The lattice is a semiring under (⊔, ⊓, ⊥, ⊤):
- (⊔, ⊥) is a commutative monoid ✓ (join is parallel composition)
- (⊓, ⊤) is a monoid ✓ (meet is sequential composition)
- ⊓ distributes over ⊔ ✓
- ⊥ annihilates under ⊓ ✓

This algebraic structure is identical to TensorQTT's precision semiring (§17.5), generalized to an arbitrary bounded lattice [KB-F3]. It provides the ordering needed for consistency checks ("is state A above state B?") and the composition rules needed for future extension to full propagation.

#### §6.1 The Two-Element Lattice [KB-D8]

**Definition KB-D8 (Existence Lattice).** The simplest convergence lattice is {exists, missing} with exists > missing. At this level, the tool checks only reference integrity: does every referenced handle exist?

This is the **zero-config case**. Every corpus, regardless of conventions, gets reference checking.

#### §6.2 Confidence Lattice [KB-D9]

**Definition KB-D9 (Confidence Lattice).** When frontmatter `status:` fields are present, the tool infers a richer lattice from observed values.

Status values partition into two sets:
- **Active**: handles under maintenance (convergence states)
- **Terminal**: handles no longer maintained (fixed points — see §6.3)

The partition is inferred by convention (files in `history/`, `archive/`, `prior/` are terminal) and from observed values (statuses found only on old files are likely terminal). Override in config.

Within the active set, the ordering is either:
- Inferred from supersession chains (if `superseded-by:` fields exist)
- Declared in config
- Left as a flat set (all active states equivalent — still useful for staleness and obligation tracking, but pipeline flow analysis requires ordering)

#### §6.3 Terminal States and Decay [KB-D10]

**Definition KB-D10 (Terminal State).** A terminal state is a fixed point. Handles at terminal states:

- Do not generate diagnostics (they are not maintained)
- Only surface when referenced by active handles (staleness)
- Have their obligations automatically mooted

This models **healthy decay** [KB-P6]. The frozen archive can be enormous. Only the active frontier is checked.

#### §6.4 Freshness [KB-D11]

**Definition KB-D11 (Freshness).** Freshness is the age in days since a handle was last modified or its `updated:` frontmatter field was set.

Freshness thresholds are configurable. Default: warn at 30 days, error at 90 days.

#### §6.5 Convention Adoption [KB-D12]

**Definition KB-D12 (Adoption Threshold).** The tool only warns about missing structure when the convention is established. Specifically: warn about missing frontmatter in a file only when >50% of files in the same directory have frontmatter.

This prevents overwhelming a project that has just started adopting conventions. One file with `status: current` in a directory of 50 files does not trigger 49 warnings.

### §7 Local Checks [KB-D13]

**Definition KB-D13 (Local Checks).** Consistency is verified by five rules applied to each handle and its immediate edges. No transitive propagation.

**[KB-R1] Existence.** For every edge (source, target, _): target must resolve [KB-D3]. Failure is an error.

**[KB-R2] Staleness.** For every edge (source, target, _) where source is active and target is terminal [KB-D10]: warn that source references a superseded or archived handle.

**[KB-R3] Confidence gap.** For every DependsOn edge (source, target): if source's declared state is above target's declared state in the convergence lattice [KB-D9], warn. ("Your `formal` document depends on a `provisional` source.")

**[KB-R4] Linearity.** For every linear handle [KB-D15]: it must be discharged by exactly one Discharges edge. Zero = error (undischarged obligation). Multiple = info (affine — redundant but harmless). Creator at terminal state = automatically mooted.

**[KB-R5] Convention.** For files in directories where >50% of siblings have frontmatter [KB-D12]: warn about missing `status:` field.

These five rules cover reference checking [KB-R1], staleness detection [KB-R2], dependency consistency [KB-R3], obligation tracking [KB-R4], and incremental adoption [KB-R5].

#### §7.1 Theoretical Note

The five local checks are the single-hop case of full lattice propagation (Kleene iteration to fixpoint, as in TensorQTT §18). The propagation machinery remains valid as a theoretical foundation and is available as an extension point: if a project discovers use cases requiring transitive computation (e.g., deep dependency chains where confidence must compound), the lattice structure [KB-D7] supports it with guaranteed convergence [KB-T1] and confluence [KB-T2].

**Theorem KB-T1 (Convergence)** [KB-T1]. If full propagation is enabled, it terminates in at most |E| × height(L) steps and computes the unique least fixed point. Proof: transfer functions are monotone on a bounded lattice; apply Kleene's fixed-point theorem. (Identical to Large Corpus formal model Theorem 18.1.)

**Theorem KB-T2 (Confluence)** [KB-T2]. The fixed point is independent of iteration order. Proof: monotone operators on a join-semilattice are confluent (Hydroflow, OOPSLA 2023) [KB-F3].

### §8 Linearity [KB-D15]

**Definition KB-D15 (Linear Handle).** A handle in a namespace annotated `linear = true` must be discharged exactly once. Discharge is evidenced by an incoming edge of kind Discharges.

```
Obligation lifecycle:
  Created → Outstanding → Discharged    (normal: consumed by proof/implementation)
                ↓
              Mooted                     (creator reached terminal state)
```

Checked by rule KB-R4.

### §9 Impact Analysis [KB-D16]

**Definition KB-D16 (Impact).** Given a handle h, the **impact set** is the set of handles reachable by traversing reverse DependsOn, Supersedes, and Verifies edges from h.

Impact analysis answers: "if I change this handle, what else might need attention?" This is the question the arriving agent needs most — not "what's the global state" but "given what just changed, where should I look next?"

Impact is computed by reverse graph traversal. Supersedes chains are acyclic by definition. DependsOn and Verifies edges can form cycles in principle (A depends on B, B verifies A) — the traversal uses standard cycle detection (visited set) to terminate.

### §10 Convergence Tracking [KB-D17]

**Definition KB-D17 (Convergence Tracking).** The tool maintains an append-only history of graph snapshots in `.anneal/history.jsonl`. Each entry records:

```json
{
  "timestamp": "2026-03-27T14:30:00Z",
  "handles": { "total": 487, "active": 142, "frozen": 345 },
  "edges": { "total": 2031 },
  "states": { "raw": 12, "digested": 8, "decided": 18, "formal": 6, "verified": 4 },
  "obligations": { "outstanding": 0, "discharged": 18, "mooted": 12 },
  "diagnostics": { "errors": 0, "warnings": 3 },
  "namespaces": {
    "OQ": { "total": 69, "open": 44, "resolved": 19, "deferred": 6 }
  }
}
```

A snapshot is appended after each `anneal check` or `anneal status` run.

Snapshots are:
- **Append-only** (never modified)
- **Derived** (computed from the graph, which is computed from files)
- **Optional** (the tool works fully without them; `--history` just shows less)
- **Small** (~1KB per snapshot)

If `.anneal/history.jsonl` is deleted, nothing breaks. History restarts from the next run.

#### §10.1 Convergence Summary [KB-D18]

**Definition KB-D18 (Convergence Summary).** The tool computes a one-line convergence signal from the snapshot delta:

```
Convergence: advancing (OQ net -3, obligations clear, freshness 78%)
```

Three states:
- **Advancing**: more resolution than creation, obligations caught up, freshness improving
- **Holding**: balanced — system is maintaining but not progressing
- **Drifting**: more creation than resolution, obligations accumulating, freshness declining

The signal is heuristic, not definitive. It summarizes structural evidence of convergence — it cannot measure coherence (whether the ideas are *right*), only whether the process for refining them is healthy [KB-F1].

#### §10.2 Graph Diff [KB-D19]

**Definition KB-D19 (Graph Diff).** The diff between two snapshots (or between the current graph and the most recent snapshot) shows what changed in the knowledge structure:

- New handles created
- Handles whose state changed (promoted, superseded, archived)
- Obligations created or discharged
- Edges added or broken
- Namespace statistics delta

This tells the arriving agent what happened while it was "away" — the delta that no individual agent experienced but the system accumulated.

---

## Part III: Emergent Properties

The following capabilities emerge from the primitives (Handle, Graph, Lattice, Local Checks, Linearity, Impact, Convergence Tracking). They are not separate mechanisms.

### §11 Derived Capabilities

**[KB-E1] Reference checking** = rule KB-R1 applied over the existence lattice [KB-D8]. The zero-config baseline.

**[KB-E2] Staleness detection** = rule KB-R2. Active handles referencing terminal handles are flagged.

**[KB-E3] Dependency consistency** = rule KB-R3. A handle declaring high convergence state while depending on a lower-state source is flagged.

**[KB-E4] Pipeline tracking** = grouping handles by their state in the convergence lattice [KB-D9]. A **stall** is a state level with many handles and no outgoing DependsOn edges to the next level — the LEARN phase of the One Loop isn't firing [KB-F2].

**[KB-E5] Obligation tracking** = rule KB-R4 on linear namespaces [KB-D15].

**[KB-E6] Graceful decay** = terminal states as fixed points [KB-D10]. Frozen handles don't generate diagnostics and don't contribute to pipeline statistics.

**[KB-E7] Handle inference** = the content scanner discovering new namespaces by sequential cardinality [KB-D4].

**[KB-E8] Suggestions** = patterns in the graph that match known templates:
- Handles with no incoming edges → orphaned
- Recurring regex patterns not yet recognized as namespaces → candidate labels [KB-D4]
- State levels with high population and no outflow → pipeline stalls [KB-E4]
- All members of a namespace frozen for >N days → abandoned namespace
- Convention adoption sufficient for missing-frontmatter warnings [KB-D12]
- Labels frequently co-occurring across files → candidate concern group

Each suggestion is a graph query, not a content heuristic [KB-P5].

**[KB-E9] Change impact** = reverse traversal from a changed handle [KB-D16]. Answers "what else might need attention?"

**[KB-E10] Convergence monitoring** = snapshot delta analysis [KB-D18]. Structural evidence that the system is advancing, holding, or drifting.

---

## Part IV: CLI Surface

### §12 Commands

Eight commands. Each supports `--json` for agent consumption [KB-P8].

#### §12.1 `anneal check` [KB-C1]

Run local checks [KB-D13], report diagnostics.

```
anneal check                     # actionable diagnostics from active files
anneal check --include-terminal  # full diagnostics, including terminal files
anneal check --errors-only       # errors only (for pre-commit hooks)
anneal check --stale             # staleness diagnostics only [KB-E2]
anneal check --obligations       # obligation status only [KB-E5]
anneal check --suggest           # suggestions only [KB-E8]
```

Diagnostics follow compiler conventions:

```
error[E001]: broken reference
  → formal-model/v17.md:1847: label OQ-99 not found in confirmed namespace OQ

warn[W001]: stale reference
  → compiler/README.md:11: references "12-phase pipeline" (superseded by A-10)

warn[W002]: confidence gap
  → formal-model/v17.md:§14.3 (formal) depends on synthesis/v17.md (provisional)

info[I001]: pipeline stall
  → 6 files at status: raw with no synthesis downstream
```

Exit code: non-zero if errors exist. Integrates with `just check` and pre-commit hooks. Appends a snapshot to `.anneal/history.jsonl`.

#### §12.2 `anneal get <handle>` [KB-C2]

Resolve any handle [KB-D3]. Return content, state, and graph context.

```
anneal get OQ-64                          # label: definition + state
anneal get formal-model/v17.md            # file: frontmatter + state
anneal get formal-model/v17.md:§14.3      # section: content range + state
anneal get P-3                            # obligation: status + creator + discharger
anneal get OQ-64 --refs                   # + reference graph (incoming + outgoing)
anneal get OQ-64 --context                # compressed agent briefing (~200 tokens)
anneal get OQ-64 --trace                  # full lineage (created by, blocks, blocked by)
```

One command, any handle type. The handle kind [KB-D2] determines what "content" means.

#### §12.3 `anneal find <query>` [KB-C3]

Search handle referents. Results filtered to active handles by default.

```
anneal find "stability"                   # full-text search across active handles
anneal find "stability" --all             # include frozen handles
anneal find --status=current              # filter by convergence state
anneal find --namespace=OQ                # all handles in a namespace
anneal find --namespace=OQ --status=open  # composed filters
```

Search is full-text in v1. The interface accommodates a future vector search backend without changing the command surface [KB-OQ3].

#### §12.4 `anneal status` [KB-C4]

Dashboard. Graph statistics, pipeline state [KB-E4], convergence summary [KB-D18], top suggestions [KB-E8].

```
anneal status
  Scanned: 265 files, 487 handles, 2031 edges
  Active: 142 handles | Frozen: 345 handles
  Pipeline: 12 raw → 8 digested → 18 decided → 6 formal → 4 verified
  Obligations: 6/6 discharged, 12 mooted
  Diagnostics: 0 errors, 3 warnings
  Convergence: advancing (OQ net -3, obligations clear, freshness 78%)
  Suggestions: 2 (run anneal check --suggest)
```

Appends a snapshot to `.anneal/history.jsonl`.

#### §12.5 `anneal map` [KB-C5]

Render the knowledge graph.

```
anneal map                                # full active graph
anneal map --concern="cost model"         # subgraph for a concern group
anneal map --around=OQ-64 --depth=2       # neighborhood of a handle
anneal map --format=dot                   # graphviz output
```

#### §12.6 `anneal init` [KB-C6]

Save inferred structure as `anneal.toml` for customization.

```
anneal init                      # infer coloring from files, write anneal.toml
anneal init --dry-run             # show what would be written
```

The generated config contains inferred active/terminal partition, confirmed/rejected namespaces, and suggested concern groups. The user reviews and edits.

#### §12.7 `anneal impact <handle>` [KB-C7]

Show what's affected if a handle changes [KB-D16].

```
anneal impact formal-model/v17.md
  Directly affected (depend on this):
    CLAUDE.md:42
    .design/README.md:58
    compiler/README.md:67-72
    implementation/2026-03-26-type-safe-pipeline.md

  Indirectly affected (depend on the above):
    (none — all leaf documents)
```

Computed by reverse graph traversal over DependsOn, Supersedes, and Verifies edges.

#### §12.8 `anneal diff [ref]` [KB-C8]

Graph-level changes since a reference point [KB-D19]. Default: since last snapshot.

```
anneal diff                      # since last snapshot
anneal diff --days=7             # since 7 days ago
anneal diff HEAD~3               # since 3 git commits ago (reads files at that ref)
```

```
Since last session:
  New handles: OQ-64, OQ-65, OQ-66 (3 open questions)
  State changes: formal-model/v17.md: current → verified
  Discharged: P-1 through P-6
  New edges: 6 from formal-model/v17.md
  Stale: compiler/README.md now references superseded content
```

---

## Part V: Configuration

### §13 The Coloring Book [KB-D14]

**Definition KB-D14 (Coloring).** A coloring is the set of project-specific parameters that instantiate the kernel for a particular corpus:

- Which directories to scan
- Which status values are active vs terminal
- Which handle namespaces are confirmed
- Which namespaces are linear
- What freshness thresholds to apply
- What concern groups exist
- What definition files map to which namespaces

The coloring is expressed in `anneal.toml`. All fields are optional [KB-P3]. An absent `anneal.toml` is a valid coloring (the zero-config case — existence lattice only).

```toml
# anneal.toml — entirely optional

root = ".design"
exclude = ["archive/research"]  # additional dirs to skip (beyond defaults)

[convergence]
active = ["raw", "digested", "decided", "formal", "verified",
          "provisional", "exploratory", "reference", "decision",
          "current", "stable", "active", "authoritative",
          "draft", "proposal", "living"]
terminal = ["superseded", "archived", "historical",
            "incorporated", "complete", "retired"]
# Optional: ordering for pipeline flow analysis
# If omitted, active states are treated as a flat set
ordering = ["raw", "digested", "decided", "formal", "verified"]

[handles]
confirmed = ["OQ", "D", "SR", "DG", "A", "P", "FM", "TQ",
             "AL", "C", "DEF", "DT", "BR", "TO", "RQ"]
rejected = ["SHA", "AVX", "GPT", "UTF", "GPL", "CRC"]

[handles.OQ]
definition_file = "OPEN-QUESTIONS.md"

[handles.P]
linear = true

[freshness]
warn = 30
error = 90

[concerns]
"cost model" = ["A-7", "A-8", "FM-015", "OQ-34", "DG-42"]
"stability" = ["FM-023", "OQ-66", "OQ-67", "SR-008"]
```

### §13.1 Colorings of Different Projects

| Project | Convergence states | Namespaces | Linear | Concern groups |
|---|---|---|---|---|
| Large Corpus .design/ | 16 active + 9 terminal | 15 (OQ, FM, A, ...) | P | cost model, stability, ... |
| Startup docs/ | draft, approved, archived | RFC, ADR | ADR | API, auth, deployment |
| Research group | sketch, submitted, published | CLAIM, METHOD | CLAIM | per-paper topics |
| Solo project | (none — existence lattice) | (none) | (none) | (none) |

---

## Part VI: Implementation

### §14 Architecture

```
                     ┌───────────────────────┐
                     │    CLI (8 commands)    │  §12
                     └───────────┬───────────┘
                                 │ queries
                     ┌───────────┴───────────┐
                     │    Checked Graph       │  §5 + §7
                     └───────────┬───────────┘
                                 │
         ┌───────────────────────┼───────────────────────┐
         │            │          │          │             │
   ┌─────┴─────┐ ┌───┴───┐ ┌───┴───┐ ┌───┴───┐ ┌───────┴───────┐
   │  Checker   │ │Resolve│ │Impact │ │Converge│ │   Snapshots   │
   │  5 rules   │ │       │ │       │ │ track  │ │  .anneal/history  │
   │  §7        │ │  §4.2 │ │  §9   │ │  §10   │ │    §10       │
   └─────┬─────┘ └───┬───┘ └───┬───┘ └───┬───┘ └───────┬───────┘
         │            │         │         │             │
         └────────────┴─────────┴─────────┴─────────────┘
                                 │
                     ┌───────────┴───────────┐
                     │    Raw Graph           │  §5.1
                     └───────────┬───────────┘
                                 │
              ┌──────────────────┼──────────────────┐
              │                  │                  │
        ┌─────┴─────┐    ┌──────┴──────┐    ┌──────┴──────┐
        │  File Scan │    │  Frontmatter │    │  Content    │
        │            │    │  Parse       │    │  Scan       │
        └─────┬─────┘    └──────┬──────┘    └──────┬──────┘
              │                  │                  │
              └──────────────────┼──────────────────┘
                                 │
                     ┌───────────┴───────────┐
                     │    Filesystem          │
                     │    + anneal.toml           │
                     └───────────────────────┘
```

Data flow: Filesystem → (scan + parse + infer) → Raw Graph → (resolve + check + impact + snapshot) → Checked Graph → (query) → CLI output.

### §15 Rust Crate Structure

```
anneal/
  src/
    handle.rs       # Handle, HandleKind, HandleId          §4
    graph.rs        # Graph, Edge, EdgeKind, construction   §5
    lattice.rs      # Lattice trait, convergence states     §6
    checks.rs       # Five local check rules                §7
    linear.rs       # Obligation lifecycle                  §8
    impact.rs       # Reverse graph traversal               §9
    snapshot.rs     # History, convergence summary, diff    §10
    parse.rs        # Frontmatter + regex scanning          §5.1
    resolve.rs      # Handle resolution                     §4.2
    config.rs       # anneal.toml parsing + inference       §13
    cli.rs          # Eight commands + --json               §12
    main.rs         # Entry point
  Cargo.toml
  .design/
    anneal-spec.md  # This document
```

### §15.1 Dependencies

```toml
[dependencies]
anyhow = "1"                   # error handling with context
clap = { version = "4", features = ["derive"] }  # CLI with derive macros
serde = { version = "1", features = ["derive"] } # serialization framework
serde_json = "1"               # JSON output (--json) + JSONL snapshots
serde_yaml_ng = "0.10"         # YAML frontmatter parsing (maintained fork)
toml = "0.8"                   # anneal.toml config
regex = "1"                    # RegexSet for multi-pattern scanning
walkdir = "2"                  # recursive directory traversal
camino = "1"                   # UTF-8 paths throughout
chrono = { version = "0.4", default-features = false, features = ["clock"] }
```

Estimated clean build: ~10s. No proc macros beyond clap and serde derive.

#### §15.2 What We Hand-Roll Instead of Importing

| Component | Lines | Instead of | Why |
|---|---|---|---|
| Directed graph | ~135 | `petgraph` | Dual adjacency lists (`fwd` + `rev` `Vec<Vec<Edge>>`) with `NodeId(u32)`. Fits existing arena patterns from sample-compiler. Only need forward/reverse traversal, cycle detection, reachability, toposort — all textbook, each <25 lines. petgraph would add 1.5s compile time for 5% of its surface. |
| Frontmatter split | ~15 | `gray_matter` | Split on `---` fences, pass YAML to `serde_yaml_ng`. The split is trivial; the library adds a dependency for string splitting. |
| JSONL append/read | ~30 | `jsonl`, `serde-jsonlines` | `serde_json::to_vec` + `\n` + single `write_all` to `O_APPEND` file. Read via `BufReader::lines()` + `serde_json::from_str`, skip unparseable lines (handles truncation from interrupted writes). No `BufWriter` needed — one write per invocation. No `fsync` — data is derived and re-computable. |

#### §15.3 Key Implementation Patterns

**Multi-pattern scanning.** `RegexSet` checks all 5 content patterns in a single pass per line. Only lines that match trigger individual `Regex` extraction. Most lines match 0 patterns — the fast path is one automaton pass. Compiled regexes stored in `std::sync::LazyLock` (stable since Rust 1.80).

```
RegexSet (single pass) → matched? → individual Regex (extract captures)
                       → no match → skip (fast path)
```

**Dual output.** Every command returns a struct that is both `Serialize` (JSON via `--json`) and implements a `print_human()` method. Global `--json` flag via `#[arg(global = true)]` in clap derive — works at any position (`anneal --json check` and `anneal check --json` are equivalent).

```rust
trait CommandOutput: Serialize {
    fn print_human(&self, w: &mut dyn Write) -> io::Result<()>;
}
```

**All-optional config.** `#[serde(default, deny_unknown_fields)]` on every config struct. All fields have concrete types with `Default` impls — no `Option<T>` wrapping. An empty `anneal.toml` deserializes to all defaults. `deny_unknown_fields` catches config typos.

**Graph construction.** `NodeId(u32)` indices into `Vec<Node>`. Dual adjacency lists for O(1) forward and reverse traversal. Edge kinds stored per-edge. Typed traversal methods (`edges_by_kind(id, EdgeKind)`) as first-class API rather than post-hoc filtering.

**Snapshot append.** `serde_json::to_vec` serializes the snapshot to a buffer, push `b'\n'`, single `write_all` to `O_APPEND` file. Practically atomic for ~1KB entries on local filesystems. On read, `BufReader::lines()` with `filter_map` — warn and skip unparseable lines (handles mid-write truncation gracefully).

### §16 Integration Points

- **Pre-commit hook**: `anneal check --errors-only` in `.git/hooks/pre-commit`
- **Just target**: `just check-design` calls `anneal check`
- **Agent session start**: `anneal status --json --compact` injected into context
- **MCP server**: future extension wrapping commands as tools [KB-OQ4]

---

## Part VII: Open Questions

### §17 Unresolved Design Questions

**[KB-OQ1] Full propagation.** The lattice supports Kleene iteration for transitive grade computation [KB-T1]. When does a project need this? Likely when dependency chains exceed 3-4 hops and confidence must compound. Monitor whether any real corpus requires it before implementing.

**[KB-OQ2] Section reference ambiguity.** Bare `§14` references are ambiguous across documents. Current decision: resolve within current file; qualify cross-document references as `formal-model/v17:§14.3`. Unresolvable section references are warnings, not errors.

**[KB-OQ3] Semantic search.** `anneal find` uses full-text matching in v1. A vector search backend (local GGUF model, following QMD's approach) would enable semantic queries. The interface accommodates this without changing the command surface.

**[KB-OQ4] MCP server.** Wrapping the eight commands as MCP tools. Thin wrapper — same graph, same queries, different transport. Build once the CLI proves useful.

**[KB-OQ5] Non-markdown corpora.** Source code comments, TOML/YAML config, structured data could contain handles. Current decision: markdown primary, with optional comment scanning for configured patterns.

**[KB-OQ6] Self-checking.** Can anneal check its own spec? Desirable for bootstrap validation.

**[KB-OQ7] Edge kind inference.** Inferring DependsOn vs Cites from context (directory relationships, proximity keywords) is heuristic. How accurate does it need to be? False Cites (should be DependsOn) means missed consistency warnings. False DependsOn (should be Cites) means noisy warnings. Probably start with Cites as default and let users override.

**[KB-OQ8] Coherence measurement.** Convergence tracking captures structural signals (resolution rate, obligation discharge, freshness) but not coherence (whether ideas are *right* and *hang together*). Session orientation speed and decision stability are proxies. Can the tool measure these? Possibly from snapshot deltas and supersession patterns.

---

## Part VIII: Labels

### KB-F (Foundations)

| Label | Description | §Ref |
|---|---|---|
| KB-F1 | Context physics — convergence as potential energy landscape | §2.1 |
| KB-F2 | The One Loop — refinement pipeline dynamics | §2.2 |
| KB-F3 | Graded type systems — TensorQTT lattice algebra | §2.3 |
| KB-F4 | Linear logic — obligation discharge | §2.4 |
| KB-F5 | Coloring book principle — kernel/coloring split | §2.5 |

### KB-P (Principles)

| Label | Description | §Ref |
|---|---|---|
| KB-P1 | Files are truth (computed graph, no storage) | §3 |
| KB-P2 | Everything is a handle | §3 |
| KB-P3 | Inference first, config second | §3 |
| KB-P4 | Capabilities over process | §3 |
| KB-P5 | Suggestions surface patterns | §3 |
| KB-P6 | Decay is healthy | §3 |
| KB-P7 | Local checks over global propagation | §3 |
| KB-P8 | Machine-readable by default | §3 |

### KB-D (Definitions)

| Label | Description | §Ref |
|---|---|---|
| KB-D1 | Handle | §4 |
| KB-D2 | Handle Kind | §4.1 |
| KB-D3 | Handle Resolution | §4.2 |
| KB-D4 | Handle Namespace | §4.3 |
| KB-D5 | Knowledge Graph | §5 |
| KB-D6 | Graph Construction | §5.1 |
| KB-D7 | Convergence Lattice | §6 |
| KB-D8 | Existence Lattice (two-element) | §6.1 |
| KB-D9 | Confidence Lattice | §6.2 |
| KB-D10 | Terminal State | §6.3 |
| KB-D11 | Freshness | §6.4 |
| KB-D12 | Convention Adoption Threshold | §6.5 |
| KB-D13 | Local Checks (five rules) | §7 |
| KB-D14 | Coloring | §13 |
| KB-D15 | Linear Handle | §8 |
| KB-D16 | Impact Analysis | §9 |
| KB-D17 | Convergence Tracking | §10 |
| KB-D18 | Convergence Summary | §10.1 |
| KB-D19 | Graph Diff | §10.2 |
| KB-D20 | Root Inference and Exclusions | §5.1 |

### KB-R (Rules)

| Label | Description | §Ref |
|---|---|---|
| KB-R1 | Existence check | §7 |
| KB-R2 | Staleness check | §7 |
| KB-R3 | Confidence gap check | §7 |
| KB-R4 | Linearity check | §7 |
| KB-R5 | Convention adoption check | §7 |

### KB-T (Theorems)

| Label | Description | §Ref |
|---|---|---|
| KB-T1 | Propagation convergence (Kleene) — extension point | §7.1 |
| KB-T2 | Propagation confluence (Hydroflow) — extension point | §7.1 |

### KB-E (Emergent Properties)

| Label | Description | §Ref |
|---|---|---|
| KB-E1 | Reference checking | §11 |
| KB-E2 | Staleness detection | §11 |
| KB-E3 | Dependency consistency | §11 |
| KB-E4 | Pipeline tracking | §11 |
| KB-E5 | Obligation tracking | §11 |
| KB-E6 | Graceful decay | §11 |
| KB-E7 | Handle inference | §11 |
| KB-E8 | Suggestions | §11 |
| KB-E9 | Change impact | §11 |
| KB-E10 | Convergence monitoring | §11 |

### KB-C (Commands)

| Label | Description | §Ref |
|---|---|---|
| KB-C1 | `anneal check` | §12.1 |
| KB-C2 | `anneal get` | §12.2 |
| KB-C3 | `anneal find` | §12.3 |
| KB-C4 | `anneal status` | §12.4 |
| KB-C5 | `anneal map` | §12.5 |
| KB-C6 | `anneal init` | §12.6 |
| KB-C7 | `anneal impact` | §12.7 |
| KB-C8 | `anneal diff` | §12.8 |

### KB-OQ (Open Questions)

| Label | Description | §Ref |
|---|---|---|
| KB-OQ1 | Full propagation — when is it needed? | §17 |
| KB-OQ2 | Section reference ambiguity | §17 |
| KB-OQ3 | Semantic search | §17 |
| KB-OQ4 | MCP server | §17 |
| KB-OQ5 | Non-markdown corpora | §17 |
| KB-OQ6 | Self-checking | §17 |
| KB-OQ7 | Edge kind inference accuracy | §17 |
| KB-OQ8 | Coherence measurement | §17 |

---

## References

### Internal

| Source | What it contributes |
|---|---|
| Host Corpus system theory (§1-4, C-10, DY-4, DY-5) | The One Loop, crystallization levels, coloring book principle, context physics framing |
| Large Corpus formal model v17 (§17-18, Theorem 18.1) | Lattice algebra, propagation algorithm, convergence proof |
| Large Corpus formal model v17 (§17.2) | Usage semiring {0, 1, ω} — basis for linearity |
| Large Corpus formal model v17 (§14.3) | Three computational atoms — structural parallel (LinMap↔Refine, Table↔Verify, Gather↔Connect) showing the lattice algebra applies to knowledge operations |
| Large Corpus .design/LABELS.md | Label system design — prefix taxonomy, cross-referencing, concern groups |
| Large Corpus .design/README.md | Front-matter conventions, status lifecycle, document versioning |
| Large Corpus .design/OPEN-QUESTIONS.md | Open question tracking pattern |
| Large Corpus v17 synthesis | Proof obligation pattern (P-1 through P-6) as linearity exemplar |
| Large Corpus implementation architecture | Four-layer architecture as coloring example |
| Mind-map skill | Numbered nodes with inline cross-references — graph rendering approach |

### External

| Source | What it contributes |
|---|---|
| napkin (github.com/Michaelliv/napkin) | Progressive disclosure for agents; motivation for agent-first design |
| QMD (github.com/jamesrisberg/qmd) | Hybrid search; hierarchical context; MCP server pattern |
| jj (github.com/martinvonz/jj) | Change identity separate from snapshot identity — handles as persistent idea IDs |
| Kleene fixed-point theorem | Convergence guarantee for monotone iteration (extension point) |
| Girard (1987), Linear Logic | Substructural resource discipline for obligations |
| Hydroflow (OOPSLA 2023) | Lattice-monotone operators are confluent |
| Sussman/Radul, Propagator model | Partial information cells accumulate monotonically on lattices |
| Research graph: propagator claims | Monotonic accumulation, parallel safety, convergence |
| Research graph: lattice confluence | Join-semilattice monotone operators are evaluation-order independent |
| Research graph: evolvability | Trading provability for extensibility in real systems (Sussman) |
