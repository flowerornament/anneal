# Post-Arc Runtime Profile — 2026-06-04

Status: measured
Date: 2026-06-04
Epic: anneal-g0l4
Task: anneal-zb7n

## Context

The compiler-arc runtime now has a single shipped substrate for static corpus
stored relations: tuple-backed rows, tuple-native time overlays, deterministic
stored relation order, and SmallVec-backed query bindings. Legacy feature
opt-outs were retired before this measurement.

Named rows still exist at dynamic boundaries: query-local facts, trail/explain
projection, derived relation storage, and final output. They are no longer the
primary stored-corpus representation.

This profile answers the closeout question: after removing the old NamedRow
store and clone-based time scope, what is the next optimization surface?

## Commands

Profiling build:

```bash
rm -rf /tmp/anneal-post-arc-profile
mkdir -p /tmp/anneal-post-arc-profile/{bare,at,status}
CARGO_TARGET_DIR=/tmp/anneal-post-arc-profile/target \
  RUSTFLAGS='-C force-frame-pointers=yes -C symbol-mangling-version=v0' \
  cargo build --profile profiling --bin anneal --features dhat-heap
```

Dhat workloads:

```bash
cd /tmp/anneal-post-arc-profile/bare
/usr/bin/time -p ../target/profiling/anneal \
  --root ~/code/murail/.design \
  -e '? *handle{id:h}.' --format=json \
  >out.json 2>stderr.txt

cd /tmp/anneal-post-arc-profile/at
/usr/bin/time -p ../target/profiling/anneal \
  --root ~/code/murail/.design \
  -e '? at("snapshot:last") { *handle{id: h, status: status} } order by h asc, status asc.' \
  --format=json \
  >out.json 2>stderr.txt

cd /tmp/anneal-post-arc-profile/status
/usr/bin/time -p ../target/profiling/anneal \
  --root ~/code/murail/.design status --format=text \
  >out.txt 2>stderr.txt
```

Perceived wall-clock check on the release binary:

```bash
cargo build --release --bin anneal
hyperfine --warmup 2 -r 10 \
  './target/release/anneal --root ~/code/murail/.design status --format=text >/dev/null'
hyperfine --warmup 2 -r 10 \
  './target/release/anneal --root ~/code/murail/.design -e "? *handle{id:h}." --format=json >/dev/null'
hyperfine --warmup 2 -r 10 \
  './target/release/anneal --root ~/code/murail/.design -e "? at(\"snapshot:last\") { *handle{id: h, status: status} } order by h asc, status asc." --format=json >/dev/null'
```

Artifacts:

- `/tmp/anneal-post-arc-profile/bare/dhat-heap.json`
- `/tmp/anneal-post-arc-profile/at/dhat-heap.json`
- `/tmp/anneal-post-arc-profile/status/dhat-heap.json`

## Headline Numbers

| Workload | Dhat bytes | Blocks | Dhat real time | Release wall time |
| --- | ---: | ---: | ---: | ---: |
| bare `? *handle{id:h}.` | 688,993,638 | 3,447,210 | 25.75s | 845.0ms +/- 13.2ms |
| `at("snapshot:last")` handle scan | 699,085,141 | 3,576,864 | 26.64s | 850.5ms +/- 9.2ms |
| `status` | 1,331,376,316 | 12,304,792 | 107.80s | 1.407s +/- 0.021s |

Perceived performance is materially better than the pre-arc status baseline:
the audited arc comparison was about 3.1s -> 1.45s on murail. The clean
closeout binary measures 1.407s mean over 10 runs.

## Allocation Buckets

Buckets are classified from dhat call stacks. The exact program-point totals
are in the artifact JSON; these buckets are architectural rollups.

### Bare Query

| Bucket | Bytes | Share | Blocks |
| --- | ---: | ---: | ---: |
| markdown extraction | 382,898,217 | 55.6% | 1,728,678 |
| tuple store build | 230,886,438 | 33.5% | 1,310,004 |
| FactStore merge/canonicalization | 39,007,493 | 5.7% | 121,024 |
| snapshot-history load | 14,481,551 | 2.1% | 102,237 |
| eval/output | 2,520,878 | 0.4% | 9,961 |

Bare query is no longer an eval problem. It is load/extract plus tuple
construction.

### Snapshot `at`

| Bucket | Bytes | Share | Blocks |
| --- | ---: | ---: | ---: |
| markdown extraction | 382,898,217 | 54.8% | 1,728,678 |
| tuple store build | 230,886,438 | 33.0% | 1,310,004 |
| FactStore merge/canonicalization | 39,007,021 | 5.6% | 121,023 |
| snapshot-history load | 14,481,551 | 2.1% | 102,237 |
| eval/time scope/output | 12,606,719 | 1.8% | 139,549 |

The tuple-native time overlay is no longer the dominant allocation surface.
The `at` workload is only about 10MB above bare, and release wall time is
effectively the same as bare.

### Status

| Bucket | Bytes | Share | Blocks |
| --- | ---: | ---: | ---: |
| eval/fixpoint | 625,223,221 | 47.0% | 8,727,046 |
| markdown extraction | 385,524,358 | 29.0% | 1,739,973 |
| tuple store build | 230,887,060 | 17.3% | 1,310,016 |
| FactStore merge/canonicalization | 39,007,493 | 2.9% | 121,024 |
| snapshot-history load | 28,987,058 | 2.2% | 205,605 |
| output/projection/render | 9,746,098 | 0.7% | 111,604 |

Status is still eval-heavy, but the hot shape changed. The old buckets
removed by the arc stayed removed: no whole-DB scope clone, no NamedRow
materialization tax, and no BTreeMap binding tree per partial match.

Top remaining eval sub-buckets:

| Eval sub-bucket | Bytes | Blocks | Notes |
| --- | ---: | ---: | --- |
| stored tuple scan candidate/result vectors | 174,229,991 | 2,336,586 | `eval_tuple_stored_traced`, candidate row collection, tuple row vectors |
| aggregate/time-scope-heavy evaluation | 170,346,400 | 2,386,819 | includes aggregate bodies and remaining overlay/path setup |
| derived relation evaluation | 57,057,048 | 235,784 | `eval_derived_from_relation_traced` |
| stored constraint building | 26,817,877 | 459,975 | repeated `Vec<(field,value)>` constraint construction |
| current SmallVec binding clone/extend | 22,657,230 | 613,157 | real, but not dominant |
| comparison evaluation | 10,544,974 | 50,604 | mostly rule-body intermediate rows |

## Corrections To Carry Forward

The current "slot frame" is not the final Plan/IR slot frame from the appendix.
It is a pragmatic flatter binding:

- `SmallVec<[(Ident, Value); 2]>`
- sorted by `Ident`
- binary searched
- still clones logical `Value`

That explains the modest Phase 5 allocation win. The true slot-frame design
is still available, but it is coupled to the Plan/IR middle-end: variables must
be assigned `VarId`/`SlotId` layouts and stored tuple values must flow as
`PhysicalValue` slots through planned atoms. Rebuilding that without planning
would recreate stringly slot bugs in a different container.

## Next Surfaces

1. **Plan/IR middle-end plus true physical slot frames.**
   This is the next eval-side architecture surface. It should compile atoms to
   relation ids, field ids, and slot ids once, then execute without rebuilding
   string-keyed constraints or logical `Value` vectors per atom. It addresses
   several current buckets together: stored candidate/result vectors,
   constraint building, derived relation evaluation, and the remaining
   SmallVec binding clones. This matches two research-graph claims:
   `ordering dependence is the first of three data dependencies that must be
   removed from formatted data systems` and `query compilation and caching
   separates the parse-optimize cost from the execute cost enabling repeated
   queries to amortize planning overhead`.

2. **Markdown extraction pipeline.**
   For narrow queries, extraction is now the dominant cost: 383MB of 689MB on
   bare, and 386MB of status. The top stacks are `pulldown-cmark` first-pass
   allocation, frontmatter YAML parsing, body joins/copies, regex capture
   allocation, and per-file buffer copies. This is outside the eval arc and is
   the highest load-path surface.

3. **Tuple store construction and interner use.**
   Tuple build is still 231MB on every workload. The largest program points are
   `string_value` during content/meta lowering and relation-index construction.
   Possible follow-ups: avoid per-row temporary `Vec<PhysicalValue>` where
   fixed arity is known, pre-size relation stores and indexes from source
   relation counts, and use symbol-aware paths for equality/dedup before
   projecting back to logical `Value`.

4. **Columnar storage only if a CPU profile says scans dominate.**
   Dhat does not justify jumping straight to columnar storage. The current
   row-compact tuple store removed the NamedRow tax; remaining allocation is
   more about candidate/result vectors and planning than row layout. Columnar
   should stay a measured option, not the next default move.

5. **Snapshot/history housekeeping.**
   Snapshot-history load is small but visible, especially status at 29MB.
   It is not the next primary surface, but legacy snapshot-line detection and
   repeated history reads are bounded cleanup candidates.

## Verdict

The arc succeeded: the shipped runtime is cleanly on the tuple substrate, status
is about 2.2x faster by perceived wall time, and the old allocation buckets are
gone.

The next serious eval win is not another container tweak inside the current
interpreter. It is the Plan/IR middle-end that can make execution operate on
relation ids, field ids, slot ids, and `PhysicalValue` slots throughout the hot
path. In parallel or before that, markdown extraction is now the biggest
non-eval load-path cost and is the clearest contained optimization surface.
