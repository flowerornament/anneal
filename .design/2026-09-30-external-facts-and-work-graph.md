---
status: draft
date: 2026-09-30
authors: [claude, morgan]
purpose: >
  Design note toward a spec. Proposes one general external-facts source so that
  any task tracker, and later any other tool that emits handle-shaped facts,
  plugs into anneal through a small exporter rather than a Rust adapter; and a
  work vocabulary that derives which issues are live, ready and orphaned from
  asserted facts. Records what the prototypes measured and what is still open.
depends-on:
  - 2026-05-13-corpus-runtime.md
---

# External facts and the derived work graph - 2026-09-30

Tracked by `anneal-iiqt`. This note is a draft: it states a direction and its
evidence, and names what must be learned before any of it is locked.

## 1. Problem

A tracker stores status. A stored status is true on the day it is written and
decays afterwards: when a specification supersedes an arc, the arc's open
issues stay open and nobody has judged them. Murail measured the result in its
own tracker: 908 open issues, median age 66 days, 734 of 1,870 close reasons
blank.

The remedy is to store only what someone asserted and derive the rest on each
run. Anneal already works this way for documents. The question here is how
work items enter, and how to keep that entry general: the tracker in use today
is not controlled by this project and may be replaced.

## 2. Evidence so far

A prototype mirrors `bd export` into one Markdown file per issue and derives
the work graph with fifteen project rules. No anneal code changed.

| corpus | issues | mirror | query | result |
|---|---:|---:|---:|---|
| anneal | 474 | 0.09 s | 0.19 s | 31 ready; the tracker's own blocked count matches |
| murail | 2,796 | 1.8 s | 2.5 to 4.4 s | 348 ready; extraction dominates |

The prototype is in this repository (`scripts/bd-mirror.py`, the `work_*`
rules and the `work-ready` and `work-orphans` verbs in `anneal.dl`). Running
it exposed four defects that are properties of the mirror, not of the idea:

1. **Unknown reads as zero.** With no mirror present, `work-ready` returns no
   rows. Generated files cannot say that the tracker was not read.
2. **Work items leak into the document lifecycle.** Mirrored issues are
   `kind = "file"` handles with statuses `open` and `closed`, so lifecycle
   diagnostics fire on them and corpus counts mix issues with documents.
3. **Extraction cost is Markdown parsing.** Murail's seconds are spent parsing
   generated files whose content was structured to begin with.
4. **References are strings.** A bead id in frontmatter is unmodeled metadata,
   so a mistyped id is not a broken reference.

Each of the four is fixed by a source that ingests facts directly.

## 3. Architecture

```text
 tracker        exporter              interchange            source          vocabulary      verbs
 bd        ──►  small script,    ──►  NDJSON of facts,  ──►  one generic ──► work.dl    ──►  work-ready
 GitHub         per tracker,          versioned,             external-       (standard       work-orphans
 Herald         outside anneal        self-describing        facts source    package)
```

Generality lives in two places. The **interchange** makes a new tracker cheap:
an exporter in any language, no anneal release. The **vocabulary** makes
trackers interchangeable: rules see a normalized work item and never a
tracker's own field names.

There is one source, not one per tracker. It is also not specific to work: it
ingests the stored relations of CR-D8 (`handle`, `edge`, `meta`) from a file.
Work items are a profile of that format, not a feature of the source. The same
seam serves any tool whose output is handle-shaped, and it is where
adapter-declared relations would arrive if `anneal-rxvy` proceeds.

The fact types already derive `Deserialize`; the runtime has an NDJSON writer
and no reader. The source is a reader plus the `Source` contract.

## 4. Interchange, version 0

One file, one JSON object per line. The first line is a header; the rest are
facts.

```text
{"anneal_facts": 0, "source": "work", "generated_at": "2026-09-30T18:00:00Z",
 "origin": "bd export", "origin_revision": "…", "complete": true}
{"handle": {"id": "work:anneal-qao9", "kind": "work", "summary": "…", "date": "2026-08-12"}}
{"meta":   {"handle": "work:anneal-qao9", "key": "work.state", "value": "active"}}
{"edge":   {"from": "work:anneal-qao9.1", "to": "work:anneal-qao9", "kind": "ChildOf"}}
```

- `source` names the generation; a refresh replaces that source's facts
  atomically, as any source does.
- `generated_at`, `origin` and `origin_revision` are the stamp every answer
  can cite. A reader can tell how old the facts are.
- `complete: false` marks a partial export. Rules that conclude from absence
  must not run over a partial export.
- A missing or unreadable file makes the source **unavailable**, reported the
  way repository operations report availability (CR-D111). It is never an
  empty source.

Anneal reads the file. It does not run the exporter: a command declared in
project configuration would execute with the caller's authority, which needs
a capability decision this note does not make. Freshness belongs to whatever
invokes the exporter.

## 5. The work profile

A work item is a handle of `kind = "work"` with these facts:

| fact | values | note |
|---|---|---|
| `work.state` | `open`, `active`, `done`, `dropped` | normalized; rules use only this |
| `work.native_state` | the tracker's own word | kept so nothing is lost |
| `work.priority` | integer, lower is more urgent | absent when the tracker has none |
| `work.updated` | ISO date | last change to the item in the tracker |
| `work.close_reason` | text | absent when open |
| edge `ChildOf` | item to parent item | at most one |
| edge `BlockedBy` | item to item | scheduling, not a logical premise |
| edge `DiscoveredFrom` | item to item | provenance of review findings |

`kind = "work"` keeps work items out of the document lifecycle: their states
are not lattice statuses and document diagnostics do not apply to them.

The profile was checked against three trackers:

| | bd | GitHub Issues | Herald `WorkItem` |
|---|---|---|---|
| id | `anneal-qao9` | `owner/repo#123` | UUID |
| state | open, in_progress, closed, deferred | open, closed + `state_reason` | `state` atom |
| done vs dropped | close reason | `completed` vs `not_planned` | not modeled yet |
| parent | `parent-child` dependency | sub-issue | none yet |
| blocks | `blocks` dependency | blocked-by relation | none yet |
| priority | 0 to 4 | none natively | none |

Nothing in GitHub or Herald requires a field the profile lacks. Both lack
fields bd has; the profile carries those as absent, which rules must treat as
unknown. Herald's work tracking is a declared read shape whose content type is
not built, so it constrains the profile only from below.

## 6. Derivation

Base facts are the work profile and four assertions written as scalar lists in
the frontmatter of the document that makes the judgment:

```text
supersedes-work: [<root item>]     the root whose children need judging
moves:   [<item>]                  continues under the successor; stays live
absorbs: [<item>]                  replaced by successor work; not live
dies:    [<item>]                  no longer needed; not live
```

Rules, never written back to the tracker:

```text
live(b)     ← state(b) ∈ {open, active}, not absorbed(b), not dead(b)
suspect(b)  ← b is under a root that a document supersedes
orphan(b)   ← live(b), suspect(b), not moved(b)
ready(b)    ← live(b), no live item blocks b
```

Supersession flags an item and never removes it. `live` depends only on
asserted facts, so an item stays workable until someone records a verdict.
An orphan is a request for judgment, reported and not gated.

`done` remains a stored fact: finishing work is something a person knows. Only
*moot* is derived.

## 7. Contract

Adopted from murail-1b's critique of the first proposal.

- Answers are recomputed when asked. They are fresh at three moments: session
  start, each invocation, and the landing gate. There is no watcher.
- Every work answer can cite what it read: the tracker stamp from the header
  and the corpus revision.
- An unavailable source yields unknown, never zero.
- Acceptance: change an input and see dependents change; delete it and see
  unsupported conclusions retract; keep a conclusion that has a second
  support; restore the input and reproduce the earlier answer; change only a
  rule and recompute; fail acquisition and show unknown; never publish an
  answer mixing two generations.

## 8. Open

1. **References.** Assertions name items by bare id. Whether those resolve as
   edges, so that a mistyped id is a broken reference, depends on how `work:`
   handles meet label resolution.
2. **Liveness is per workspace until a document lands; the tracker is global.**
   One workspace can declare an item dead while another holds it in progress.
3. **Who owns orphans.** Without an owner at the moment of supersession the
   orphan count becomes a second backlog.
4. **Verb surface.** Which answers ship as verbs, and whether orphans get a
   diagnostic code.
5. **Evidence of activity.** Session history could show whether an orphan is
   still being worked. It is machine-local and may only ever be advisory.
6. **Declared relations.** Facts beyond CR-D8 need `anneal-rxvy`.

## 9. Before this becomes a spec

- Murail's sweep lands about 500 real verdicts in the assertion shape. Three
  verdicts either hold or a fourth appears.
- Two weeks of use here and in murail show whether orphans get judged.
- The source is prototyped and measured against the mirror on murail's corpus.
- A GitHub exporter is written against version 0 and run on a real repository.
