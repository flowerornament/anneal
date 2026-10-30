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

Atlas source capture, lexical grammar, navigation and shape measurements come
from the pinned public `atlas-core` library. The xtask keeps Anneal's command
syntax, native JSON/text presentation and comment citation vocabulary. Runtime
crates do not depend on Atlas. Relationships exclude comments and strings;
unions are recognized alongside structs and enums.

File modules inherit test boundaries through declared descendants, including
explicit path mounts. Shared production mounts and Cargo target roots remain
production; a directory named `tests` alone is no evidence. Historical scans
retain current workspace membership and read an immutable revision tree through the shared Git boundary. Source bytes, including terminal
CR/LF, are preserved. Historical reads from jj-only desks resolve their own backing Git store;
they never use an ancestor repository. Physical shape reports prefer a
production context when a file has both test and production package mounts;
navigation retains each package context.
