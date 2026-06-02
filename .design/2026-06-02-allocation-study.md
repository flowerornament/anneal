---
status: complete
date: 2026-06-02
issue: anneal-2p0q
epic: anneal-g0l4
authors: [codex]
relates: [.design/2026-06-01-perf-architecture-arc.md]
---

# Allocation Study: Compiler Arc Phase 1

## Question

Does allocation profiling confirm that the current row/binding data model is the
dominant allocation ceiling, justifying the compiler-arc rewrite toward a
schema registry, intern table, tuple store, planned evaluator, and scope views?

## Method

The root `anneal` binary was built with a feature-gated `dhat` allocator:

```bash
RUSTFLAGS='-C force-frame-pointers=yes -C symbol-mangling-version=v0' \
  cargo build --profile profiling --features dhat-heap -p anneal
```

Completed dhat workloads:

```bash
target/profiling/anneal --root ~/code/murail/.design \
  -e '? *handle{id:h}.' --format=json

target/profiling/anneal --root ~/code/murail/.design \
  -e '? active(h).' --format=json

target/profiling/anneal --root ~/code/murail/.design \
  -e '? at("snapshot:last") { *handle{id: h, status: status} }.' --format=json
```

The canonical `status` workload was attempted first, but per-allocation
backtrace capture did not complete within a useful window:

```text
anneal --root ~/code/murail/.design status --format=json
terminated after 500.41s under dhat, still CPU-active
```

A live sample taken during that run showed the main thread inside
`Database::scoped_to_time_ref` cloning `BTreeMap<Ident, StoredRelation>` rows:

```text
sample 34557 1 1
main
  anneal_cli::app::RuntimeSession::eval
    anneal_core::runtime::eval::run_rule_group
      eval_body_traced
        eval_atom_traced
          Database::scoped_to_time_ref       eval.rs:1291
            BTreeMap::clone_subtree<Ident, StoredRelation>
              BTreeMap::clone_subtree<Ident, Value>
```

The local raw sample was also saved while working at
`target/profiles/dhat-status/status-live-sample.txt`, but the durable evidence
is the excerpt above plus the completed `at("snapshot:last")` allocation profile
for the same scoped-view mechanism.

Profile artifacts:

- `target/profiles/dhat-bare/dhat-heap.json`
- `target/profiles/dhat-active/dhat-heap.json`
- `target/profiles/dhat-at/dhat-heap.json`
- aborted status stderr: `target/profiles/dhat-status/status.stderr`

## Headline Numbers

| Workload | Output rows | Total allocated | Allocations | Peak live | Runtime under dhat |
| --- | ---: | ---: | ---: | ---: | ---: |
| bare `*handle` | 1,581 | 828.9 MB | 5.88 M | 357.1 MB | 43.23 s |
| `active(h)` | 1,413 | 822.7 MB | 5.83 M | 353.3 MB | 43.45 s |
| one `at("snapshot:last")` | 1,581 | 1,003.6 MB | 7.67 M | 507.3 MB | 63.97 s |

Approximate allocations per output row:

| Workload | Bytes / row | Allocations / row |
| --- | ---: | ---: |
| bare `*handle` | 524 KB | 3,716 |
| `active(h)` | 582 KB | 4,125 |
| one `at("snapshot:last")` | 635 KB | 4,852 |

These row-normalized numbers are not a claim that output projection alone costs
that much. They show that every query pays the full corpus materialization cost
before returning even a narrow result.

## Allocation Attribution

### Bare Stored Query

`? *handle{id:h}.`

| Bucket | Bytes | Allocations | Share |
| --- | ---: | ---: | ---: |
| Markdown extraction / fact construction | 383.2 MB | 1.74 M | 46.2% |
| Store to database `NamedRow` construction | 361.4 MB | 3.60 M | 43.6% |
| Other | 62.2 MB | 0.43 M | 7.5% |
| Other `BTreeMap` / node churn | 13.1 MB | 0.04 M | 1.6% |
| Output projection rows | 7.3 MB | 0.05 M | 0.9% |
| Eval binding / join path | 1.7 MB | 0.02 M | 0.2% |

### Simple Derived Query

`? active(h).`

| Bucket | Bytes | Allocations | Share |
| --- | ---: | ---: | ---: |
| Markdown extraction / fact construction | 383.2 MB | 1.74 M | 46.6% |
| Store to database `NamedRow` construction | 361.4 MB | 3.60 M | 43.9% |
| Other | 62.3 MB | 0.43 M | 7.6% |
| Other `BTreeMap` / node churn | 13.1 MB | 0.04 M | 1.6% |
| Eval binding / join path | 1.8 MB | 0.02 M | 0.2% |
| Output projection rows | 1.0 MB | 0.004 M | 0.1% |

The simple derived query does not materially add allocation pressure beyond
load/materialization. This refutes the narrow version of the hypothesis:
`Binding` clone/extend is not the dominant allocation source for all queries.

### Single Time-Scoped Query

`? at("snapshot:last") { *handle{id: h, status: status} }.`

| Bucket | Bytes | Allocations | Share |
| --- | ---: | ---: | ---: |
| Markdown extraction / fact construction | 383.2 MB | 1.74 M | 38.2% |
| Store to database `NamedRow` construction | 361.4 MB | 3.60 M | 36.0% |
| `scoped_to_time_ref` clone / view | 169.8 MB | 1.73 M | 16.9% |
| Other | 62.2 MB | 0.43 M | 6.2% |
| Other `BTreeMap` / node churn | 13.1 MB | 0.04 M | 1.3% |
| Output projection rows | 7.4 MB | 0.05 M | 0.7% |
| Eval binding / join path | 6.5 MB | 0.08 M | 0.7% |

One `at("snapshot:last")` block adds 169.8 MB and 1.73 M allocations through
`Database::scoped_to_time_ref`. The full status run invokes this family of
rules repeatedly and became too expensive to complete under dhat; the live CPU
sample during that run was in the same clone path.

## Top Call Stacks

Representative high-allocation stacks:

| Bytes | Blocks | Stack |
| ---: | ---: | --- |
| 72.2 MB | 879 | `pulldown_cmark::Parser::new_ext` -> `scan_file_cmark` -> `build_graph_scoped` |
| 57.0 MB | 14,659 | `content_row` -> `insert_named_rows` -> `from_store_with_visibility` |
| 57.0 MB | 14,659 | `insert_content` -> `insert_row` -> `insert_named_rows` |
| 41.3 MB | 13,622 | `body_lines_in_range` -> `emit_content_spans` |
| 17.2 MB | 349 | `frontmatter_scalars` -> `build_graph_scoped` |
| 17.2 MB | 349 | `parse_frontmatter` -> `build_graph_scoped` |
| 15.8 MB | 439 | `build_graph_scoped` file body reads |
| 15.7 MB | 14,659 | `BTreeSet<ContentKey>` node allocation in content indexes |
| 9.7 MB | 15,310 | `BTreeMap<Ident, Value>` bulk construction for stored rows |
| 78.4 MB | 482,022 | `Value::clone` -> `BTreeMap::clone_subtree` inside time-scope clone |
| 38.2 MB | 60,431 | `BTreeMap<Ident, Value>` node allocation inside time-scope clone |

## Suspect-by-Suspect Result

| Suspect | Result |
| --- | --- |
| `Binding = BTreeMap<Ident, Value>` clone/extend | Not dominant in completed bare or `active(h)` traces: ~1.7-1.8 MB. In the `at` trace, eval binding rises to 6.5 MB, still smaller than materialization and scope cloning. |
| `Row.fields = BTreeMap<String, Value>` output construction | Output projection is small relative to load: 1.0-7.4 MB. Keep deterministic projection at the boundary, but it is not the ceiling. |
| `NamedRow = BTreeMap<Ident, Value>` stored-row construction | Co-dominant with extraction in bare/narrow queries: 361.4 MB and 3.60 M allocations on every profiled query. This is the physical-row model tax paid before narrow queries can run. |
| `Value::String` clones | Dominant by call path, especially content/meta/span row construction and time-scope clone. Repeated `Value::String` payloads are copied through `NamedRow`, stored relations, content indexes, and snapshots. |
| `BTreeMap` node churn | Large when counted through stored rows and time-scope clone. Standalone residual bucket is 13.1 MB, but the major `BTreeMap` node allocations are inside `NamedRow` construction and `scoped_to_time_ref`. |
| `scoped_to_time_ref` database clone | Confirmed: one time block adds 169.8 MB and 1.73 M allocations. Full status under dhat did not complete and live-sampled in this path. Scope-as-view is justified. |
| Output projection | Not the main target. Projection should remain the only `BTreeMap<String, Value>` boundary for JSON/text determinism. |

## What This Confirms

The narrow story "hot `Binding` clone/extend dominates allocations" is false for
the completed traces. The broader compiler-arc story is confirmed:

- Anneal pays hundreds of MB per invocation to convert fact batches into
  `BTreeMap<Ident, Value>` rows.
- It duplicates text and metadata strings across extraction facts, stored rows,
  content indexes, and time-scoped clones.
- Time scope is a deep physical clone, not a cheap relational view.
- Even a bare stored query pays the full physical materialization cost.

So the target should be sharpened:

> Replace the physical row substrate, not just `Binding`.

The tuple store, schema registry, typed ids, intern table, planned evaluator,
and scope overlays remain the right architecture, but the first win is likely
store materialization and scope views before fine-tuning execution frames.

## Realistic Ceiling Estimate

Completed profiles show these removable or reducible buckets:

- Store-to-database `NamedRow` construction: ~361 MB and 3.60 M allocations per
  invocation. A tuple store built directly from source facts should remove most
  of this.
- Time-scope clone: +170 MB and +1.73 M allocations per `at` query. A
  `RelationView` overlay should remove nearly all of this bucket.
- BTreeMap node churn attached to stored rows and time-scope clone: tens of MB
  directly, and millions of allocation sites indirectly.
- String duplication: interning should collapse repeated handle ids, statuses,
  kinds, metadata keys, edge kinds, source ids, file paths, and common values.
  It will not remove large unique body text by itself.

What remains after the physical-model rewrite:

- Markdown extraction and content-span construction: ~383 MB in the current
  profiles. Some of this is unavoidable source text ownership, but the pass
  contract should avoid constructing the same body slices multiple times.
- Large content payload storage: body text remains real data, not a symbol.
- Output projection: a small boundary cost that preserves stable JSON/text
  behavior.

Expected allocation ceiling from the rewrite:

- Bare/narrow queries: reduce allocation volume by roughly 40-50% immediately
  if stored-row BTreeMaps disappear, with a likely larger allocation-count
  reduction because 3.6 M of 5.8 M allocations are in store materialization.
- Time-scoped/status-like queries: reduce allocation volume by roughly 50-60%
  when tuple storage and scope overlays are both in place.
- Further wins require adapter/pass-contract cleanup to reduce extraction-side
  body/frontmatter duplication.

## Gate Verdict

PROCEED, with a corrected emphasis.

The allocation study confirms that the current physical representation is a
dominant allocation ceiling: co-dominant with extraction for bare/narrow
queries, and dominant once time-scope cloning enters the workload. It does not
confirm that `Binding` clone/extend is the dominant allocation site in
isolation. The rewrite should proceed as the design says: logical/physical
separation with tuple storage and scope views.

Recommended implementation order for the arc:

1. Keep the logical API stable: `Row`/`BTreeMap` remains projection-only.
2. Add a scoped-view overlay for `scoped_to_time_ref`. This is the cheapest
   contained first win for status-like workloads and does not require waiting
   for the whole tuple-store rewrite.
3. Replace stored `NamedRow` materialization with schema-addressed tuple rows.
4. Add the interner while lowering source facts into tuples.
5. Then plan/evaluate rules over slot frames; measure whether `Binding` is still
   meaningful after the substrate stops producing BTreeMap rows.
6. Treat adapter extraction duplication as a parallel follow-up, not as a reason
   to abandon the compiler arc.
