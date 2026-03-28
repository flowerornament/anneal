# Phase 1: Graph Foundation - Research

**Researched:** 2026-03-28
**Domain:** Rust graph construction from markdown corpus, handle resolution, convergence lattice inference
**Confidence:** HIGH

## Summary

Phase 1 implements the foundational data model for anneal: scanning a directory of markdown files, extracting handles (File, Section, Label, Version) and typed edges (Cites, DependsOn, Supersedes, Verifies, Discharges), resolving handles across inferred namespaces, computing a convergence lattice from frontmatter status values, and parsing optional `anneal.toml` configuration. The spec (`.design/anneal-spec.md`) is highly prescriptive -- it defines exact types, regex patterns, graph structure, and implementation patterns. The research task is confirming the implementation approach is sound and surfacing pitfalls, not discovering an architecture.

The primary test corpus (Murail `.design/`, 283 markdown files) has been probed. It contains ~25 distinct status values, 15+ label namespaces with thousands of references, known false positives (SHA-256, GPT-2, AVX-512, etc.), and rich cross-reference structure including frontmatter `supersedes:` fields. All 10 dependencies are already declared in Cargo.toml and build cleanly on Rust 1.94.0. The project is greenfield -- only a placeholder `main.rs` exists.

**Primary recommendation:** Implement modules bottom-up (handle -> config -> parse -> graph -> lattice -> resolve -> main), following the spec's type definitions and patterns closely. The spec's key implementation patterns (RegexSet with LazyLock, dual adjacency lists, serde defaults) are well-tested Rust idioms.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Implement full keyword-based edge kind inference in Phase 1, not deferred to Phase 2. Both frontmatter fields (`superseded-by:` -> Supersedes, `discharges:` -> Discharges, `verifies:` -> Verifies, `depends-on:` -> DependsOn) AND body-text context keywords (`incorporates`, `builds on`, `extends`, `based on` -> DependsOn; `see also`, `cf.`, `related` -> Cites) are implemented. Default edge kind for unmatched references is Cites.
- **D-02:** This ensures Phase 2 checks have real DependsOn edges to work with immediately, rather than needing to add inference and checks simultaneously.

### Claude's Discretion
- Keyword proximity rule for body text inference (same-line vs same-paragraph) -- Claude should choose based on what works best during implementation and testing against the Murail corpus.

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| GRAPH-01 | Scan directory tree for .md files and create File handles | walkdir crate, root inference (spec KB-D20), default exclusions pattern |
| GRAPH-02 | Parse YAML frontmatter between `---` fences, extract `status:` and metadata | Hand-rolled frontmatter split (~15 lines) + serde_yaml_ng, spec KB-D6 |
| GRAPH-03 | Parse markdown headings (`#{1,6}`) to create Section handles within files | Regex pattern `^#{1,6}\s`, part of 5-pattern RegexSet |
| GRAPH-04 | Scan content with RegexSet for labels, section refs, file paths, version refs | RegexSet with LazyLock, 5 patterns per spec KB-D6, fast-path optimization |
| GRAPH-05 | Build directed graph with typed edges (5 kinds) | Hand-rolled DiGraph (~135 lines) with dual adjacency lists, spec KB-D5 |
| GRAPH-06 | Graph is computed from files on every invocation, never stored | Ephemeral design principle KB-P1 -- no .anneal/ directory created in Phase 1 |
| HANDLE-01 | Resolve File handles by filesystem path | Camino Utf8PathBuf, relative to root |
| HANDLE-02 | Resolve Section handles to heading ranges within parent files | Section handles carry parent File handle + heading text |
| HANDLE-03 | Resolve Label handles by scanning confirmed namespaces across all files | Two-pass: collect all matches, then filter by confirmed namespaces (KB-D4) |
| HANDLE-04 | Resolve Version handles by matching versioned artifact naming conventions | `v\d+` in versioned context (filenames like `*-v17.md`) |
| HANDLE-05 | Infer handle namespaces by sequential cardinality (N >= 3 members, M >= 2 files) | Namespace inference algorithm per KB-D4 |
| HANDLE-06 | Only labels in confirmed namespaces generate broken-reference errors | Unconfirmed labels are ignored -- no diagnostics for SHA-256, GPT-2 |
| LATTICE-01 | Support two-element existence lattice {exists, missing} as zero-config baseline | Default when no status values found, per KB-D8 |
| LATTICE-02 | Infer confidence lattice from observed frontmatter status values | Collect all `status:` values, build lattice per KB-D9 |
| LATTICE-03 | Partition status values into active and terminal sets | Directory convention (archive/, prior/, history/) + config override per KB-D9, KB-D10 |
| LATTICE-04 | Compute freshness from file mtime or `updated:` frontmatter field | chrono crate for date parsing, std::fs::metadata for mtime, per KB-D11 |
| CONFIG-01 | Parse anneal.toml with all-optional fields via `#[serde(default, deny_unknown_fields)]` | toml crate with serde derive, spec KB-D14 and section 15.3 pattern |
| CONFIG-02 | Zero-config is valid -- tool works with no anneal.toml (existence lattice only) | All config struct fields have Default impls, per KB-P3 |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- **Language:** Rust, edition 2024, pinned to 1.94.0 stable
- **Build:** Use `just` for all build operations; `just check` is the quality gate (fmt + clippy + test)
- **Lint:** `unsafe` denied workspace-wide; clippy all + pedantic denied with targeted allows
- **Style:** No `unwrap()` in production -- use `expect("reason")` or propagate with `?`
- **Dependencies:** 10 crates already declared in Cargo.toml -- use these, do not add new ones
- **Hand-roll:** Graph (~135 lines), frontmatter split (~15 lines), JSONL (~30 lines) -- do NOT use petgraph, gray_matter, or jsonl crates
- **Spec authority:** `.design/anneal-spec.md` is authoritative; follow it closely
- **Test corpus:** `~/code/murail/.design/` (283 files, 15+ namespaces)
- **Output:** `--json` on every command via `CommandOutput` trait: `Serialize` + `print_human()`

## Standard Stack

### Core (already declared in Cargo.toml)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| anyhow | ^1 | Error handling with context chains | Idiomatic Rust error handling for applications |
| clap | ^4 (derive) | CLI argument parsing | Phase 1 needs minimal skeleton only |
| serde | ^1 (derive) | Serialization framework | Required by config, JSON output, YAML parsing |
| serde_json | ^1 | JSON output for `--json` flag | Standard JSON in Rust ecosystem |
| serde_yaml_ng | ^0.10 | YAML frontmatter parsing | Maintained fork of archived serde_yaml |
| toml | ^0.8 | anneal.toml config parsing | Standard TOML parser for Rust |
| regex | ^1 | RegexSet for multi-pattern scanning | 5-pattern content scanner per spec |
| walkdir | ^2 | Recursive directory traversal | Standard for filesystem walking |
| camino | ^1 | UTF-8 typed paths throughout | Guarantees valid UTF-8 in all path operations |
| chrono | ^0.4 (clock) | Date parsing for freshness/updated field | Standard datetime handling |

### Not Used in Phase 1 (but declared)
No crates need adding. All 10 are already in Cargo.toml. Some features (like chrono's clock for freshness computation) are Phase 1 relevant.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Hand-rolled DiGraph | petgraph | petgraph adds 1.5s compile time for 5% of surface used; hand-rolling is ~135 lines |
| Hand-rolled frontmatter split | gray_matter | Frontmatter split is ~15 lines of string splitting; library is overkill |
| serde_yaml_ng | serde_yaml | serde_yaml is archived; serde_yaml_ng is the maintained community fork |
| regex::RegexSet | manual iteration | RegexSet checks all 5 patterns in a single automaton pass -- much faster |

## Architecture Patterns

### Recommended Module Build Order

```
1. handle.rs       # Types only, no logic dependencies
2. config.rs       # Types + serde defaults, no other module deps
3. parse.rs        # Depends on handle types (frontmatter split + regex scanning)
4. graph.rs        # Depends on handle types (DiGraph, Edge, EdgeKind)
5. lattice.rs      # Depends on config (ConvergenceState, active/terminal partition)
6. resolve.rs      # Depends on graph, handle, config (resolution logic)
7. main.rs         # Wires everything together, minimal CLI skeleton
```

### Project Structure (Phase 1 files)

```
src/
  handle.rs       # Handle, HandleKind, HandleId -- the primitive (spec section 4)
  config.rs       # AnnealConfig with serde defaults (spec section 13)
  parse.rs        # Frontmatter split + RegexSet scanning (spec section 5.1)
  graph.rs        # DiGraph with dual adjacency lists (spec section 5)
  lattice.rs      # Lattice trait, ConvergenceState (spec section 6)
  resolve.rs      # Handle resolution across namespaces (spec section 4.2)
  main.rs         # Entry point + minimal CLI skeleton
```

### Pattern 1: Hand-Rolled DiGraph with Dual Adjacency Lists

**What:** Arena-indexed directed graph with forward and reverse adjacency lists for O(1) traversal in both directions.
**When to use:** Always -- this is the only graph representation.
**Source:** Spec section 15.3

```rust
// NodeId is a newtype index into the arena
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct NodeId(u32);

pub struct DiGraph {
    nodes: Vec<Node>,
    fwd: Vec<Vec<Edge>>,   // fwd[src] = outgoing edges from src
    rev: Vec<Vec<Edge>>,   // rev[dst] = incoming edges to dst
}

pub struct Edge {
    pub source: NodeId,
    pub target: NodeId,
    pub kind: EdgeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum EdgeKind {
    Cites,
    DependsOn,
    Supersedes,
    Verifies,
    Discharges,
}
```

### Pattern 2: RegexSet with LazyLock for Multi-Pattern Scanning

**What:** Compile all 5 content patterns once into a RegexSet, stored in a static LazyLock. Each line is checked against the set in a single automaton pass. Only matching lines trigger individual regex extraction.
**When to use:** Content scanning (GRAPH-04).
**Source:** Spec section 15.3

```rust
use std::sync::LazyLock;
use regex::{Regex, RegexSet};

static PATTERN_SET: LazyLock<RegexSet> = LazyLock::new(|| {
    RegexSet::new([
        r"^#{1,6}\s",                    // 0: section headings
        r"[A-Z][A-Z_]*-\d+",            // 1: label references
        r"§\d+(\.\d+)*",                // 2: section cross-references
        r"[a-z0-9_/-]+\.md",            // 3: file path references
        r"\bv\d+\b",                     // 4: version references
    ]).expect("regex patterns must compile")
});

// Individual regexes for capture extraction (also in LazyLock)
static LABEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([A-Z][A-Z_]*)-(\d+)").expect("label regex must compile")
});
```

### Pattern 3: All-Optional Config with Serde Defaults

**What:** Every config field has a concrete type with a Default impl. No Option<T> wrapping. `deny_unknown_fields` catches typos.
**When to use:** `config.rs` for anneal.toml parsing.
**Source:** Spec section 15.3, spec section 13

```rust
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnnealConfig {
    pub root: String,                    // defaults to "" (inferred)
    pub exclude: Vec<String>,            // additional dirs to skip
    pub convergence: ConvergenceConfig,
    pub handles: HandlesConfig,
    pub freshness: FreshnessConfig,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FreshnessConfig {
    pub warn: u32,    // default 30
    pub error: u32,   // default 90
}

impl Default for FreshnessConfig {
    fn default() -> Self {
        Self { warn: 30, error: 90 }
    }
}
```

### Pattern 4: Edge Kind Inference from Context (Locked Decision D-01)

**What:** Edge kinds are inferred from two sources: (a) frontmatter fields and (b) body-text keywords near references.
**When to use:** During content scanning and edge creation.
**Source:** CONTEXT.md D-01, spec section 5 (KB-D5)

Frontmatter field -> edge kind mapping:
- `superseded-by:` / `supersedes:` -> Supersedes
- `discharges:` -> Discharges
- `verifies:` -> Verifies
- `depends-on:` -> DependsOn

Body text keyword -> edge kind mapping:
- `incorporates`, `builds on`, `extends`, `based on` -> DependsOn
- `see also`, `cf.`, `related` -> Cites
- Default (no keyword match) -> Cites

### Pattern 5: Frontmatter Split (Hand-Rolled)

**What:** Split markdown content at `---` fences to extract YAML frontmatter.
**When to use:** Every .md file during parsing (GRAPH-02).

```rust
/// Split file content into optional YAML frontmatter and body.
/// Returns (Some(yaml_str), body) or (None, full_content).
fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    if !content.starts_with("---") {
        return (None, content);
    }
    // Find closing --- fence (skip the opening one)
    if let Some(end) = content[3..].find("\n---") {
        let yaml = &content[3..end + 3].trim();
        let body = &content[end + 3 + 4..]; // skip past "\n---"
        (Some(yaml), body)
    } else {
        (None, content)
    }
}
```

### Pattern 6: Namespace Inference by Sequential Cardinality (KB-D4)

**What:** After collecting all label matches, infer which prefixes are real namespaces vs. false positives.
**When to use:** Handle resolution (HANDLE-05).

Algorithm:
1. Collect all `[A-Z][A-Z_]*-\d+` matches across all files
2. Group by prefix: HashMap<String, Vec<(u32, file_path)>>
3. For each prefix, compute: N = count of distinct sequential numbers, M = count of distinct files
4. Confirmed namespace if N >= 3 AND M >= 2
5. Reject prefixes where all members are at large numbers with no sequential run (SHA-256, AVX-512)
6. Cross-reference with config: `handles.confirmed` and `handles.rejected` override inference

### Anti-Patterns to Avoid

- **Markdown AST parsing:** The spec explicitly says "No markdown AST parsing. No NLP. Five regexes and a YAML parser." Do not use pulldown-cmark or similar.
- **petgraph:** Do not use. The graph is simpler to hand-roll for this use case (~135 lines).
- **Option<T> in config:** Use concrete types with Default impls. The spec is explicit: "All fields have concrete types with Default impls -- no Option<T> wrapping."
- **`unwrap()` anywhere:** Use `expect("reason")` or `?` propagation per CLAUDE.md rules.
- **Storing graph state:** The graph is ephemeral (KB-P1). No `.anneal/` directory, no database, no serialized graph. Recomputed every invocation.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Directory walking | Custom recursive traversal | walkdir | Handles symlinks, permissions, cross-platform edge cases |
| Regex compilation | One-off Regex::new per call | RegexSet + LazyLock | Compiles once, amortized across all files; RegexSet does single-pass matching |
| YAML parsing | Custom frontmatter parser | serde_yaml_ng | Handles YAML edge cases (multiline strings, anchors, type coercion) |
| TOML parsing | Custom config parser | toml crate with serde | Handles all TOML types, deny_unknown_fields catches typos |
| UTF-8 path handling | String manipulation | camino::Utf8PathBuf | Type-safe UTF-8 guarantee on all paths |
| Error context chains | Manual error messages | anyhow::Context | Adds file/line context to errors automatically |
| CLI argument parsing | Manual arg parsing | clap derive | Derive macros generate correct parsing with help text |

**Key insight:** The spec explicitly identifies 3 things TO hand-roll (graph, frontmatter split, JSONL) because they are trivially simple. Everything else uses established crates.

## Common Pitfalls

### Pitfall 1: False Positive Label Detection

**What goes wrong:** Regex `[A-Z][A-Z_]*-\d+` matches technical strings like SHA-256, GPT-2, UTF-8, AVX-512, GPL-3, CRC-32 that are not knowledge labels.
**Why it happens:** The label regex is intentionally broad to catch all possible labels.
**How to avoid:** Implement namespace inference (HANDLE-05) as a two-pass system. First pass collects ALL matches. Second pass filters by confirmed namespaces (N >= 3 sequential members across M >= 2 files). The Murail corpus has 62 SHA-256 matches, 23 AVX-512, 17 GPL-3 -- all correctly rejected by cardinality filtering because they appear with only 1-2 distinct numbers.
**Warning signs:** If the graph has handles with prefixes like SHA, GPT, UTF, AVX, GPL, CRC, something is wrong with namespace filtering.

### Pitfall 2: Frontmatter Boundary Detection

**What goes wrong:** Files that start with `---` but the content between fences is not valid YAML (e.g., horizontal rules in markdown use `---`).
**Why it happens:** `---` is overloaded in markdown as both a frontmatter fence and a horizontal rule.
**How to avoid:** The opening `---` must be the very first line of the file (no leading whitespace or content). The closing `---` must be on its own line. If the "YAML" between fences fails to parse, treat the file as having no frontmatter (log a warning, don't error).
**Warning signs:** Deserialization errors on files that don't actually have frontmatter.

### Pitfall 3: Section Heading Regex in Code Blocks

**What goes wrong:** Lines like `# comment` inside fenced code blocks (```) or YAML frontmatter match the heading regex `^#{1,6}\s`.
**Why it happens:** The regex doesn't know about code block context.
**How to avoid:** Track whether the scanner is inside a fenced code block (toggle on lines starting with `` ``` ``). Skip heading detection inside code blocks. Also skip lines inside the frontmatter YAML section (already handled by splitting frontmatter first, then scanning body).
**Warning signs:** Spurious Section handles created from code examples.

### Pitfall 4: Relative Path Resolution

**What goes wrong:** File path references like `formal-model/murail-formal-model-v11.md` are relative to the referring file's directory, not the root. Getting this wrong breaks File handle resolution.
**Why it happens:** Different files are at different depths in the directory tree.
**How to avoid:** When resolving a `.md` path reference found in file `synthesis/foo.md`, resolve relative to `synthesis/`, then canonicalize relative to root. Use camino's `Utf8Path::join` and path normalization.
**Warning signs:** Many unresolved file references despite the target files existing.

### Pitfall 5: Edge Kind Keyword Proximity

**What goes wrong:** Keywords like "extends" or "based on" appear on a different line from any label reference, creating incorrect DependsOn edges.
**Why it happens:** Body-text edge kind inference requires proximity decisions.
**How to avoid:** Per CONTEXT.md (Claude's discretion), start with same-line proximity and test against Murail corpus. If too restrictive, expand to same-paragraph. The keyword must be on the same line as the reference for the edge kind to override the default Cites.
**Warning signs:** Too many DependsOn edges (noisy) or too few (keywords missed). Test both approaches against the corpus.

### Pitfall 6: Clippy Pedantic Strictness

**What goes wrong:** Code that compiles and works correctly is rejected by clippy pedantic lints.
**Why it happens:** The project denies clippy::all and clippy::pedantic at the workspace level.
**How to avoid:** Write idiomatic Rust from the start. Common pedantic issues: missing `#[must_use]` (allowed in config), `module_name_repetitions` (allowed), `doc_markdown` (allowed). Use `pub(crate)` visibility where appropriate. Add `#[allow(clippy::...)]` with justification for truly noisy cases, but the Cargo.toml already configures the common allows.
**Warning signs:** `just check` fails on clippy even though tests pass.

### Pitfall 7: Large File Performance

**What goes wrong:** The scanner reads every line of every file. Some files in Murail are very large (formal model v17 is thousands of lines).
**Why it happens:** RegexSet is fast per-line, but total work scales with total lines.
**How to avoid:** RegexSet already optimizes the fast path (most lines match 0 patterns, costing only one automaton pass). Read files with `std::fs::read_to_string` (single allocation). Don't allocate per-line unless a match is found. The 100ms target for 283 files should be easily achievable.
**Warning signs:** Parsing takes >100ms. Profile with `--release` before optimizing.

### Pitfall 8: serde_yaml_ng Value Type Handling

**What goes wrong:** YAML values that look like numbers or booleans get deserialized as the wrong type. E.g., `status: 1.0` becomes a float, not a string.
**Why it happens:** YAML has implicit type coercion.
**How to avoid:** Deserialize frontmatter as `serde_yaml_ng::Value` or `HashMap<String, serde_yaml_ng::Value>` first, then extract specific fields with type checking. The `status` field should always be treated as a string. Use `.as_str()` or convert explicitly.
**Warning signs:** Status values like `v1` being parsed as something unexpected.

## Code Examples

### Complete Graph Construction Flow

```rust
// Source: spec section 5.1 (KB-D6) + section 15.3

// 1. Walk directory, create File handles
for entry in WalkDir::new(&root).into_iter().filter_entry(|e| !is_excluded(e)) {
    let entry = entry?;
    if entry.path().extension() == Some("md") {
        let path = Utf8PathBuf::try_from(entry.into_path())?;
        let content = std::fs::read_to_string(&path)?;
        let (frontmatter, body) = split_frontmatter(&content);

        // 2. Parse frontmatter -> status + metadata
        let metadata = if let Some(yaml) = frontmatter {
            parse_frontmatter(yaml)?
        } else {
            Metadata::default()
        };

        // 3. Create File handle with status
        let file_id = graph.add_node(Handle::file(path.clone(), metadata.status));

        // 4. Scan body for edges and handles
        scan_content(&body, file_id, &path, &mut graph, &config)?;
    }
}

// 5. Infer namespaces from collected label candidates
let namespaces = infer_namespaces(&graph, &config);

// 6. Filter labels by confirmed namespaces
resolve_handles(&mut graph, &namespaces, &config)?;
```

### Namespace Inference Algorithm

```rust
// Source: spec section 4.3 (KB-D4)

fn infer_namespaces(
    label_occurrences: &HashMap<String, Vec<(u32, Utf8PathBuf)>>,
    config: &AnnealConfig,
) -> HashSet<String> {
    let mut confirmed = HashSet::new();

    // Config overrides first
    for ns in &config.handles.confirmed {
        confirmed.insert(ns.clone());
    }

    let rejected: HashSet<_> = config.handles.rejected.iter().cloned().collect();

    for (prefix, occurrences) in label_occurrences {
        if rejected.contains(prefix) {
            continue;
        }
        if confirmed.contains(prefix) {
            continue; // already confirmed by config
        }

        // Count distinct sequential numbers
        let numbers: BTreeSet<u32> = occurrences.iter().map(|(n, _)| *n).collect();
        let distinct_files: HashSet<_> = occurrences.iter().map(|(_, f)| f).collect();

        // N >= 3 sequential members AND M >= 2 files
        if numbers.len() >= 3 && distinct_files.len() >= 2 {
            confirmed.insert(prefix.clone());
        }
    }

    confirmed
}
```

### Root Inference and Directory Exclusion

```rust
// Source: spec section 5.1 (KB-D20)

fn infer_root(cwd: &Utf8Path) -> Utf8PathBuf {
    if cwd.join(".design").is_dir() {
        cwd.join(".design")
    } else if cwd.join("docs").is_dir() {
        cwd.join("docs")
    } else {
        cwd.to_path_buf()
    }
}

const DEFAULT_EXCLUSIONS: &[&str] = &[
    ".git", ".planning", ".anneal",
    "target", "node_modules", ".build",
];

fn is_excluded(entry: &walkdir::DirEntry) -> bool {
    let name = entry.file_name().to_str().unwrap_or("");
    if entry.file_type().is_dir() {
        // Exclude defaults
        if DEFAULT_EXCLUSIONS.contains(&name) {
            return true;
        }
        // Exclude hidden directories (but not root if root starts with .)
        if name.starts_with('.') && name != ".design" {
            return true;
        }
    }
    false
}
```

### Active/Terminal Partition

```rust
// Source: spec section 6.2-6.3 (KB-D9, KB-D10)

fn partition_states(
    observed_statuses: &HashSet<String>,
    config: &AnnealConfig,
    file_directories: &HashMap<String, Utf8PathBuf>,
) -> (HashSet<String>, HashSet<String>) {
    let mut active = HashSet::new();
    let mut terminal = HashSet::new();

    // Config overrides
    for s in &config.convergence.active {
        active.insert(s.clone());
    }
    for s in &config.convergence.terminal {
        terminal.insert(s.clone());
    }

    // Directory convention: files in archive/, history/, prior/ are terminal
    let terminal_dirs = ["archive", "history", "prior"];

    for status in observed_statuses {
        if active.contains(status) || terminal.contains(status) {
            continue; // already classified by config
        }
        // Check if this status appears only in terminal directories
        // (heuristic, per spec KB-D9)
        // Default: treat as active unless evidence suggests terminal
        active.insert(status.clone());
    }

    (active, terminal)
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `serde_yaml` | `serde_yaml_ng` | 2024 (serde_yaml archived) | Must use serde_yaml_ng -- serde_yaml is unmaintained |
| `lazy_static!` / `once_cell` | `std::sync::LazyLock` | Rust 1.80 (stable) | Use LazyLock from std, no external lazy_static crate needed |
| Rust edition 2021 | Rust edition 2024 | Rust 1.85+ | Edition 2024 is already configured; affects some syntax and defaults |
| `OsString` path handling | `camino::Utf8Path` | N/A | Camino provides ergonomic UTF-8 paths; already a dependency |

**Deprecated/outdated:**
- `serde_yaml`: Archived, use `serde_yaml_ng` (already in Cargo.toml)
- `lazy_static!`: Superseded by `std::sync::LazyLock` (stable since Rust 1.80)
- `once_cell::sync::Lazy`: Superseded by `std::sync::LazyLock`

## Open Questions

1. **Keyword proximity for body-text edge inference**
   - What we know: D-01 requires keywords like "incorporates", "builds on" to infer DependsOn edges from body text
   - What's unclear: Whether same-line or same-paragraph is the right proximity rule
   - Recommendation: Start with same-line (simplest, most precise). Test against Murail corpus. Expand to paragraph if too restrictive. This is explicitly Claude's discretion per CONTEXT.md.

2. **Version handle discovery scope**
   - What we know: `v\d+` in "versioned context" should create Version handles
   - What's unclear: What precisely constitutes "versioned context" beyond filenames like `*-v17.md`
   - Recommendation: Match `v\d+` only in file names and in frontmatter `version:` fields. Body text `v17` is too noisy (Murail has 861 matches for `v1` alone). The primary signal is the filename pattern `*-vN.md` and the frontmatter `version:` field.

3. **Section reference (paragraph sign) ambiguity**
   - What we know: `§14` is ambiguous across documents (spec KB-OQ2). Bare section refs resolve within current file.
   - What's unclear: Whether to create edges for unresolvable cross-doc section refs in Phase 1
   - Recommendation: Create the edge with the section ref as target identity. Resolution will fail if ambiguous. In Phase 1, unresolvable section refs should be noted but not error (Phase 2's CHECK-01 handles existence errors). The data model supports it; the diagnostic severity is a Phase 2 decision.

4. **Heading text normalization for Section handles**
   - What we know: Headings like `### §4.1 Handle Kinds [KB-D2]` need parsing into a clean identity
   - What's unclear: Exact normalization rules (strip label suffixes? keep section numbers?)
   - Recommendation: Store the full heading text as-is for display. For identity/matching, strip leading `#` and whitespace. Section refs like `§4.1` match against the heading's `§N.N` component if present.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust | Everything | Yes | 1.94.0 | -- |
| Cargo | Build | Yes | 1.94.0 | -- |
| just | Quality gate | Yes | 1.47.1 | -- |
| Murail corpus | Integration testing | Yes | 283 .md files at ~/code/murail/.design/ | -- |

**Missing dependencies with no fallback:** None.

**Missing dependencies with fallback:** None.

## Corpus Analysis (Murail .design/)

Key statistics from probing the test corpus -- the planner should use these for task verification criteria:

| Metric | Value |
|--------|-------|
| Total .md files | 283 |
| Distinct status values | ~25 (raw, digested, decided, formal, verified, living, superseded, archived, etc.) |
| Label namespaces (real) | 15+ (OQ, D, SR, DG, A, P, FM, TQ, LD, RQ, C, AL, DEF, DT, BR, TO, ...) |
| False positive prefixes | SHA (62), AVX (23+), GPL (17), CRC (14), GPT (1), UTF (2) |
| Total label references | ~5000+ across all files |
| Section refs (paragraph sign) | ~1300+ |
| File path refs (.md) | Hundreds |
| Frontmatter `supersedes:` fields | 5 files |
| Files with frontmatter | ~120 (majority of files) |
| Terminal directories | archive/, prior/, history/ at various depths |
| Body text edge-kind keywords | 146 files contain "incorporates", "builds on", "extends", etc. |

## Sources

### Primary (HIGH confidence)
- `.design/anneal-spec.md` -- sections 4, 5, 5.1, 6, 6.1-6.5, 13, 14, 15, 15.1-15.3 (authoritative spec)
- `.planning/phases/01-graph-foundation/01-CONTEXT.md` -- user decisions D-01, D-02
- `.planning/REQUIREMENTS.md` -- 18 phase requirements (GRAPH-01..06, HANDLE-01..06, LATTICE-01..04, CONFIG-01..02)
- `Cargo.toml` -- verified dependency declarations build cleanly on Rust 1.94.0
- `~/code/murail/.design/` -- live corpus probed for actual label counts, status values, directory structure

### Secondary (MEDIUM confidence)
- Rust std library documentation for `std::sync::LazyLock` (stable since 1.80, confirmed available on 1.94.0)
- regex crate `RegexSet` API (well-established, used per spec recommendation)

### Tertiary (LOW confidence)
- None. All findings are from the authoritative spec or direct corpus probing.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all 10 crates already declared and verified building on Rust 1.94.0
- Architecture: HIGH -- spec is highly prescriptive with exact type definitions and patterns
- Pitfalls: HIGH -- identified from direct corpus analysis and known Rust/markdown edge cases

**Research date:** 2026-03-28
**Valid until:** Indefinite -- the spec is stable and all dependencies are locked
