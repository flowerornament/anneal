# Phase 2: Checks & CLI - Research

**Researched:** 2026-03-28
**Domain:** Rust CLI application -- local consistency checking, reverse graph traversal, clap subcommands, compiler-style diagnostics
**Confidence:** HIGH

## Summary

Phase 2 builds on a solid Phase 1 foundation (259 files, 9788 handles, 6408 edges, 22 namespaces in <200ms) to add the five local consistency rules (KB-R1 through KB-R5), impact analysis via reverse graph traversal, extensible frontmatter field mapping, and the core CLI commands (check, get, find, init, impact). The existing codebase already has forward stubs for most Phase 2 APIs (`node()`, `outgoing()`, `incoming()`, `edges_by_kind()`, `classify_status()`, `compute_freshness()`, `state_level()`, `frontmatter_adoption_rate()`) marked with `#[allow(dead_code)]`.

The critical prerequisite work is fixing Phase 1 resolution gaps: bare filename resolution (D-02), terminal status classification via directory convention analysis (D-04), code block label skip (D-08), and URL false positive rejection (D-03). Without these fixes, CHECK-01 would produce ~3100 noise diagnostics instead of real broken references, and CHECK-02/CHECK-03 would have no terminal/active distinction. The extensible frontmatter field mapping (D-05/D-06, CONFIG-03) is the most architecturally significant addition, requiring changes to config.rs, parse.rs, and the build_graph pipeline.

All 18 phase requirements map cleanly to existing module structure. No new crates are needed. The clap 4.6.0 derive API supports subcommands, global flags, and all patterns needed for the CLI. The `CommandOutput` trait (spec section 15.3) provides the dual JSON/human output pattern.

**Primary recommendation:** Execute in three waves: (1) fix Phase 1 resolution gaps + wire terminal status + code block skip, (2) implement five check rules + impact analysis + diagnostics, (3) implement CLI subcommands + init auto-detection + extensible frontmatter config.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Section references (~2517 edges) get a single info-level summary diagnostic (I001). Not per-reference errors.
- **D-02:** Wire existing `resolve_file_path()` for bare filenames (~870 refs) and frontmatter bare names (~70 refs). Search relative to referring file directory, then root.
- **D-03:** Fix file path regex to reject URL fragments (~6 false positives) via negative lookbehind for `://`.
- **D-04:** Wire directory convention analysis into `build_graph`. Walk directories during graph construction and tag which statuses appear in `archive/`, `history/`, `prior/` directories. Pass to `infer_lattice` as `terminal_by_directory`.
- **D-05:** Fully extensible frontmatter field mapping via `anneal.toml`. All frontmatter fields configurable, including the 6 current core fields. Config maps field names to edge kinds with direction.
- **D-06:** Zero-config case ships sensible defaults matching current 6 core fields.
- **D-07:** `anneal init` scans all observed frontmatter keys, identifies reference-like values, proposes field-to-edge-kind mappings.
- **D-08:** Skip label scanning inside fenced code blocks.
- **D-09:** Version handles inherit status from their parent file handle.
- **D-10:** Remove unused `version_refs` collection from `scan_file`.
- **D-11:** Remove dead `is_excluded` helper from `parse.rs`.

### Claude's Discretion
- Diagnostic formatting details within compiler-style constraint (CHECK-06): color, alignment, grouping
- Error code numbering scheme (E001/W001/I001 vs E0001/W0001 etc.)
- Internal architecture of extensible frontmatter mapping (trait-based, table-driven, etc.)
- How `anneal find` implements full-text search (simple substring, regex, or glob matching)
- Whether `anneal get` shows raw content or formatted output

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CHECK-01 | Existence check -- every edge target must resolve | D-02 fixes bare filename resolution; D-01 handles section refs as info summary; D-03 fixes URL false positives. Spec KB-R1. |
| CHECK-02 | Staleness check -- active handle referencing terminal handle | D-04 provides terminal status classification via directory convention. Spec KB-R2. `classify_status()` exists in lattice.rs. |
| CHECK-03 | Confidence gap -- DependsOn where source > target state | D-04 + lattice ordering. Spec KB-R3. `state_level()` and `edges_by_kind()` exist. |
| CHECK-04 | Linearity check -- linear handles discharged exactly once | Spec KB-R4, KB-D15. Config `handles.{NS}.linear = true`. Obligation lifecycle: Created -> Outstanding -> Discharged or Mooted. |
| CHECK-05 | Convention adoption -- missing frontmatter when >50% siblings have it | Spec KB-R5, KB-D12. `frontmatter_adoption_rate()` exists. Per-directory sibling analysis. |
| CHECK-06 | Compiler-style diagnostics with error codes | Spec section 12.1 format. Diagnostic struct with severity, code, message, location. |
| IMPACT-01 | Reverse traversal over DependsOn, Supersedes, Verifies | Spec KB-D16, section 9. `incoming()` exists in graph.rs. |
| IMPACT-02 | Cycle detection via visited set | Standard BFS/DFS with HashSet<NodeId>. Graph has dual adjacency lists. |
| IMPACT-03 | Show direct and indirect affected handles | Two-tier output: depth=1 (direct) and depth>1 (indirect). |
| CLI-01 | `anneal check` -- run checks, report diagnostics, exit non-zero on errors | Spec section 12.1. Wire check rules, count errors, set exit code. |
| CLI-02 | `anneal get <handle>` -- resolve handle, show content + state + context | Spec section 12.2. Resolve identity string, look up graph node, format output. |
| CLI-03 | `anneal find <query>` -- full-text search filtered by convergence state | Spec section 12.3. Substring search across handle identities and file content. |
| CLI-06 | `anneal init` -- generate anneal.toml from inferred structure | Spec section 12.6. D-07 auto-detection. Serialize `AnnealConfig` to TOML. |
| CLI-07 | `anneal impact <handle>` -- show affected handles | Spec section 12.7. Wire IMPACT-01/02/03. |
| CLI-09 | All commands support `--json` via global flag | Already on `Cli` struct. `CommandOutput` trait: `Serialize` + `print_human()`. |
| CLI-10 | Human-readable output as default via `CommandOutput` trait | Spec section 15.3. Each command returns a struct implementing `CommandOutput`. |
| CONFIG-03 | Extensible frontmatter field mapping, concern groups, linear namespaces | D-05/D-06. Extend `AnnealConfig` with `[frontmatter.fields]` table. |
| CONFIG-04 | `anneal init` generates config from inferred structure | D-07. Scan frontmatter keys, detect reference-like values, propose mappings. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| clap | 4.6.0 | CLI with derive subcommands | Already in Cargo.toml. Derive API for subcommands, global flags, enum dispatch. |
| serde | 1.0.228 | Serialization framework | Already in Cargo.toml. All output types derive `Serialize`. |
| serde_json | 1.0.149 | JSON output for `--json` flag | Already in Cargo.toml. `serde_json::to_string_pretty()` for CLI output. |
| anyhow | 1.0.102 | Error handling with context | Already in Cargo.toml. `Result<T>` throughout with `.context()`. |
| regex | 1.12.3 | Pattern scanning | Already in Cargo.toml. RegexSet + individual Regex with LazyLock. |
| camino | 1.2.2 | UTF-8 paths | Already in Cargo.toml. `Utf8PathBuf` throughout. |
| toml | 0.8.23 | Config serialization (for `anneal init` output) | Already in Cargo.toml. Need `toml::to_string_pretty()` for generating anneal.toml. |

### Supporting
No new dependencies needed. All Phase 2 work uses the existing 10-crate stack.

**No installation needed -- all dependencies are already in Cargo.toml.**

## Architecture Patterns

### Module Structure (Phase 2 additions)

```
src/
  checks.rs       # NEW: Five check rules, Diagnostic type, severity enum
  impact.rs       # NEW: Reverse graph traversal with cycle detection
  cli.rs          # NEW: CommandOutput trait, subcommand dispatch, output formatting
  config.rs       # MODIFY: Add FrontmatterFieldConfig, ConcernsConfig, linear namespaces
  parse.rs        # MODIFY: Code block label skip (D-08), directory convention collection (D-04), extensible frontmatter parsing (D-05)
  resolve.rs      # MODIFY: Wire resolve_file_path for bare filenames (D-02)
  handle.rs       # MODIFY: Add HandleMetadata extensible fields
  lattice.rs      # MINOR: Remove #[allow(dead_code)] from functions now used
  graph.rs        # MINOR: Remove #[allow(dead_code)] from methods now used
  main.rs         # MODIFY: Subcommand dispatch, CommandOutput routing
```

### Pattern 1: Diagnostic Type System

The five check rules produce diagnostics of three severities. All diagnostics flow through a single type:

```rust
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Diagnostic {
    pub(crate) severity: Severity,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum Severity {
    Error,
    Warning,
    Info,
}
```

Error codes following the spec section 12.1 format:
- `E001`: Broken reference (KB-R1)
- `E002`: Undischarged obligation (KB-R4, zero discharges)
- `W001`: Stale reference (KB-R2)
- `W002`: Confidence gap (KB-R3)
- `W003`: Missing frontmatter convention (KB-R5)
- `I001`: Section reference summary (D-01)
- `I002`: Multiple discharges (KB-R4, affine -- redundant but harmless)

### Pattern 2: CommandOutput Trait

Per spec section 15.3, every command returns a type implementing both `Serialize` and `print_human()`:

```rust
pub(crate) trait CommandOutput: Serialize {
    fn print_human(&self, w: &mut dyn std::io::Write) -> std::io::Result<()>;
}
```

The main dispatch:
```rust
fn dispatch_output(output: &dyn CommandOutput, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::to_value(output)?)?);
    } else {
        output.print_human(&mut std::io::stdout().lock())?;
    }
    Ok(())
}
```

Note: Since `CommandOutput` requires both `Serialize` and a method, and Rust does not support trait upcasting to `dyn Serialize`, the dispatch will likely use a concrete enum or generic function rather than trait objects. Each subcommand returns its own output type; the main match dispatches per-variant.

### Pattern 3: Clap Subcommand Derive

The existing `Cli` struct needs a `#[command(subcommand)]` field:

```rust
#[derive(Parser)]
#[command(name = "anneal", about = "Convergence assistant for knowledge corpora")]
struct Cli {
    #[arg(long)]
    root: Option<String>,

    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run local consistency checks
    Check {
        #[arg(long)]
        errors_only: bool,
    },
    /// Resolve a handle and show its content
    Get {
        handle: String,
        #[arg(long)]
        refs: bool,
    },
    /// Search handles by text
    Find {
        query: String,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        namespace: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    /// Generate anneal.toml from inferred structure
    Init {
        #[arg(long)]
        dry_run: bool,
    },
    /// Show what's affected if a handle changes
    Impact {
        handle: String,
    },
}
```

Note: `status`, `map`, and `diff` are Phase 3 commands. Only the 5 commands above are in scope.

### Pattern 4: Extensible Frontmatter Field Mapping (D-05/D-06)

Table-driven approach. Config maps field names to edge semantics:

```rust
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct FrontmatterFieldMapping {
    pub(crate) edge_kind: String,      // "DependsOn", "Supersedes", etc.
    pub(crate) direction: Direction,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Direction {
    Forward,  // source -> target (e.g., "depends-on: X" means this file DependsOn X)
    Inverse,  // target -> source (e.g., "affects: X" means X DependsOn this file)
}
```

Default mappings (zero-config):
| Field | EdgeKind | Direction | Current Behavior |
|-------|----------|-----------|-----------------|
| `superseded-by` | Supersedes | Forward | Source superseded by target |
| `depends-on` | DependsOn | Forward | Source depends on target |
| `discharges` | Discharges | Forward | Source discharges target |
| `verifies` | Verifies | Forward | Source verifies target |
| `supersedes` | Supersedes | Inverse | Target superseded by source (reverse of `superseded-by`) |
| `affects` | DependsOn | Inverse | Target depends on source |

The parsing pipeline (`parse_frontmatter`) becomes table-driven: for each key in the frontmatter YAML, check if it maps to a configured field, extract values as string or list-of-strings, produce pending edges with the appropriate kind and direction.

### Pattern 5: Directory Convention Analysis (D-04)

During `build_graph`, track which statuses appear in terminal-convention directories:

```rust
const TERMINAL_DIRS: &[&str] = &["archive", "history", "prior"];

// In build_graph, for each file:
// Check if any ancestor directory name is in TERMINAL_DIRS
// If so, and file has a status, add status to terminal_by_directory set
```

From the Murail corpus analysis:
- `archive/` dirs: statuses "historical" (8), "complete" (5)
- `history/` dirs: statuses "superseded" (5), "authoritative" (4), "historical" (1), "full-form" (1)
- `prior/` dirs: mixed -- "proposal" (5), "authoritative" (4), "historical-extract" (3), etc.

The `prior/` directory is noisy -- it contains files with active-looking statuses ("proposal", "active", "draft"). The directory convention should be a signal that feeds into `infer_lattice` but not override explicit config. Only statuses that appear **exclusively** in terminal directories (and not in non-terminal directories) should be auto-classified as terminal.

### Pattern 6: Impact Analysis Traversal

Reverse BFS from a starting handle over DependsOn, Supersedes, and Verifies edges:

```rust
pub(crate) fn compute_impact(
    graph: &DiGraph,
    start: NodeId,
) -> ImpactResult {
    let mut visited = HashSet::new();
    let mut direct = Vec::new();
    let mut indirect = Vec::new();
    let mut queue = VecDeque::new();

    visited.insert(start);
    queue.push_back((start, 0u32)); // (node, depth)

    while let Some((current, depth)) = queue.pop_front() {
        for edge in graph.incoming(current) {
            if !matches!(edge.kind, EdgeKind::DependsOn | EdgeKind::Supersedes | EdgeKind::Verifies) {
                continue;
            }
            if visited.insert(edge.source) {
                if depth == 0 {
                    direct.push(edge.source);
                } else {
                    indirect.push(edge.source);
                }
                queue.push_back((edge.source, depth + 1));
            }
        }
    }

    ImpactResult { direct, indirect }
}
```

### Anti-Patterns to Avoid
- **Reporting all unresolved pending edges as broken references:** D-01 specifies section refs get a single summary diagnostic, not per-reference errors.
- **Global mutable state for diagnostics:** Pass diagnostics as `&mut Vec<Diagnostic>` through check functions, not global collectors.
- **Hardcoding frontmatter field names:** D-05 requires extensible mapping. Do not add new special-case fields to `parse_frontmatter`; use the table-driven approach from the start.
- **Filesystem access in check rules:** Checks operate on the in-memory graph only. All filesystem work happens in `build_graph`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| CLI argument parsing | Custom parser | clap 4 derive macros | Already in use; subcommand derive is mature |
| Compiler-style diagnostic formatting | Custom formatter | Hand-roll diagnostic printer (~60 lines) | No Rust diagnostic library fits; `codespan-reporting` and `miette` require source spans that anneal doesn't have. A simple format function is sufficient for file:line diagnostics. |
| TOML serialization for `init` | Custom TOML writer | `toml::to_string_pretty()` | Already have the `toml` crate. Need Serialize derives on config types. |
| Reverse graph traversal | Custom graph library | Existing `DiGraph::incoming()` | Already built with dual adjacency lists |

**Key insight:** anneal's diagnostics are file-level, not span-level. Diagnostic crates like `miette` or `codespan-reporting` are designed for source code with byte offsets and line/column spans. anneal reports on handles and files, not character positions. A simple ~60-line printer matching the compiler-style format from spec section 12.1 is more appropriate than adding a heavyweight diagnostic crate.

## Common Pitfalls

### Pitfall 1: False Positive Broken References from Section Refs
**What goes wrong:** Reporting ~2517 section references as broken references because `section:4.1` identity format does not match `path#heading-slug` node index keys.
**Why it happens:** Phase 1 creates pending edges with `section:N.N` target identities. These can never resolve against the node index.
**How to avoid:** D-01 specifies: collect all unresolved section refs, emit a single I001 info diagnostic summarizing the count. Never iterate these as individual errors.
**Warning signs:** `anneal check` producing hundreds of E001 diagnostics for section references.

### Pitfall 2: Terminal Status Classification Contaminates Active Statuses
**What goes wrong:** `prior/` directories contain files with statuses like "proposal", "active", "draft" that should be active in non-terminal contexts.
**Why it happens:** Naive directory convention: "any status seen in prior/ is terminal."
**How to avoid:** Only classify a status as terminal-by-directory if it appears **exclusively** in terminal directories (not also in non-terminal directories). Config overrides take precedence. From Murail data: "superseded" (5 in history/, 0 elsewhere) is clearly terminal. "authoritative" (4 in prior/, 4 in history/, but also 9 elsewhere) should NOT be auto-terminal.
**Warning signs:** Common active statuses like "proposal" or "draft" classified as terminal.

### Pitfall 3: Clippy Pedantic on New Code
**What goes wrong:** New code fails `clippy::pedantic` with denials for `clippy::module_name_repetitions`, `clippy::must_use_candidate`, etc.
**Why it happens:** Cargo.toml denies `clippy::all` and `clippy::pedantic` at priority -1, with specific allows listed.
**How to avoid:** Check Cargo.toml allows before writing code. Currently allowed: `module_name_repetitions`, `must_use_candidate`, `return_self_not_must_use`, `missing_errors_doc`, `missing_panics_doc`, `doc_markdown`, `too_many_lines`, `similar_names`, `items_after_statements`, `struct_excessive_bools`. New functions MUST use `?` propagation (no `unwrap()`), explicit types on public APIs, and doc comments on public items (though `missing_errors_doc`/`missing_panics_doc` are allowed).
**Warning signs:** `just check` failing on clippy before commit.

### Pitfall 4: `deny_unknown_fields` Breaks Config Extensibility
**What goes wrong:** Adding `[frontmatter.fields]` to anneal.toml causes parse failure because `AnnealConfig` has `deny_unknown_fields`.
**Why it happens:** The existing `AnnealConfig` denies unknown fields. Any new config section must have a corresponding struct field.
**How to avoid:** Add a `frontmatter` field to `AnnealConfig` with its own sub-struct. Also add `concerns` and any other CONFIG-03 fields BEFORE testing with extended anneal.toml files.
**Warning signs:** `anneal init --dry-run` output can't be parsed back by `load_config`.

### Pitfall 5: CommandOutput Trait Object Limitations
**What goes wrong:** Trying to use `Box<dyn CommandOutput>` for polymorphic dispatch fails because `Serialize` is not object-safe.
**Why it happens:** `serde::Serialize` requires `Self: Sized` for `serialize()`.
**How to avoid:** Use a match on the subcommand enum and dispatch to concrete types. Each branch creates its output struct and calls the appropriate formatting. Alternatively, serialize to `serde_json::Value` first.
**Warning signs:** Compiler error about `Serialize` not being object-safe.

### Pitfall 6: Bare Filename Resolution Ambiguity
**What goes wrong:** A bare filename like `summary.md` could match multiple files in different directories.
**Why it happens:** Murail has ~870 bare file refs. Some filenames may exist in multiple subdirectories.
**How to avoid:** D-02 specifies: search relative to referring file directory first, then root. Take the first match. If multiple matches exist at the same priority level, resolve to the closest one (directory of referring file). Log ambiguities as a diagnostic.
**Warning signs:** Non-deterministic resolution depending on filesystem walk order.

### Pitfall 7: `in_code_block` Not Tracked for Labels
**What goes wrong:** Labels inside fenced code blocks create spurious edges.
**Why it happens:** Phase 1 tracks `in_code_block` for headings but still scans labels inside code blocks. D-08 requires extending the skip to labels too.
**How to avoid:** When `in_code_block` is true, skip ALL pattern matching (labels, file paths, section refs), not just headings.
**Warning signs:** `anneal check` reporting edges from code examples.

## Code Examples

### Check Rule Implementation Pattern (KB-R1 Existence)

```rust
// Source: spec section 7, KB-R1
pub(crate) fn check_existence(
    graph: &DiGraph,
    unresolved: &[PendingEdge],
    section_ref_count: usize,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // D-01: Single summary for section refs
    if section_ref_count > 0 {
        diagnostics.push(Diagnostic {
            severity: Severity::Info,
            code: "I001",
            message: format!(
                "{section_ref_count} section references use section notation, \
                 not resolvable to heading slugs"
            ),
            file: None,
            line: None,
        });
    }

    // Real broken references: pending edges that failed resolution
    // (excluding section refs, which are filtered before this call)
    for edge in unresolved {
        diagnostics.push(Diagnostic {
            severity: Severity::Error,
            code: "E001",
            message: format!("broken reference: {} not found", edge.target_identity),
            file: graph.node(edge.source).file_path.as_ref().map(|p| p.to_string()),
            line: None,
        });
    }

    diagnostics
}
```

### Compiler-Style Diagnostic Formatting

```rust
// Source: spec section 12.1 format
impl Diagnostic {
    pub(crate) fn print_human(&self, w: &mut dyn std::io::Write) -> std::io::Result<()> {
        let prefix = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warn",
            Severity::Info => "info",
        };
        write!(w, "{prefix}[{}]: {}", self.code, self.message)?;
        if let Some(ref file) = self.file {
            write!(w, "\n  -> {file}")?;
            if let Some(line) = self.line {
                write!(w, ":{line}")?;
            }
        }
        writeln!(w)
    }
}
```

### Extensible Frontmatter Default Mappings

```rust
// Source: D-05, D-06. Zero-config defaults for the 6 core fields.
impl Default for FrontmatterConfig {
    fn default() -> Self {
        let mut fields = HashMap::new();
        fields.insert("superseded-by".to_string(), FrontmatterFieldMapping {
            edge_kind: "Supersedes".to_string(),
            direction: Direction::Forward,
        });
        fields.insert("depends-on".to_string(), FrontmatterFieldMapping {
            edge_kind: "DependsOn".to_string(),
            direction: Direction::Forward,
        });
        fields.insert("discharges".to_string(), FrontmatterFieldMapping {
            edge_kind: "Discharges".to_string(),
            direction: Direction::Forward,
        });
        fields.insert("verifies".to_string(), FrontmatterFieldMapping {
            edge_kind: "Verifies".to_string(),
            direction: Direction::Forward,
        });
        // Extended defaults for common patterns
        fields.insert("supersedes".to_string(), FrontmatterFieldMapping {
            edge_kind: "Supersedes".to_string(),
            direction: Direction::Inverse,
        });
        fields.insert("affects".to_string(), FrontmatterFieldMapping {
            edge_kind: "DependsOn".to_string(),
            direction: Direction::Inverse,
        });
        Self { fields }
    }
}
```

### Init Auto-Detection (D-07, CONFIG-04)

```rust
// Scan all frontmatter keys across all files, identify reference-like values
fn detect_frontmatter_fields(build_result: &BuildResult) -> Vec<DetectedField> {
    // For each frontmatter key not in the default set:
    // 1. Check if values look like label patterns ([A-Z]+-\d+)
    // 2. Check if values look like file paths (*.md)
    // 3. Check if values are lists of identifiers
    // Propose edge_kind based on field name heuristics:
    //   "affects", "impacts" -> DependsOn inverse
    //   "source", "sources", "based-on" -> DependsOn forward
    //   "resolves", "addresses" -> Discharges forward
    //   etc.
    // Return proposed mappings for the generated anneal.toml
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Hardcoded frontmatter fields | Extensible field mapping (D-05) | Phase 2 | Any frontmatter field can create edges |
| Empty terminal set | Directory convention + config | Phase 2 | CHECK-02/03 become functional |
| Flat CLI (--root, --json only) | Subcommand dispatch | Phase 2 | Full CLI surface per spec section 12 |
| Labels scanned in code blocks | Code block skip (D-08) | Phase 2 | Fewer spurious edges |

## Project Constraints (from CLAUDE.md)

- **Rust project** -- use `just` for all build operations
- **Toolchain:** Rust 1.94.0 stable, edition 2024
- **`unsafe` is denied** workspace-wide
- **Clippy:** all + pedantic denied, with targeted allows
- **No `unwrap()` in production** -- use `expect("reason")` or `?`
- **`--json` on every command** -- `CommandOutput` trait
- **Pre-commit hook** runs `just check` (fmt + clippy + test)
- **Test corpus:** `~/code/murail/.design/` (260 files, 120 with frontmatter)
- **Hand-roll** graph, frontmatter split, JSONL -- no external libraries for these
- **`serde_yaml_ng`** (maintained fork), not `serde_yaml`

## Open Questions

1. **`anneal find` search implementation**
   - What we know: Spec says "full-text in v1" (section 12.3). Interface allows `--all`, `--status`, `--namespace` filters.
   - What's unclear: Whether "full-text" means searching file content or just handle identities/metadata. For file content search, we'd need to keep content in memory or re-read files.
   - Recommendation: Search handle identities and frontmatter status/metadata first (fast, no extra I/O). Add file content search only if the identity search is insufficient. Handle IDs + metadata covers the primary use case ("find me all OQ handles that are open").

2. **`anneal get` output format**
   - What we know: Spec says "content, state, and graph context" (section 12.2). For files, content is the file body. For labels, content is the definition site text.
   - What's unclear: How much content to show. Full file body could be very long.
   - Recommendation: Show handle metadata (id, kind, status, file path) + incoming/outgoing edge summary. For `--refs`, include the edge list. Don't dump raw file content by default -- that's what the editor is for.

3. **Inverse edge direction in extensible frontmatter**
   - What we know: D-05 specifies direction (forward/inverse). `affects: [OQ-14, FM-006]` means "OQ-14 DependsOn this file" (inverse).
   - What's unclear: Whether inverse edges should use the file containing the frontmatter as source or target.
   - Recommendation: For `direction: inverse`, the file containing the frontmatter is the **target** and the referenced handle is the **source**. This means `affects: [OQ-14]` in file A creates edge `OQ-14 --DependsOn--> A`. This matches the semantic: "this file affects OQ-14" means "OQ-14 depends on this file."

## Sources

### Primary (HIGH confidence)
- `.design/anneal-spec.md` -- sections 7, 8, 9, 12, 13, 15. Authoritative specification.
- `.planning/phases/01-graph-foundation/01-OBSERVATIONS.md` -- 9 post-execution observations.
- `.planning/phases/02-checks-cli/02-CONTEXT.md` -- 11 locked decisions.
- Source code: `src/graph.rs`, `src/lattice.rs`, `src/parse.rs`, `src/resolve.rs`, `src/config.rs`, `src/handle.rs`, `src/main.rs`.

### Secondary (MEDIUM confidence)
- Murail corpus analysis (grep-based): status distributions, directory conventions, frontmatter field usage.

### Tertiary (LOW confidence)
- None. All findings verified against spec and source code.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all libraries already in Cargo.toml with resolved versions verified
- Architecture: HIGH -- spec is authoritative and detailed, existing code has clear integration points
- Pitfalls: HIGH -- derived from Phase 1 observations and Murail corpus analysis with concrete numbers
- Check rules: HIGH -- spec section 7 gives exact semantics for all 5 rules
- CLI patterns: HIGH -- clap 4 derive API is mature and already in use

**Research date:** 2026-03-28
**Valid until:** indefinite (specification-driven project with locked decisions)
