---
status: locked
date: 2026-08-14
authors: [codex, claude, morgan]
purpose: >
  Prevent jj added workspaces from inheriting Git operations from an unrelated
  ancestor repository. Defines one nearest-VCS authority, four independent
  repository-operation capabilities, and the user-visible disclosure contract
  for operations that have not earned an answer. Real jj history semantics are
  deliberately deferred.
depends-on:
  - 2026-05-13-corpus-runtime.md
---

# Repository operation capabilities - 2026-08-14

## 1. The measured failure

Anneal 0.25.1 was run over a Murail jj added workspace and its colocated Git
checkout, both pinned to `b8733704fd12`. The `.design` Markdown trees and the
mounted `crates/murail-model/fm` Markdown trees were byte-identical, and an
independent probe produced byte-identical file-handle sets.

| Surface | jj added workspace | Git checkout | Disposition |
|---|---:|---:|---|
| `git_mtime` | 0 | 2,539 | no fabricated tuple, but capability undisclosed |
| `changed_within(h, 30)` | 0 | 943 | no fabricated tuple, but capability undisclosed |
| `spec_code_drift` source handles | 0 | 37 | false zero |
| assertion edges | 20,742 | 20,742 | population preserved |
| edges with assertion date and revision | 0 | 11,586 | global absence collapsed into nullable fields |
| edges with both fields null | 20,742 | 9,156 | legitimate null and unavailable provenance conflated |

The jj desk also reported
`references/papers/2026-03-29-MANIFEST.md` as a
`gitignored_scanned_file`, although Murail tracks it. `git check-ignore` had
walked past the nearer `.jj` boundary to the ancestor `$HOME` repository. The
ancestor could read Murail's nested `.gitignore` policy but its index could not
know that Murail tracks the file, so it applied a rule Murail's own index
suppresses.

This is the walks/tracks/mounts error from CR-D110 inside its own disclosure:
Anneal asks the wrong repository what the corpus repository tracks. CR-D110
predates CR-D111, when "find the Git root" was still treated as unambiguous.

The status comparison's `open` versus `holding`/`drifting` movement is **not
evidence for this defect**. The desk and anchor carried different Anneal
snapshot histories. That population was excluded rather than attributed to
VCS capability.

`recent_frontier` was byte-identical only because authored dates covered the
selected rows. That is a latent case, not a correctness gate: an undated corpus
can still reorder when Git recency disappears.

## 2. Governing decision

CR-D111 gains an executable nearest-VCS authority. Starting at the corpus root,
the authority walks ancestors until it finds a VCS workspace boundary:

1. `.git` wins when `.git` and `.jj` are colocated. The main jj workspace is
   still a direct Git working tree for existing operations.
2. A `.jj`-only boundary identifies an added jj workspace and stops the walk.
   No Git repository above that boundary may supply history, blame, or index
   semantics.
3. No VCS marker means no repository provider. Package manifests may bound a
   project, but they do not manufacture a VCS capability.

The authority records capabilities **per operation**, never as one history
boolean:

| Operation | Existing direct-Git implementation | jj added workspace in this slice |
|---|---|---|
| `change_history` | `git_mtime`, `changed_within`, Git recency fallback | unavailable |
| `assertion_blame` | edge assertion date and revision | unavailable |
| `target_history` | W006, referent history, drift evidence | unavailable |
| `ignore_index` | tracked-aware `git check-ignore` | unavailable |

The current availability happens to be all-or-none for direct Git versus jj,
but the representation must not encode that coincidence. Future jj work may
earn these operations independently.

The nearest-VCS authority is shared substrate because both query hosts and
source adapters require it. It belongs at the `anneal-core` root under the
public-altitude admission rule. Static `SourceCapabilities` remains a source's
declared implementation support; repository-operation capability is runtime
availability for this concrete workspace and must not be folded into it.

## 3. Runtime contract

The sealed relation is:

```text
repository_operation_capability(operation, availability, provider, reason)
```

It emits one row for each of the four operations. `availability` is
`available` or `unavailable`; `provider` is `git`, `jj`, or `none`. `reason`
names the observed state per operation. Direct Git uses
`direct-git-worktree`; jj distinguishes unavailable change history, assertion
blame, target history, and workspace index semantics; a non-VCS workspace uses
`no-vcs-workspace`.

This relation is the machine-readable distinction missing from the 0.25.1
surfaces. An empty `git_mtime` relation no longer has to carry both "no files
have history" and "history cannot be asked here."

Capability state is positive evidence, not permission. It neither authorizes
an actor nor promises that any result row exists. Actor `RuntimeCapability`
and source `SourceCapabilities` keep their existing meanings.

## 4. Surface contract

Unavailable operations appear only when applicable; direct-Git output stays
byte-identical.

Status renders two consequence-scoped lines:

```text
History      jj workspace, Git-derived recency, W006, and assertion provenance unavailable
Scope        Git ignore-index classification unavailable
```

When `target_history` is unavailable, Health renders:

```text
Health       errors=N, blockers=N, spec_code_drift=- (distinct source handles)
```

The dash is unknown, never zero. The Diagnostics line uses `observed` rather
than `total` while a diagnostic-producing capability is unavailable. The
`check` adjacent-set hint likewise says `observed non-error diagnostic rows`
and names W006 as unavailable; the error gate and exit status do not change.
Convergence cells whose membership can move when W006 evidence appears
(`blocked`, `open`, `holding`, and `drifting`) likewise render `-`; `broken`
and snapshot-derived `advancing` retain their earned counts.

An empty query that reaches `git_mtime` or `changed_within` replaces the generic
zero-result adjacency with:

```text
hint: Git change history is unavailable in this jj workspace; query `repository_operation_capability` for runtime availability.
```

A query projecting `assertion_date` or `assertion_revision` emits a stderr hint
when `assertion_blame` is unavailable, even when edge rows exist:

```text
hint: assertion provenance is unavailable in this jj workspace; null fields may mean unavailable provenance or no per-edge assertion evidence. Query `repository_operation_capability`.
```

The capability row is what makes the nullable edge projection honest. Null
remains the correct value for an individual edge with no assertion evidence;
it no longer has to carry global provider failure alone.

`gitignored_scanned_file` emits no row when `ignore_index` is unavailable.
Status names the unavailable classification instead of reporting a false
positive or silently claiming the scanned set is clean.

`recent_frontier` retains authored-date behavior. Its teaching card states that
Git fallback is conditional on `change_history`; it does not claim byte
identity from the measured dated corpus as a general invariant.

## 5. Delivery and gates

Two commits keep failure attribution intact:

1. Add the nearest-VCS authority and route change history, assertion blame,
   target history, drift caching, and ignore-index classification through it.
2. Add the sealed projection and the conditional CLI disclosures above.

The synthetic acceptance fixture nests a `.jj`-only workspace below an
unrelated Git repository and includes a path ignored by the corpus policy but
tracked by the corpus repository. The fixture fails if any operation crosses
the `.jj` boundary or if `check-ignore` consults the unrelated index.

Controls cover direct Git, colocated `.git` plus `.jj`, and non-VCS corpora.
The real Murail desk/anchor A/B verifies the exact user-visible delta. Existing
anneal, Murail anchor, and Herald surfaces remain byte-identical. Public API and
performance measurements carry their exact commands.

## 6. Deliberate non-decision

This correction does not implement jj history. It does not equate Git commit
time with jj change time, choose jj commit versus change identity for caches,
or define jj blame and move semantics. `anneal-qao9` resumes those decisions
only after this defect lands, using official current jj interfaces and separate
evidence for each operation.
