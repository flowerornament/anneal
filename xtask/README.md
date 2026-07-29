# Anneal xtask

Workspace-local, read-only development instruments invoked through
`cargo xtask` or `just`.

## Atlas

The atlas derives a whole-workspace Rust name and module map directly from the
current source tree. It owns no ledger and assigns no composite quality grade.
Each view keeps its evidence separate:

- `census` reports the production name population and non-test lines;
- `concentration` reports the largest top-level block in each module, so a
  single god object cannot hide inside an average;
- `name` reports declarations, documentation, re-export facades, approximate
  references, and declaration-level co-mention edges;
- `module` reports orientation comments, item population, heuristic fan-in and
  fan-out, orphan types, and optional item/edge detail;
- `comments --audit` reports missing module and item documentation;
- `diff` reports module-shape movement against a Git revision;
- `dump --json` exposes the complete derived carrier for other tools.

```bash
just atlas census
just atlas concentration anneal-core
just atlas module crates/anneal-core/src/runtime/eval.rs
just atlas name GraphIndex
just atlas comments --audit
just atlas diff HEAD~1
```

The scanner is deliberately line-based and approximate. It recognizes
declaration signatures and top-level blocks without adding a second Rust parser.
Co-mention and import edges are therefore navigational evidence, not compiler
truth; rendered output labels them accordingly.

This implementation is a close port of Murail's proven atlas. Anneal removes
only Murail's formal-model binding enrichment and supplies its own comment
citation vocabulary. Once both consumers have established the stable seams, the
shared scanner and views should move to a standalone library rather than evolve
as two implementations.
