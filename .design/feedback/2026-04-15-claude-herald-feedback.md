// Needs frontmatter

## Feedback

⏺ Let me think about this properly. I used anneal extensively this session alongside manual reading, and the
gaps are clearer to me now than they were five minutes ago.

Anneal's purpose (from the skill): orient in a markdown knowledge corpus, recover relevant context quickly,
validate structural assumptions. It treats a corpus as a convergence system — settledness, obligations, drift.

Here's what I actually needed, organized by whether anneal should own it:

---

Things anneal SHOULD do but doesn't

1. Rooted dependency visualization

I needed "what feeds into this document?" as a tree. Had to call anneal get --refs on 5+ files and assemble
mentally. The edges exist in the graph — anneal just can't render them as a rooted traversal.

# What I wanted

anneal map --rooted-at=impl-plan.md --upstream --depth=3

# What I had to do

anneal get impl-plan.md --refs # flat list
anneal get arch-synthesis.md --refs # flat list
anneal get herald-arch.md --refs # flat list

# ... manually trace and diagram

This is squarely in anneal's domain — it's a graph question about document relationships. anneal map --around
exists but gives neighborhood, not directed ancestry.

2. Purpose/summary extraction from frontmatter

Every spec has a purpose: field in frontmatter. Anneal indexes it but doesn't expose it. --context gives a
snippet (first line of body), not the purpose. When orienting on 15 specs, I needed purposes, not snippets.

# What I wanted

anneal list --status=active --show=purpose

# What I had to do

Read each file's first 30 lines, 15 times

This is anneal's job — it already parses frontmatter. Exposing purpose: alongside status in list/query output
is a small surface addition.

3. Status pipeline semantics documentation

I couldn't tell what stable vs active vs final vs incorporated vs decision meant operationally. The
architecture synthesis was stable but was the governing entry point. The implementation plan was incorporated
but had unfinished work. I had to read the documents to understand whether their status was correct.

Anneal should either:

- Document the pipeline semantics in anneal help status (what does each level MEAN for "should I still work
  from this?")
- Or expose the pipeline configuration from anneal.toml so I can see which statuses are terminal vs active

# What I wanted

anneal help pipeline

# → "active: work is ongoing. stable: content settled, may still govern work.

# final: delivered, no changes expected. incorporated: absorbed into another doc."

4. Obligation resolution tracking

I resolved COMP-OQ-1/2/3 by editing LABELS.md and OPEN-QUESTIONS.md text, but anneal still showed them as E002
errors because the Discharges edge wasn't present. The mismatch between "the text says resolved" and "the
graph says outstanding" is confusing.

Anneal should either:

- Recognize status text changes in obligation tables (fragile, probably wrong)
- Or provide guidance on HOW to discharge: anneal explain obligation COMP-OQ-1 → "To discharge, add
  discharges: [COMP-OQ-1] to the frontmatter of the resolving document"

I never figured out the right way to wire the discharge edge. The error kept showing even after I'd
substantively resolved the question.

5. "What's active and why?" — filtered inventory

I needed "show me all active documents with their purpose" to understand what's still governing work. anneal
find "" --status=active doesn't exist (find is text search). anneal query handles --kind file doesn't filter
by status.

# What I wanted

anneal list --active --purposes

# → active files with purpose: text, sorted by dependency depth

# What I had to do

anneal status --json --compact # just counts
anneal check --active-only # just problems

# Then manually read each file

6. Convergence delta since a point in time

anneal diff exists but I didn't know my baseline. When entering a session, the question is "what moved since
the last session?" — which obligations were discharged, which statuses changed, which new documents appeared.

# What I wanted

anneal diff --days=7

# → "3 files changed status. 2 obligations discharged. 1 new file."

# This may already work — I didn't try it because I didn't know

# if snapshots existed from prior sessions

7. Cross-reference from obligation to discharging document

When I saw E002: COMP-OQ-1 has no Discharges edge, I wanted to know "what document SHOULD discharge this?" The
obligation lives in the compression spec, and the resolution happened in code (the Connectors refactor).
Anneal could suggest candidates based on edge proximity or recent changes.

# What I wanted

anneal explain obligation COMP-OQ-1

# → "Defined in: compression-spec.md

# Context: Should Sources become the general connector abstraction?

# Candidate dischargers: (none found)

# To discharge: add discharges: [COMP-OQ-1] to resolving document frontmatter"

8. Batch status queries

I needed to check the status of 5 specific files. Had to call anneal get 5 times. A batch query would help.

# What I wanted

anneal get arch-synthesis.md impl-plan.md compression-spec.md --status-only

# → arch-synthesis: active, impl-plan: active, compression-spec: complete

# What I had to do

anneal get file1 --context
anneal get file2 --context
anneal get file3 --context

---

Things anneal should NOT do (that I did manually, correctly)

9. Summarize document content — I read specs to understand their substance. Anneal should expose the purpose:
   frontmatter field but should NOT try to summarize body content. That's an LLM job, not a structural tool's
   job.

10. Assess whether a spec's work is "done" — I compared spec descriptions against live code to determine
    delivery status. This requires code understanding. Anneal tracks status as declared in frontmatter; it
    shouldn't infer delivery from external systems.

11. Recommend next work — I determined "Phase B is next" by reading the build order section of the
    implementation plan. Anneal could surface active documents sorted by dependency depth (structural), but
    shouldn't reason about project priorities (semantic).

12. Generate diagrams — The ASCII diagram I built was a synthesis of structural knowledge (edges) and semantic
    knowledge (purposes, delivery status). Anneal could provide the raw data for a tree visualization, but the
    annotated diagram requires understanding content, which is the agent's job.

13. Validate spec claims against code — When I checked whether "all phases shipped," I had to read code.
    Anneal tracks corpus convergence, not code-corpus alignment. A separate tool (or anneal extension) could
    cross-reference bd issues or git history, but that's outside the corpus-structural scope.

---

Summary: the gap pattern

The common theme is anneal has the data but lacks the views. The graph, statuses, edges, obligations, and
frontmatter are all indexed. What's missing are the traversal and projection commands that turn that indexed
data into the oriented understanding I needed:

- Tree traversal (rooted upstream/downstream walks)
- Frontmatter field projection (purpose, date, references in list output)
- Pipeline semantics documentation
- Obligation lifecycle guidance (how to discharge, not just that it's undischarged)
- Batch operations on known handles
- Time-windowed convergence deltas
