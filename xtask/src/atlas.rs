//! The atlas — a derived whole-system name and module map.
//!
//! Seven read-only verbs fold over the shared item scanner (`crate::scan`),
//! which owns the one line grammar and the one test boundary. `census` walks
//! the aggregation ladder
//! (workspace → crate → directory → module); `name` renders one name's card
//! (locations, doc summary, re-export façades, co-mention edges); `module`
//! renders one module's card (header, per-item census,
//! use-edge fan-in/out, orphans, `--graph` edges); `dump` emits the complete
//! item and relationship index;
//! `comments --audit` mechanically checks the standing comment policy.
//! Relationships are declaration-level co-mention edges: the captured blocks
//! scanned for the captured type
//! names — the type graph without a parser, approximate by design. The atlas
//! owns no facts and never writes: every run recomputes from the tree, so
//! stale means re-run. Output is dense, deterministic (rows sorted by path,
//! then name), greppable, and budgeted to roughly a screenful per ladder rung;
//! `--json` emits the same data for machines.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use cargo_metadata::MetadataCommand;
use serde::Serialize;
use serde_json::json;

use crate::scan::{self, Item, ItemKind, Visibility};

type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "\
usage: cargo xtask atlas <verb> [--json] [--pub-only]
  atlas census [CRATE|PATH]      aggregation ladder: workspace -> crate -> directory -> module
  atlas name <Name|path::Name>   one name's card: locations, docs, facades, refs, edges
  atlas module <PATH>            one module's card: header, items, fan-in/out, orphans
                                 [--items full item table] [--graph co-mention edges]
  atlas dump --json              complete derived item and relationship index
  atlas comments --audit         the standing comment-policy audit
  atlas concentration [CRATE]    per module: largest top-level block (depth) and
                                 top-level item count (breadth), ranked
  atlas diff <REF>               module shape moved against a git ref: Δloc, Δblock,
                                 Δitems";

#[derive(Clone, Copy, Default)]
struct AtlasFlags {
    json: bool,
    pub_only: bool,
    audit: bool,
    items: bool,
    graph: bool,
}

/// Run one atlas verb. Read-only: the atlas never writes a ledger.
pub(crate) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut flags = AtlasFlags::default();
    let mut positional = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--json" => flags.json = true,
            "--pub-only" => flags.pub_only = true,
            "--audit" => flags.audit = true,
            "--items" => flags.items = true,
            "--graph" => flags.graph = true,
            flag if flag.starts_with("--") => {
                return Err(format!("unknown atlas flag `{flag}`\n{USAGE}"));
            }
            _ => positional.push(arg),
        }
    }
    let mut positional = positional.into_iter();
    let verb = positional.next().ok_or_else(|| USAGE.to_owned())?;
    let argument = positional.next();
    if let Some(extra) = positional.next() {
        return Err(format!("unexpected atlas argument `{extra}`\n{USAGE}"));
    }
    validate_invocation(&verb, argument.as_deref(), flags)?;

    let filter = Filter {
        pub_only: flags.pub_only,
        include_private: verb == "dump" && !flags.pub_only,
    };
    let workspace = Workspace::load()?;
    let output = match (verb.as_str(), argument.as_deref()) {
        ("census", key) => {
            let graph = build_graph(&workspace, filter);
            census(&workspace, key, &graph, filter, flags.json)?
        }
        ("name", Some(query)) => {
            let graph = build_graph(&workspace, filter);
            name_card(&workspace, query, &graph, filter, flags.json)?
        }
        ("module", Some(path)) => {
            let graph = build_graph(&workspace, filter);
            module_card(
                &workspace,
                path,
                &graph,
                filter,
                flags.items,
                flags.graph,
                flags.json,
            )?
        }
        ("dump", None) => {
            let graph = build_graph(&workspace, filter);
            dump(&workspace, &graph, filter)?
        }
        ("comments", None) => comments_audit(&workspace, filter, flags.json)?,
        ("concentration", key) => concentration(&workspace, key, flags.json)?,
        ("diff", Some(reference)) => diff(&workspace, reference, flags.json)?,
        _ => unreachable!("validate_invocation accepts only complete command shapes"),
    };
    println!("{output}");
    Ok(())
}

fn validate_invocation(verb: &str, argument: Option<&str>, flags: AtlasFlags) -> Result<()> {
    let known = matches!(
        verb,
        "census" | "name" | "module" | "dump" | "comments" | "concentration" | "diff"
    );
    if !known {
        return Err(format!("unknown atlas verb `{verb}`\n{USAGE}"));
    }
    if flags.audit && verb != "comments" {
        return Err(format!("atlas {verb} does not accept --audit\n{USAGE}"));
    }
    if flags.items && verb != "module" {
        return Err(format!("atlas {verb} does not accept --items\n{USAGE}"));
    }
    if flags.graph && verb != "module" {
        return Err(format!("atlas {verb} does not accept --graph\n{USAGE}"));
    }
    if flags.pub_only && matches!(verb, "concentration" | "diff") {
        return Err(format!("atlas {verb} does not accept --pub-only\n{USAGE}"));
    }

    match (verb, argument) {
        ("name", None) => Err(format!("atlas name requires <Name|path::Name>\n{USAGE}")),
        ("module", None) => Err(format!("atlas module requires <PATH>\n{USAGE}")),
        ("dump", Some(_)) => Err(format!("atlas dump takes no argument\n{USAGE}")),
        ("dump", None) if !flags.json => Err(format!("atlas dump requires --json\n{USAGE}")),
        ("comments", Some(_)) => Err(format!("atlas comments takes no argument\n{USAGE}")),
        ("comments", None) if !flags.audit => {
            Err(format!("atlas comments requires --audit\n{USAGE}"))
        }
        ("diff", None) => Err(format!("atlas diff requires <REF>\n{USAGE}")),
        _ => Ok(()),
    }
}

// ── the scanned workspace ──────────────────────────────────────────────────

/// Every member crate's `src` tree, scanned once per run through the one item
/// grammar. Crates are sorted by name; files by path — the sort order of every
/// atlas view.
struct Workspace {
    root: PathBuf,
    crates: Vec<CrateMap>,
}

struct CrateMap {
    name: String,
    /// Workspace-relative `src` directory, e.g. `crates/anneal-core/src`.
    src_rel: String,
    files: Vec<FileMap>,
}

/// One Rust file and every scanner-owned view Atlas derives from it.
struct FileMap {
    /// Workspace-relative path with `/` separators — the atlas's module key.
    rel: String,
    items: Vec<Item>,
    regions: scan::TestRegions,
    projection: scan::RustSourceProjection,
    /// Whether the whole file is test code, because a sibling declares it
    /// under a test-implying gate (`#[cfg(test)] mod fixtures;`). A file
    /// cannot know this about itself; `Workspace::load` resolves it.
    test_file: bool,
}

impl FileMap {
    fn load(root: &Path, path: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
        Ok(Self::of(rel_display(root, path), text))
    }

    fn of(rel: String, text: String) -> Self {
        let projection = scan::RustSourceProjection::of(text);
        let items = projection.items(Path::new(&rel));
        let regions = projection.test_regions();
        Self {
            rel,
            items,
            regions,
            projection,
            test_file: false,
        }
    }

    /// Lines outside every test region — zero for a whole test file.
    fn nontest_loc(&self) -> usize {
        if self.test_file {
            0
        } else {
            self.regions.non_test_loc()
        }
    }

    /// Items on the production side of the one test boundary
    /// (`scan::TestRegions`) — the only population the atlas counts.
    fn production_items(&self) -> impl Iterator<Item = &Item> {
        self.items
            .iter()
            .filter(|item| !self.test_file && !self.regions.covers(item.line))
    }

    /// Lexically projected lines outside every test-only region.
    fn production_code_lines(&self) -> impl Iterator<Item = (usize, &str)> {
        self.projection
            .code_lines()
            .enumerate()
            .filter(|(index, _)| !self.test_file && !self.regions.covers(index + 1))
            .map(|(index, line)| (index + 1, line))
    }
}

impl Workspace {
    fn load() -> Result<Self> {
        let metadata = MetadataCommand::new()
            .no_deps()
            .exec()
            .map_err(|err| format!("cargo metadata failed: {err}"))?;
        let root = metadata.workspace_root.as_std_path().to_path_buf();
        let mut packages: Vec<_> = metadata.workspace_packages().into_iter().collect();
        packages.sort_by(|a, b| a.name.cmp(&b.name));

        let mut crates = Vec::new();
        for package in packages {
            let src = package
                .manifest_path
                .as_std_path()
                .parent()
                .ok_or_else(|| format!("manifest without a parent: {}", package.manifest_path))?
                .join("src");
            let mut paths = Vec::new();
            collect_rs_files(&src, &mut paths)?;
            paths.sort();
            let mut files = paths
                .iter()
                .map(|path| FileMap::load(&root, path))
                .collect::<Result<Vec<_>>>()?;
            mark_test_files(&mut files);
            crates.push(CrateMap {
                name: package.name.to_string(),
                src_rel: rel_display(&root, &src),
                files,
            });
        }
        Ok(Self { root, crates })
    }

    /// The same workspace as it stood at a git ref, scanned through the same
    /// grammar. Membership comes from the *current* manifest (a crate added
    /// since the ref has no `before` side, which is exactly how it should
    /// read), so this reuses cargo's own answer rather than a path pattern.
    fn at_ref(&self, reference: &str) -> Result<Self> {
        let listing = git(&self.root, &["ls-tree", "-r", "--name-only", reference])?;
        let mut crates = Vec::new();
        for crate_map in &self.crates {
            let prefix = format!("{}/", crate_map.src_rel);
            let mut files = listing
                .lines()
                .filter(|path| path.starts_with(&prefix) && is_rust_source(Path::new(path)))
                .map(|path| {
                    let text = git(&self.root, &["show", &format!("{reference}:{path}")])?;
                    Ok(FileMap::of(path.to_owned(), text))
                })
                .collect::<Result<Vec<_>>>()?;
            mark_test_files(&mut files);
            crates.push(CrateMap {
                name: crate_map.name.clone(),
                src_rel: crate_map.src_rel.clone(),
                files,
            });
        }
        Ok(Self {
            root: self.root.clone(),
            crates,
        })
    }

    fn crate_named(&self, name: &str) -> Option<&CrateMap> {
        self.crates.iter().find(|crate_map| crate_map.name == name)
    }

    fn file(&self, rel: &str) -> Option<(&CrateMap, &FileMap)> {
        self.crates.iter().find_map(|crate_map| {
            crate_map
                .files
                .iter()
                .find(|file| file.rel == rel)
                .map(|file| (crate_map, file))
        })
    }

    fn files(&self) -> impl Iterator<Item = &FileMap> {
        self.crates
            .iter()
            .flat_map(|crate_map| crate_map.files.iter())
    }
}

/// Where a module file's child modules live, per Rust's own layout rule:
/// beside a crate or directory root (`lib.rs`, `main.rs`, `mod.rs`), and in
/// the matching `foo/` directory for any other `foo.rs`.
fn child_directory(rel: &str) -> String {
    let (directory, file) = rel.rsplit_once('/').unwrap_or(("", rel));
    match file {
        "lib.rs" | "main.rs" | "mod.rs" => directory.to_owned(),
        _ => format!("{directory}/{}", file.trim_end_matches(".rs")),
    }
}

/// Read-only git plumbing for `atlas diff` — one command, trimmed stdout.
fn git(root: &Path, arguments: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(arguments)
        .output()
        .map_err(|err| format!("git {}: {err}", arguments.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|err| format!("git output not UTF-8: {err}"))
}

/// Resolve `#[cfg(test)] mod name;` declarations against the crate's files:
/// the declared module's whole file is test code. Derived from the source's
/// own module graph rather than from filename conventions, so a fixture file
/// counts as tests wherever it lives and whatever it is called.
fn mark_test_files(files: &mut [FileMap]) {
    let declared: BTreeSet<String> = files
        .iter()
        .flat_map(|file| {
            let directory = child_directory(&file.rel);
            file.projection
                .test_module_declarations()
                .into_iter()
                .flat_map(move |name| {
                    [
                        format!("{directory}/{name}.rs"),
                        format!("{directory}/{name}/mod.rs"),
                    ]
                })
        })
        .collect();
    for file in files {
        file.test_file = declared.contains(&file.rel);
    }
}

/// The scanned-source test, shared by the disk walk and the git listing so
/// both sides of `atlas diff` admit exactly the same files.
fn is_rust_source(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
}

fn collect_rs_files(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let entries =
        fs::read_dir(dir).map_err(|err| format!("read source dir {}: {err}", dir.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|err| format!("read source entry in {}: {err}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("read file type {}: {err}", path.display()))?;
        if file_type.is_dir() {
            collect_rs_files(&path, paths)?;
        } else if is_rust_source(&path) {
            paths.push(path);
        }
    }
    Ok(())
}

fn rel_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

// ── the census policy ──────────────────────────────────────────────────────

/// The one population filter: pub-ish (`pub` + `pub(crate)` + `pub(super)`)
/// by default, strict `pub` under `--pub-only`.
#[derive(Clone, Copy)]
struct Filter {
    pub_only: bool,
    include_private: bool,
}

impl Filter {
    fn admits(self, item: &Item) -> bool {
        if self.pub_only {
            matches!(item.visibility, Visibility::Pub)
        } else if self.include_private {
            true
        } else {
            !matches!(item.visibility, Visibility::Private)
        }
    }

    const fn label(self) -> &'static str {
        if self.pub_only {
            "pub"
        } else if self.include_private {
            "all"
        } else {
            "pub-ish"
        }
    }
}

/// Census rows are types and fns; `pub use` and `macro_rules!` are
/// recorded beside them as aliases/macros, never as rows.
const fn is_type_kind(kind: ItemKind) -> bool {
    matches!(
        kind,
        ItemKind::Struct | ItemKind::Enum | ItemKind::Trait | ItemKind::TypeAlias
    )
}

const fn is_census_kind(kind: ItemKind) -> bool {
    is_type_kind(kind) || matches!(kind, ItemKind::Fn)
}

/// Every scanner-owned declaration that can be the endpoint of a re-export.
const fn is_dump_kind(kind: ItemKind) -> bool {
    !matches!(kind, ItemKind::PubUse | ItemKind::Impl)
}

/// One aggregated census cell — the fold every ladder rung shares.
#[derive(Default, Clone, Copy)]
struct Tally {
    types: usize,
    fns: usize,
    aliases: usize,
    macros: usize,
    documented: usize,
    nontest_loc: usize,
}

impl Tally {
    fn add_file(&mut self, file: &FileMap, filter: Filter) {
        self.nontest_loc += file.nontest_loc();
        for item in file.production_items() {
            match item.kind {
                ItemKind::MacroRules => self.macros += 1,
                ItemKind::PubUse if filter.admits(item) => self.aliases += 1,
                kind if is_census_kind(kind) && filter.admits(item) => {
                    if matches!(kind, ItemKind::Fn) {
                        self.fns += 1;
                    } else {
                        self.types += 1;
                    }
                    if item.doc.is_some() {
                        self.documented += 1;
                    }
                }
                _ => {}
            }
        }
    }

    fn absorb(&mut self, other: Self) {
        self.types += other.types;
        self.fns += other.fns;
        self.aliases += other.aliases;
        self.macros += other.macros;
        self.documented += other.documented;
        self.nontest_loc += other.nontest_loc;
    }

    const fn census_items(&self) -> usize {
        self.types + self.fns
    }

    /// Doc coverage in whole percent; `None` when the rung has no census rows.
    fn doc_pct(&self) -> Option<usize> {
        (self.census_items() > 0).then(|| self.documented * 100 / self.census_items())
    }
}

// ── census: the aggregation ladder ─────────────────────────────────────────

fn census(
    ws: &Workspace,
    key: Option<&str>,
    graph: &Graph,
    filter: Filter,
    json: bool,
) -> Result<String> {
    let key = key.map(|k| k.trim_end_matches('/'));
    let key_column = if key.is_none() { "crate" } else { "module" };
    let (scope, rows) = match key {
        None => (
            "workspace (per crate)".to_owned(),
            ws.crates
                .iter()
                .map(|crate_map| {
                    let mut tally = Tally::default();
                    for file in &crate_map.files {
                        tally.add_file(file, filter);
                    }
                    (crate_map.name.clone(), tally)
                })
                .collect::<Vec<_>>(),
        ),
        Some(k) => {
            if let Some(crate_map) = ws.crate_named(k) {
                (
                    format!("{k} (per module under {}/)", crate_map.src_rel),
                    dir_rows(ws, &crate_map.src_rel, filter),
                )
            } else if ws.file(k).is_some() {
                // A module path is the ladder's deepest rung: the module card.
                return module_card(ws, k, graph, filter, false, false, json);
            } else if ws
                .files()
                .any(|file| file.rel.starts_with(&format!("{k}/")))
            {
                (format!("{k}/ (per child)"), dir_rows(ws, k, filter))
            } else {
                return Err(format!(
                    "`{k}` is neither a workspace crate, a source directory, nor a module path"
                ));
            }
        }
    };

    let mut total = Tally::default();
    for (_, tally) in &rows {
        total.absorb(*tally);
    }

    if json {
        let json_rows: Vec<_> = rows
            .iter()
            .map(|(row_key, tally)| tally_json(row_key, tally))
            .collect();
        return pretty(&json!({
            "verb": "census",
            "scope": scope,
            "population": filter.label(),
            "rows": json_rows,
            "total": tally_json("TOTAL", &total),
        }));
    }

    let mut cells: Vec<Vec<String>> = rows
        .iter()
        .map(|(row_key, tally)| tally_cells(row_key, tally))
        .collect();
    cells.push(tally_cells("TOTAL", &total));
    Ok(format!(
        "atlas census — {scope} · population: {} · loc: non-test\n{}",
        filter.label(),
        render_table(
            &[key_column, "types", "fns", "alias", "macro", "loc", "doc%"],
            &cells,
            1,
        )
    ))
}

/// Rows for one directory rung: each child directory aggregated to one row
/// (`name/`), each child file one row — never more than a directory's fanout.
fn dir_rows(ws: &Workspace, prefix: &str, filter: Filter) -> Vec<(String, Tally)> {
    let mut children: BTreeMap<String, Tally> = BTreeMap::new();
    for file in ws.files() {
        let Some(rest) = file
            .rel
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('/'))
        else {
            continue;
        };
        let key = match rest.split_once('/') {
            Some((segment, _)) => format!("{segment}/"),
            None => rest.to_owned(),
        };
        children.entry(key).or_default().add_file(file, filter);
    }
    children.into_iter().collect()
}

fn tally_cells(key: &str, tally: &Tally) -> Vec<String> {
    vec![
        key.to_owned(),
        tally.types.to_string(),
        tally.fns.to_string(),
        tally.aliases.to_string(),
        tally.macros.to_string(),
        tally.nontest_loc.to_string(),
        tally
            .doc_pct()
            .map_or_else(|| "-".to_owned(), |pct| format!("{pct}%")),
    ]
}

fn tally_json(key: &str, tally: &Tally) -> serde_json::Value {
    json!({
        "key": key,
        "types": tally.types,
        "fns": tally.fns,
        "aliases": tally.aliases,
        "macros": tally.macros,
        "nontest_loc": tally.nontest_loc,
        "documented": tally.documented,
        "doc_pct": tally.doc_pct(),
    })
}

// ── co-mention edges: the type graph without a parser ───────────────────────

/// How one declaration mentions a type: a fn's parameter side `consumes`, a
/// fn's return side `produces`, a struct/enum body `composes` (the mentioned
/// type is a component of the source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EdgeClass {
    Consumes,
    Produces,
    Composes,
}

const fn class_label(class: EdgeClass) -> &'static str {
    match class {
        EdgeClass::Consumes => "consumes",
        EdgeClass::Produces => "produces",
        EdgeClass::Composes => "composes",
    }
}

/// One declaration-level co-mention edge: a fn/struct/enum `source` mentions
/// a captured struct/enum/trait name in its declaration block. Textual and
/// identifier-bounded over the captured blocks — approximate by design
/// (bodies excluded: signatures are the intentional interface).
struct Edge {
    source_file: String,
    source_line: usize,
    source_name: String,
    class: EdgeClass,
    target: String,
    /// The declaration site the name resolves to (same-crate declarations
    /// preferred over cross-crate ones); `None` when ambiguous.
    target_file: Option<String>,
    target_line: Option<usize>,
    /// The preferred candidate set still holds more than one declaration —
    /// a same-name type in several crates that text alone cannot pick apart.
    ambiguous: bool,
}

/// The whole-workspace co-mention graph plus its orphan strip.
struct Graph {
    edges: Vec<Edge>,
    /// Module → census-population type names no *other* declaration block
    /// mentions anywhere in the workspace (name-sorted per module).
    orphans: BTreeMap<String, Vec<String>>,
}

enum UniqueCandidate<'a, T> {
    None,
    One(&'a T),
    Multiple,
}

fn unique_candidate<'a, T>(candidates: impl Iterator<Item = &'a T>) -> UniqueCandidate<'a, T> {
    let mut found = None;
    for candidate in candidates {
        if found.is_some() {
            return UniqueCandidate::Multiple;
        }
        found = Some(candidate);
    }
    found.map_or(UniqueCandidate::None, UniqueCandidate::One)
}

fn unique_local_candidate<T>(
    candidates: &[T],
    is_local: impl Fn(&T) -> bool,
) -> UniqueCandidate<'_, T> {
    match unique_candidate(candidates.iter().filter(|candidate| is_local(candidate))) {
        UniqueCandidate::None => unique_candidate(candidates.iter()),
        found => found,
    }
}

/// One pass over every captured declaration block, looking up each
/// identifier token in the census-population type registry. Re-exports are
/// aliases, not uses: a `pub use` neither rescues a type from orphanhood nor
/// forms an edge. Self-mentions (an item named like the type — its own
/// declaration, its impl blocks) never count as mentions.
fn build_graph(ws: &Workspace, filter: Filter) -> Graph {
    struct Decl<'ws> {
        crate_index: usize,
        file: &'ws str,
        line: usize,
    }
    let mut registry: BTreeMap<&str, Vec<Decl<'_>>> = BTreeMap::new();
    let workspace_roots = ws
        .crates
        .iter()
        .map(|crate_map| crate_map.name.replace('-', "_"))
        .collect::<BTreeSet<_>>();
    for (crate_index, crate_map) in ws.crates.iter().enumerate() {
        for file in &crate_map.files {
            for item in file.production_items() {
                let is_type = matches!(
                    item.kind,
                    ItemKind::Struct | ItemKind::Enum | ItemKind::Trait
                );
                if is_type && filter.admits(item) {
                    registry.entry(item.name.as_str()).or_default().push(Decl {
                        crate_index,
                        file: file.rel.as_str(),
                        line: item.line,
                    });
                }
            }
        }
    }

    let mut mentioned: BTreeSet<&str> = BTreeSet::new();
    let mut edges = Vec::new();
    for (crate_index, crate_map) in ws.crates.iter().enumerate() {
        for file in &crate_map.files {
            let external_bindings = external_import_bindings(file, &workspace_roots);
            for item in file.production_items() {
                if item.kind == ItemKind::PubUse {
                    continue;
                }
                let regions: Vec<(&str, EdgeClass)> = match item.kind {
                    ItemKind::Fn => {
                        let (params, ret) = split_signature(&item.declaration_block);
                        vec![(params, EdgeClass::Consumes), (ret, EdgeClass::Produces)]
                    }
                    _ => vec![(item.declaration_block.as_str(), EdgeClass::Composes)],
                };
                // Trait/impl/alias/const blocks rescue orphans but form no
                // classified edge — only fn signatures and struct/enum bodies
                // carry the consume/produce/compose meaning.
                let is_edge_source =
                    matches!(item.kind, ItemKind::Fn | ItemKind::Struct | ItemKind::Enum);
                let mut seen: BTreeSet<(&str, EdgeClass)> = BTreeSet::new();
                for (region, class) in regions {
                    for token in identifiers(region) {
                        if token == item.name || external_bindings.suppresses(token) {
                            continue;
                        }
                        let Some((name, decls)) = registry.get_key_value(token) else {
                            continue;
                        };
                        mentioned.insert(name);
                        if !is_edge_source || !seen.insert((name, class)) {
                            continue;
                        }
                        let resolution =
                            unique_local_candidate(decls, |decl| decl.crate_index == crate_index);
                        let (target_file, target_line, ambiguous) = match resolution {
                            UniqueCandidate::One(only) => {
                                (Some(only.file.to_owned()), Some(only.line), false)
                            }
                            UniqueCandidate::None => (None, None, false),
                            UniqueCandidate::Multiple => (None, None, true),
                        };
                        edges.push(Edge {
                            source_file: file.rel.clone(),
                            source_line: item.line,
                            source_name: item.name.clone(),
                            class,
                            target: (*name).to_owned(),
                            ambiguous,
                            target_file,
                            target_line,
                        });
                    }
                }
            }
        }
    }

    let mut orphans: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, decls) in &registry {
        if mentioned.contains(name) {
            continue;
        }
        for decl in decls {
            orphans
                .entry(decl.file.to_owned())
                .or_default()
                .push((*name).to_owned());
        }
    }
    // Registry iteration is name-sorted, so each module's orphan list is too.
    Graph { edges, orphans }
}

// ── dump: the complete derived base for the concept spine ────────────────

#[derive(Serialize)]
struct Dump {
    verb: &'static str,
    population: &'static str,
    items: Vec<DumpItem>,
    edges: Vec<DumpEdge>,
}

#[derive(Serialize)]
struct DumpItem {
    id: String,
    name: String,
    kind: &'static str,
    home: String,
    file: String,
    line: usize,
    vis: &'static str,
    r#pub: bool,
}

#[derive(Serialize)]
struct DumpEdge {
    source: String,
    source_path: Option<String>,
    source_id: Option<String>,
    target: String,
    target_id: Option<String>,
    target_file: Option<String>,
    ambiguous: bool,
    unresolved: bool,
    kind: &'static str,
    file: String,
    line: usize,
}

/// Serialize the workspace index already used by the cards. The dump is a
/// machine-readable projection of declarations and declaration-level
/// relationships, not a second stored authority.
fn dump(ws: &Workspace, graph: &Graph, filter: Filter) -> Result<String> {
    let mut items = Vec::new();
    let mut admitted_sites = BTreeSet::new();
    let mut declarations: BTreeMap<String, Vec<DumpDeclaration>> = BTreeMap::new();
    let mut pending_reexports = Vec::new();
    for (crate_index, crate_map) in ws.crates.iter().enumerate() {
        for file in &crate_map.files {
            let home = item_home(crate_map, file);
            for item in file.production_items().filter(|item| filter.admits(item)) {
                if item.kind == ItemKind::PubUse {
                    pending_reexports.push(PendingReexport {
                        crate_index,
                        file: file.rel.clone(),
                        home: home.clone(),
                        line: item.line,
                        visibility: item.visibility,
                        crate_root: module_path_of(&crate_map.src_rel, &file.rel).is_none(),
                        names: scan::import_bindings(&item.declaration_block),
                    });
                    continue;
                }
                if !is_dump_kind(item.kind) {
                    continue;
                }
                admitted_sites.insert((file.rel.as_str(), item.line, item.name.as_str()));
                let id = item_id(&file.rel, item.line, &item.name);
                declarations
                    .entry(item.name.clone())
                    .or_default()
                    .push(DumpDeclaration {
                        crate_index,
                        id: id.clone(),
                        qualified: qualified_item_path(&home, &item.name),
                    });
                items.push(DumpItem {
                    id,
                    name: item.name.clone(),
                    kind: kind_label(item.kind),
                    home: home.clone(),
                    file: file.rel.clone(),
                    line: item.line,
                    vis: dump_visibility_label(item.visibility),
                    r#pub: matches!(item.visibility, Visibility::Pub),
                });
            }
        }
    }

    let mut edges = Vec::new();
    for edge in &graph.edges {
        if !admitted_sites.contains(&(
            edge.source_file.as_str(),
            edge.source_line,
            edge.source_name.as_str(),
        )) {
            continue;
        }
        edges.push(DumpEdge {
            source: edge.source_name.clone(),
            source_path: None,
            source_id: Some(item_id(
                &edge.source_file,
                edge.source_line,
                &edge.source_name,
            )),
            target: edge.target.clone(),
            target_id: edge
                .target_file
                .as_ref()
                .zip(edge.target_line)
                .map(|(target_file, target_line)| item_id(target_file, target_line, &edge.target)),
            target_file: edge.target_file.clone(),
            ambiguous: edge.ambiguous,
            unresolved: false,
            kind: match edge.class {
                EdgeClass::Consumes => "consumes",
                EdgeClass::Produces => "produces",
                EdgeClass::Composes => "composed-of",
            },
            file: edge.source_file.clone(),
            line: edge.source_line,
        });
    }

    for pending in pending_reexports {
        let kind = reexport_edge_kind(pending.visibility, pending.crate_root);
        for name in pending.names {
            let target_id = item_id(&pending.file, pending.line, &name.local_name);
            items.push(DumpItem {
                id: target_id.clone(),
                name: name.local_name.clone(),
                kind: "re-export",
                home: pending.home.clone(),
                file: pending.file.clone(),
                line: pending.line,
                vis: dump_visibility_label(pending.visibility),
                r#pub: matches!(pending.visibility, Visibility::Pub),
            });
            let source = resolve_declaration(
                &declarations,
                pending.crate_index,
                &name.source,
                &name.source_path,
            );
            edges.push(DumpEdge {
                source: name.source,
                source_path: Some(name.source_path),
                source_id: source.id,
                target: name.local_name,
                target_id: Some(target_id),
                target_file: Some(pending.file.clone()),
                ambiguous: source.ambiguous,
                unresolved: source.unresolved,
                kind,
                file: pending.file.clone(),
                line: pending.line,
            });
        }
    }
    items.sort_by(|a, b| {
        (a.file.as_str(), a.line, a.name.as_str(), a.kind).cmp(&(
            b.file.as_str(),
            b.line,
            b.name.as_str(),
            b.kind,
        ))
    });
    edges.sort_by(|a, b| {
        (
            a.file.as_str(),
            a.line,
            a.source.as_str(),
            a.kind,
            a.target.as_str(),
        )
            .cmp(&(
                b.file.as_str(),
                b.line,
                b.source.as_str(),
                b.kind,
                b.target.as_str(),
            ))
    });

    let value = Dump {
        verb: "dump",
        population: filter.label(),
        items,
        edges,
    };
    let mut bytes = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"");
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
    value
        .serialize(&mut serializer)
        .map_err(|err| format!("serialize atlas dump: {err}"))?;
    String::from_utf8(bytes).map_err(|err| format!("encode atlas dump as UTF-8: {err}"))
}

struct DumpDeclaration {
    crate_index: usize,
    id: String,
    qualified: String,
}

struct PendingReexport {
    crate_index: usize,
    file: String,
    home: String,
    line: usize,
    visibility: Visibility,
    crate_root: bool,
    names: Vec<scan::ImportBinding>,
}

struct ResolvedDeclaration {
    id: Option<String>,
    ambiguous: bool,
    unresolved: bool,
}

fn resolve_declaration(
    declarations: &BTreeMap<String, Vec<DumpDeclaration>>,
    crate_index: usize,
    name: &str,
    source_path: &str,
) -> ResolvedDeclaration {
    let Some(candidates) = declarations.get(name) else {
        return ResolvedDeclaration {
            id: None,
            ambiguous: false,
            unresolved: true,
        };
    };
    if source_path.contains("::") {
        let suffix = normalized_source_suffix(source_path);
        match unique_candidate(
            candidates
                .iter()
                .filter(|candidate| candidate.qualified.ends_with(&suffix)),
        ) {
            UniqueCandidate::One(only) => {
                return ResolvedDeclaration {
                    id: Some(only.id.clone()),
                    ambiguous: false,
                    unresolved: false,
                };
            }
            UniqueCandidate::Multiple => {
                return ResolvedDeclaration {
                    id: None,
                    ambiguous: true,
                    unresolved: false,
                };
            }
            UniqueCandidate::None => {}
        }
    }
    match unique_local_candidate(candidates, |candidate| candidate.crate_index == crate_index) {
        UniqueCandidate::One(only) => ResolvedDeclaration {
            id: Some(only.id.clone()),
            ambiguous: false,
            unresolved: false,
        },
        UniqueCandidate::Multiple => ResolvedDeclaration {
            id: None,
            ambiguous: true,
            unresolved: false,
        },
        UniqueCandidate::None => ResolvedDeclaration {
            id: None,
            ambiguous: false,
            unresolved: true,
        },
    }
}

fn normalized_source_suffix(path: &str) -> String {
    path.trim_start_matches("::")
        .trim_start_matches("crate::")
        .trim_start_matches("self::")
        .trim_start_matches("super::")
        .replace('-', "_")
}

fn qualified_item_path(home: &str, name: &str) -> String {
    let home = home
        .strip_suffix("::lib")
        .or_else(|| home.strip_suffix("::main"))
        .unwrap_or(home)
        .replace('-', "_");
    format!("{home}::{name}")
}

fn item_id(file: &str, line: usize, name: &str) -> String {
    format!("{file}:{line}:{name}")
}

const fn reexport_edge_kind(visibility: Visibility, crate_root: bool) -> &'static str {
    if matches!(visibility, Visibility::Pub) && crate_root {
        "facade"
    } else {
        "re-exports"
    }
}

const fn dump_visibility_label(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Pub => "pub",
        Visibility::PubCrate => "crate",
        Visibility::PubSuper => "super",
        Visibility::Private => "private",
    }
}

/// The crate-qualified module that owns an item. Crate roots retain `lib` or
/// `main`, because those are distinct homes even though neither has a Rust
/// module-path segment.
fn item_home(crate_map: &CrateMap, file: &FileMap) -> String {
    let inside = file
        .rel
        .strip_prefix(&crate_map.src_rel)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(file.rel.as_str());
    let stem = inside.strip_suffix(".rs").unwrap_or(inside);
    let module = stem.strip_suffix("/mod").unwrap_or(stem).replace('/', "::");
    format!("{}::{module}", crate_map.name)
}

/// Split a fn signature block at the depth-0 `->` into (parameter side,
/// return side). Arrows inside generic bounds or nested fn types
/// (`F: Fn() -> T`) sit at bracket depth > 0 and are skipped.
fn split_signature(block: &str) -> (&str, &str) {
    let bytes = block.as_bytes();
    let mut paren = 0usize;
    let mut angle = 0usize;
    let mut previous = b' ';
    for (index, &byte) in bytes.iter().enumerate() {
        match byte {
            b'(' => paren += 1,
            b')' => paren = paren.saturating_sub(1),
            b'<' => angle += 1,
            b'>' if previous != b'-' => angle = angle.saturating_sub(1),
            b'-' if paren == 0 && angle == 0 && bytes.get(index + 1) == Some(&b'>') => {
                return (&block[..index], &block[index + 2..]);
            }
            _ => {}
        }
        previous = byte;
    }
    (block, "")
}

/// Identifier-bounded tokens of a declaration block — the mention grammar.
fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|ch: char| !(ch == '_' || ch.is_ascii_alphanumeric()))
        .filter(|token| !token.is_empty() && !token.starts_with(|ch: char| ch.is_ascii_digit()))
}

// ── name: one name's card ───────────────────────────────────────────────────

/// Edges shown per direction on a name card before "(+N more)", keeping the
/// card to roughly a screenful.
const NAME_EDGE_BUDGET: usize = 8;

fn name_card(
    ws: &Workspace,
    query: &str,
    graph: &Graph,
    filter: Filter,
    json: bool,
) -> Result<String> {
    let (fragment, name) = match query.rsplit_once("::") {
        Some((fragment, name)) => (Some(fragment.replace("::", "/")), name),
        None => (None, query),
    };
    let in_fragment = |rel: &str| {
        fragment.as_deref().is_none_or(|frag| {
            rel == frag
                || rel.ends_with(&format!("/{frag}"))
                || rel.ends_with(&format!("/{frag}.rs"))
        })
    };

    // Declarations: every named kind except re-exports (façades are listed
    // separately — the definition site is canonical).
    let mut shown: Vec<(&FileMap, &Item)> = Vec::new();
    let mut hidden = 0usize;
    let mut reexports: Vec<String> = Vec::new();
    let mut impls: Vec<String> = Vec::new();
    let mut defining: BTreeSet<(&str, usize)> = BTreeSet::new();
    for file in ws.files() {
        for item in file.production_items() {
            if item.kind == ItemKind::PubUse {
                if in_fragment(&file.rel)
                    && scan::import_bindings(&item.declaration_block)
                        .iter()
                        .any(|export| export.source == name || export.local_name == name)
                {
                    reexports.push(format!("{}:{}", file.rel, item.line));
                }
                continue;
            }
            if item.name != name || !in_fragment(&file.rel) {
                continue;
            }
            // Impl blocks carry the type's name but declare nothing new: they
            // are a strip on the card, not declaration matches.
            if item.kind == ItemKind::Impl {
                impls.push(format!("{}:{}", file.rel, item.line));
                continue;
            }
            defining.insert((file.rel.as_str(), item.line));
            if filter.admits(item) {
                shown.push((file, item));
            } else {
                hidden += 1;
            }
        }
    }
    if shown.is_empty() && hidden == 0 {
        return Err(format!(
            "no declaration of `{query}` in the atlas scope \
(macro-generated declarations are outside the line grammar — a known blind spot)"
        ));
    }
    shown.sort_by(|a, b| (a.0.rel.as_str(), a.1.line).cmp(&(b.0.rel.as_str(), b.1.line)));

    // The cheap reference count: textual, identifier-bounded, defining lines
    // excluded — approximate by design.
    let mut refs = 0usize;
    for file in ws.files() {
        for (line_number, line) in file.production_code_lines() {
            if defining.contains(&(file.rel.as_str(), line_number)) {
                continue;
            }
            refs += count_word_occurrences(line, name);
        }
    }

    // Co-mention edges. Incoming when the name is a type — who
    // produces, consumes, and composes it; outgoing when it is a fn — what
    // its own signature consumes and produces. `?` marks an ambiguous edge:
    // the name is declared in more than one candidate crate.
    let is_type_match = shown.iter().any(|(_, item)| {
        matches!(
            item.kind,
            ItemKind::Struct | ItemKind::Enum | ItemKind::Trait
        )
    });
    let is_fn_match = shown.iter().any(|(_, item)| item.kind == ItemKind::Fn);
    let incoming = |class: EdgeClass| -> Vec<&Edge> {
        let mut found: Vec<&Edge> = graph
            .edges
            .iter()
            .filter(|edge| edge.class == class && edge.target == name)
            .collect();
        found.sort_by(|a, b| {
            (
                a.source_file.as_str(),
                a.source_line,
                a.source_name.as_str(),
            )
                .cmp(&(
                    b.source_file.as_str(),
                    b.source_line,
                    b.source_name.as_str(),
                ))
        });
        found
    };
    let outgoing = |class: EdgeClass| -> Vec<&Edge> {
        let mut found: Vec<&Edge> = graph
            .edges
            .iter()
            .filter(|edge| {
                edge.class == class
                    && edge.source_name == name
                    && defining.contains(&(edge.source_file.as_str(), edge.source_line))
            })
            .collect();
        found.sort_by(|a, b| a.target.cmp(&b.target));
        found
    };
    let mark = |ambiguous: bool| if ambiguous { "?" } else { "" };
    let mut edge_sections: Vec<(&str, Vec<String>, Vec<&Edge>)> = Vec::new();
    if is_type_match {
        for (label, class) in [
            ("produced-by", EdgeClass::Produces),
            ("consumed-by", EdgeClass::Consumes),
            ("composes", EdgeClass::Composes),
        ] {
            let found = incoming(class);
            let entries = found
                .iter()
                .map(|edge| {
                    format!(
                        "{} {}:{}{}",
                        edge.source_name,
                        edge.source_file,
                        edge.source_line,
                        mark(edge.ambiguous)
                    )
                })
                .collect();
            edge_sections.push((label, entries, found));
        }
        let found = outgoing(EdgeClass::Composes);
        let entries = found
            .iter()
            .map(|edge| format!("{}{}", edge.target, mark(edge.ambiguous)))
            .collect();
        edge_sections.push(("composed-of", entries, found));
    }
    if is_fn_match {
        for (label, class) in [
            ("consumes", EdgeClass::Consumes),
            ("produces", EdgeClass::Produces),
        ] {
            let found = outgoing(class);
            let entries = found
                .iter()
                .map(|edge| format!("{}{}", edge.target, mark(edge.ambiguous)))
                .collect();
            edge_sections.push((label, entries, found));
        }
    }

    if json {
        let matches: Vec<_> = shown
            .iter()
            .map(|(file, item)| {
                json!({
                    "path": file.rel,
                    "line": item.line,
                    "kind": kind_label(item.kind),
                    "visibility": visibility_label(item.visibility),
                    "doc": item.doc,
                })
            })
            .collect();
        let edges: serde_json::Map<String, serde_json::Value> = edge_sections
            .iter()
            .map(|(label, _, found)| {
                let list: Vec<_> = found
                    .iter()
                    .map(|edge| {
                        json!({
                            "source": edge.source_name,
                            "path": edge.source_file,
                            "line": edge.source_line,
                            "target": edge.target,
                            "target_file": edge.target_file,
                            "target_line": edge.target_line,
                            "ambiguous": edge.ambiguous,
                        })
                    })
                    .collect();
                ((*label).replace('-', "_"), json!(list))
            })
            .collect();
        return pretty(&json!({
            "verb": "name",
            "query": query,
            "population": filter.label(),
            "matches": matches,
            "hidden": hidden,
            "reexports": reexports,
            "impls": impls,
            "edges": edges,
            "refs_approx": refs,
        }));
    }

    let mut lines = Vec::new();
    let hidden_note = if hidden > 0 {
        format!(
            " · {hidden} match(es) outside the {} population hidden",
            filter.label()
        )
    } else {
        String::new()
    };
    lines.push(format!(
        "atlas name — {name} · {} match(es) ({} population){hidden_note} · ~{refs} refs (approximate, textual)",
        shown.len(),
        filter.label()
    ));
    lines.push(if reexports.is_empty() {
        "reexports: (none)".to_owned()
    } else {
        format!("reexports: {}", reexports.join(" · "))
    });
    if !impls.is_empty() {
        lines.push(format!(
            "impl blocks ({}): {}",
            impls.len(),
            impls.join(" · ")
        ));
    }
    for (label, entries, _) in &edge_sections {
        if entries.is_empty() {
            lines.push(format!("{label}: (none)"));
            continue;
        }
        let total = entries.len();
        let more = if total > NAME_EDGE_BUDGET {
            format!(" (+{} more)", total - NAME_EDGE_BUDGET)
        } else {
            String::new()
        };
        lines.push(format!(
            "{label} ({total}): {}{more}",
            entries[..total.min(NAME_EDGE_BUDGET)].join(" · ")
        ));
    }
    for (file, item) in &shown {
        lines.push(format!(
            "{}:{}  {}  {}",
            file.rel,
            item.line,
            kind_label(item.kind),
            visibility_label(item.visibility),
        ));
        lines.push(format!(
            "    /// {}",
            item.doc.as_deref().unwrap_or("(MISSING — no doc comment)")
        ));
    }
    Ok(lines.join("\n"))
}

/// Occurrences of `name` delimited by non-identifier characters. Textual:
/// strings and comments count — the reference count is labeled approximate.
fn count_word_occurrences(text: &str, name: &str) -> usize {
    let bytes = text.as_bytes();
    text.match_indices(name)
        .filter(|(start, _)| {
            let end = start + name.len();
            let before_ok = *start == 0 || !is_ident_byte(bytes[start - 1]);
            let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
            before_ok && after_ok
        })
        .count()
}

const fn is_ident_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

// ── module: one module's card ──────────────────────────────────────────────

/// Per-item listings above this budget summarize by kind so each rung stays
/// near a screenful; `--items` restores the full table.
const MODULE_ITEM_BUDGET: usize = 30;

/// `--graph` edge lists above this budget summarize to per-class counts and
/// the most-mentioned targets.
const MODULE_GRAPH_BUDGET: usize = 40;

#[allow(
    clippy::fn_params_excessive_bools,
    reason = "the two bools mirror the two CLI flags (--items, --graph); a config struct would only rename them"
)]
fn module_card(
    ws: &Workspace,
    path: &str,
    graph: &Graph,
    filter: Filter,
    show_items: bool,
    show_graph: bool,
    json: bool,
) -> Result<String> {
    let path = path.trim_start_matches("./");
    let (crate_map, file) = ws.file(path).ok_or_else(|| {
        format!(
            "no workspace module at `{path}` (module keys are workspace-relative paths \
             like crates/<crate>/src/<module>.rs)"
        )
    })?;
    let header = file.projection.module_header_lines().first().cloned();
    let mut tally = Tally::default();
    tally.add_file(file, filter);
    let undocumented = file
        .production_items()
        .filter(|item| is_census_kind(item.kind) && filter.admits(item) && item.doc.is_none())
        .count();
    let (imports, internal) = fan_out(file);
    let fan_in_files = fan_in(crate_map, file);
    let orphans = graph
        .orphans
        .get(&file.rel)
        .map(Vec::as_slice)
        .unwrap_or_default();
    // The module's own co-mention edges, deterministically ordered.
    let mut module_edges: Vec<&Edge> = graph
        .edges
        .iter()
        .filter(|edge| edge.source_file == file.rel)
        .collect();
    module_edges.sort_by(|a, b| {
        (a.source_line, a.class, a.target.as_str()).cmp(&(
            b.source_line,
            b.class,
            b.target.as_str(),
        ))
    });

    if json {
        let items: Vec<_> = file
            .production_items()
            .map(|item| {
                json!({
                    "line": item.line,
                    "kind": kind_label(item.kind),
                    "visibility": visibility_label(item.visibility),
                    "name": item.name,
                    "doc": item.doc,
                })
            })
            .collect();
        let mut card = json!({
            "verb": "module",
            "path": file.rel,
            "crate": crate_map.name,
            "header": header,
            "nontest_loc": file.nontest_loc(),
            "census": tally_json(&file.rel, &tally),
            "undocumented": undocumented,
            "fan_out": {"imports": imports, "crate_internal": internal},
            "fan_in_files": fan_in_files,
            "orphans": orphans,
            "items": items,
        });
        if show_graph {
            let edges: Vec<_> = module_edges
                .iter()
                .map(|edge| {
                    json!({
                        "line": edge.source_line,
                        "source": edge.source_name,
                        "class": class_label(edge.class),
                        "target": edge.target,
                        "target_file": edge.target_file,
                        "target_line": edge.target_line,
                        "ambiguous": edge.ambiguous,
                    })
                })
                .collect();
            if let Some(map) = card.as_object_mut() {
                map.insert("graph".to_owned(), json!(edges));
            }
        }
        return pretty(&card);
    }

    let mut lines = Vec::new();
    lines.push(format!("atlas module — {} ({})", file.rel, crate_map.name));
    lines.push(header.map_or_else(
        || "//! MISSING — no module header".to_owned(),
        |first| format!("//! {first}"),
    ));
    lines.push(format!(
        "nontest-loc: {} · census ({}): {} types, {} fns, {} aliases, {} macros · undocumented {}: {undocumented}",
        file.nontest_loc(),
        filter.label(),
        tally.types,
        tally.fns,
        tally.aliases,
        tally.macros,
        filter.label(),
    ));
    let fan_in_text = fan_in_files.as_ref().map_or_else(
        || "- (crate root)".to_owned(),
        |files| {
            if files.is_empty() {
                "0 files".to_owned()
            } else {
                format!("{} files: {}", files.len(), files.join(" · "))
            }
        },
    );
    lines.push(format!(
        "fan-out: {imports} use-imports ({internal} crate-internal) · fan-in: {fan_in_text} [heuristic use-edges]"
    ));
    lines.push(if orphans.is_empty() {
        "orphans: 0".to_owned()
    } else {
        const ORPHAN_LIST_BUDGET: usize = 12;
        let more = if orphans.len() > ORPHAN_LIST_BUDGET {
            format!(" (+{} more)", orphans.len() - ORPHAN_LIST_BUDGET)
        } else {
            String::new()
        };
        format!(
            "orphans ({}): {}{more} [census types no other declaration mentions]",
            orphans.len(),
            orphans[..orphans.len().min(ORPHAN_LIST_BUDGET)].join(" · ")
        )
    });
    if show_graph {
        if module_edges.len() > MODULE_GRAPH_BUDGET {
            let mut by_class: BTreeMap<&str, usize> = BTreeMap::new();
            let mut by_target: BTreeMap<&str, usize> = BTreeMap::new();
            for edge in &module_edges {
                *by_class.entry(class_label(edge.class)).or_default() += 1;
                *by_target.entry(edge.target.as_str()).or_default() += 1;
            }
            let classes: Vec<String> = by_class
                .iter()
                .map(|(class, count)| format!("{class} {count}"))
                .collect();
            let mut targets: Vec<(&str, usize)> = by_target.into_iter().collect();
            targets.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
            targets.truncate(12);
            let targets: Vec<String> = targets
                .into_iter()
                .map(|(target, count)| format!("{target} ×{count}"))
                .collect();
            lines.push(format!(
                "graph: {} edges (over the {MODULE_GRAPH_BUDGET}-edge budget; summarized)",
                module_edges.len()
            ));
            lines.push(format!("  by class: {}", classes.join(" · ")));
            lines.push(format!("  top targets: {}", targets.join(" · ")));
        } else {
            lines.push(format!(
                "graph: {} edges (declaration-level co-mentions)",
                module_edges.len()
            ));
            for edge in &module_edges {
                let site = match (&edge.target_file, edge.ambiguous) {
                    (_, true) => " (ambiguous)".to_owned(),
                    (Some(target_file), _) if *target_file != file.rel => {
                        format!(" ({target_file})")
                    }
                    _ => String::new(),
                };
                lines.push(format!(
                    "  {} {} -{}-> {}{site}",
                    edge.source_line,
                    edge.source_name,
                    class_label(edge.class),
                    edge.target
                ));
            }
        }
    }
    let item_count = file.production_items().count();
    if !show_items && item_count > MODULE_ITEM_BUDGET {
        let mut kinds: std::collections::BTreeMap<&str, (usize, usize)> =
            std::collections::BTreeMap::new();
        for item in file.production_items() {
            let entry = kinds.entry(kind_label(item.kind)).or_default();
            entry.0 += 1;
            if item.doc.is_none() {
                entry.1 += 1;
            }
        }
        lines.push(format!(
            "items: {item_count} (over the {MODULE_ITEM_BUDGET}-item screenful budget; \
summarized by kind — pass --items for the full table)"
        ));
        for (kind, (count, undoc)) in kinds {
            lines.push(format!("  {kind:8} {count:4}   undocumented {undoc}"));
        }
        return Ok(lines.join("\n"));
    }
    lines.push("items:".to_owned());
    let rows: Vec<Vec<String>> = file
        .production_items()
        .map(|item| {
            vec![
                item.line.to_string(),
                kind_label(item.kind).to_owned(),
                visibility_label(item.visibility).to_owned(),
                item.name.clone(),
                item.doc.as_deref().map_or_else(
                    || "(MISSING ///)".to_owned(),
                    |doc| truncated(&format!("/// {doc}"), 72),
                ),
            ]
        })
        .collect();
    lines.push(render_table(
        &["line", "kind", "vis", "name", "doc"],
        &rows,
        0,
    ));
    Ok(lines.join("\n"))
}

/// Textual `use`-import fan-out over the production region: `(all imports,
/// crate-internal imports)`. A labeled heuristic, not compiler truth.
fn fan_out(file: &FileMap) -> (usize, usize) {
    let mut imports = 0usize;
    let mut internal = 0usize;
    for (_, line) in file.production_code_lines() {
        let Some(target) = use_import(line) else {
            continue;
        };
        imports += 1;
        if ["crate::", "super::", "self::"]
            .iter()
            .any(|prefix| target.starts_with(prefix))
        {
            internal += 1;
        }
    }
    (imports, internal)
}

fn use_import(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    [
        "use ",
        "pub use ",
        "pub(crate) use ",
        "pub(super) use ",
        "pub(self) use ",
    ]
    .iter()
    .find_map(|prefix| trimmed.strip_prefix(prefix))
    .map(str::trim_start)
}

/// Bindings introduced by `use` statements rooted outside this workspace.
///
/// This extends the Atlas's textual import grammar rather than adding a
/// second Rust parser. Suppressing an unresolved external binding is safer
/// than claiming it names a same-spelled workspace declaration.
struct ExternalImportBindings {
    names: BTreeSet<String>,
    has_glob: bool,
}

impl ExternalImportBindings {
    fn suppresses(&self, name: &str) -> bool {
        self.has_glob || self.names.contains(name)
    }

    fn absorb_import(&mut self, use_tree: &str) {
        for binding in scan::import_bindings_from_use_tree(use_tree) {
            if binding.local_name == "*" {
                self.has_glob = true;
            } else {
                self.names.insert(binding.local_name);
            }
        }
    }
}

/// Collect external import bindings that suppress same-named graph edges.
fn external_import_bindings(
    file: &FileMap,
    workspace_roots: &BTreeSet<String>,
) -> ExternalImportBindings {
    let mut bindings = ExternalImportBindings {
        names: BTreeSet::new(),
        has_glob: false,
    };
    let mut statement = None::<String>;
    for (_, line) in file.production_code_lines() {
        if let Some(current) = statement.as_mut() {
            current.push(' ');
            current.push_str(line.trim());
        } else if let Some(target) = use_import(line) {
            statement = Some(target.to_owned());
        }
        let Some(current) = statement.as_ref() else {
            continue;
        };
        if !current.contains(';') {
            continue;
        }
        let completed = statement.take().expect("completed import statement");
        let completed = completed.split(';').next().unwrap_or_default().trim();
        if completed.starts_with('{') {
            continue;
        }
        let mut tokens = identifiers(completed);
        let Some(root) = tokens.next() else {
            continue;
        };
        if matches!(root, "crate" | "super" | "self") || workspace_roots.contains(root) {
            continue;
        }
        bindings.absorb_import(completed);
    }
    bindings
}

/// Files in the same crate whose `use` lines mention this module's path or
/// its final segment — the labeled fan-in heuristic. `None` for crate roots:
/// every `crate::` path references them, so the count would be noise.
fn fan_in<'a>(crate_map: &'a CrateMap, file: &FileMap) -> Option<Vec<&'a str>> {
    let module_path = module_path_of(&crate_map.src_rel, &file.rel)?;
    let segment = module_path
        .rsplit("::")
        .next()
        .unwrap_or(module_path.as_str())
        .to_owned();
    let mut files = Vec::new();
    for other in &crate_map.files {
        if other.rel == file.rel {
            continue;
        }
        let mentions = other
            .production_code_lines()
            .map(|(_, line)| line)
            .filter_map(use_import)
            .any(|target| {
                target.contains(&module_path) || count_word_occurrences(target, &segment) > 0
            });
        if mentions {
            files.push(other.rel.as_str());
        }
    }
    Some(files)
}

/// The `::`-joined module path of a file within its crate (`src/a/b.rs` →
/// `a::b`, `src/a/mod.rs` → `a`); `None` for the crate root.
fn module_path_of(src_rel: &str, rel: &str) -> Option<String> {
    let inside = rel.strip_prefix(src_rel)?.strip_prefix('/')?;
    let stem = inside.strip_suffix(".rs")?;
    let stem = stem.strip_suffix("/mod").unwrap_or(stem);
    if matches!(stem, "lib" | "main" | "mod") {
        return None;
    }
    Some(stem.replace('/', "::"))
}

/// Marker substrings that count a `//!` header as citing its authority
/// (the master spec, a CR decision, or another `.design` spec) for the comments
/// audit.
const CITATION_MARKERS: &[&str] = &[".design", "corpus-runtime", "CR-", "§"];

fn cites(header_block: &str) -> bool {
    CITATION_MARKERS
        .iter()
        .any(|marker| header_block.contains(marker))
}

#[derive(Default, Clone, Copy)]
struct AuditRow {
    files: usize,
    missing_header: usize,
    uncited_header: usize,
    items: usize,
    undocumented: usize,
}

impl AuditRow {
    fn absorb(&mut self, other: Self) {
        self.files += other.files;
        self.missing_header += other.missing_header;
        self.uncited_header += other.uncited_header;
        self.items += other.items;
        self.undocumented += other.undocumented;
    }
}

fn comments_audit(ws: &Workspace, filter: Filter, json: bool) -> Result<String> {
    let rows: Vec<(String, AuditRow)> = ws
        .crates
        .iter()
        .map(|crate_map| {
            let mut row = AuditRow::default();
            for file in &crate_map.files {
                row.files += 1;
                let header = file.projection.module_header_lines();
                if header.is_empty() {
                    row.missing_header += 1;
                } else if !cites(&header.join("\n")) {
                    row.uncited_header += 1;
                }
                for item in file
                    .production_items()
                    .filter(|item| is_census_kind(item.kind) && filter.admits(item))
                {
                    row.items += 1;
                    if item.doc.is_none() {
                        row.undocumented += 1;
                    }
                }
            }
            (crate_map.name.clone(), row)
        })
        .collect();

    let mut total = AuditRow::default();
    for (_, row) in &rows {
        total.absorb(*row);
    }

    let row_json = |key: &str, row: &AuditRow| {
        json!({
            "key": key,
            "files": row.files,
            "missing_header": row.missing_header,
            "uncited_header": row.uncited_header,
            "items": row.items,
            "undocumented": row.undocumented,
        })
    };
    if json {
        let json_rows: Vec<_> = rows.iter().map(|(key, row)| row_json(key, row)).collect();
        return pretty(&json!({
            "verb": "comments",
            "population": filter.label(),
            "citation_markers": CITATION_MARKERS,
            "rows": json_rows,
            "total": row_json("TOTAL", &total),
        }));
    }

    let row_cells = |key: &str, row: &AuditRow| {
        vec![
            key.to_owned(),
            row.files.to_string(),
            row.missing_header.to_string(),
            row.uncited_header.to_string(),
            row.items.to_string(),
            row.undocumented.to_string(),
        ]
    };
    let mut cells: Vec<Vec<String>> = rows.iter().map(|(key, row)| row_cells(key, row)).collect();
    cells.push(row_cells("TOTAL", &total));
    Ok(format!(
        "atlas comments --audit · population: {} · uncited = //! block missing all of {:?}\n{}",
        filter.label(),
        CITATION_MARKERS,
        render_table(
            &["crate", "files", "no-//!", "uncited-//!", "items", "no-///"],
            &cells,
            1,
        )
    ))
}

/// The largest single top-level item block on the production side of the test
/// boundary: each top-level item's span runs to the next top-level item (or
/// the boundary), and the file's concentration is the max span. A max, never
/// a mean — the mean is what the cut density grade tried and it could not see
/// a god-impl diluted among many small items.
fn max_block(file: &FileMap) -> Option<(usize, &Item)> {
    let mut tops: Vec<&Item> = file.items.iter().filter(|item| item.top_level).collect();
    tops.sort_by_key(|item| item.line);
    let end_of_file = file.projection.text().lines().count() + 1;
    let mut best: Option<(usize, &Item)> = None;
    for (index, item) in tops.iter().enumerate() {
        // A block ends at whichever comes first: the next top-level item or
        // the next test region. Both bounds are needed — the region catches
        // trailing tests whose declaration the line grammar did not record,
        // which is what let a block measure wider than its own module.
        let next_item = tops.get(index + 1).map_or(end_of_file, |next| next.line);
        let next_region = file
            .regions
            .next_start_after(item.line)
            .unwrap_or(end_of_file);
        let span = next_item.min(next_region).saturating_sub(item.line);
        let production = !file.test_file && !file.regions.covers(item.line);
        if production && best.as_ref().is_none_or(|(top, _)| span > *top) {
            best = Some((span, item));
        }
    }
    best
}

/// The concentration report — the spec's named successor to the failed
/// per-module mean grade: a per-module **max**, never a mean,
/// because a mean cannot see a god-impl diluted among many small top-level
/// items. For every module, the largest single top-level item block on the
/// production side of the test boundary, measured as the line span to the
/// next top-level item (exact up to trailing blank lines under rustfmt's
/// column-0 discipline — approximate by design, like every atlas edge).
/// Alongside it, the module's top-level item count — the *breadth* axis. The
/// two are orthogonal pathologies and neither sees the other: a 6k-line
/// `impl` is one item, and a 344-item module has no large block. Reading them
/// together is the point of one table.
/// Report-only: not a gate, not a ledger.
fn concentration(ws: &Workspace, key: Option<&str>, json: bool) -> Result<String> {
    struct Row {
        rel: String,
        nontest_loc: usize,
        block_loc: usize,
        items: usize,
        kind: ItemKind,
        name: String,
        line: usize,
    }
    let mut rows = Vec::new();
    for crate_map in &ws.crates {
        if let Some(key) = key
            && crate_map.name != key
            && !crate_map.src_rel.starts_with(key)
        {
            continue;
        }
        for file in &crate_map.files {
            if let Some((span, item)) = max_block(file) {
                rows.push(Row {
                    rel: file.rel.clone(),
                    nontest_loc: file.nontest_loc(),
                    block_loc: span,
                    items: file
                        .production_items()
                        .filter(|item| item.top_level)
                        .count(),
                    kind: item.kind,
                    name: item.name.clone(),
                    line: item.line,
                });
            }
        }
    }
    rows.sort_by(|a, b| b.block_loc.cmp(&a.block_loc).then(a.rel.cmp(&b.rel)));
    if json {
        let values: Vec<_> = rows
            .iter()
            .map(|row| {
                json!({
                    "module": row.rel,
                    "nontest_loc": row.nontest_loc,
                    "max_block_loc": row.block_loc,
                    "top_level_items": row.items,
                    "kind": kind_label(row.kind),
                    "name": row.name,
                    "line": row.line,
                })
            })
            .collect();
        return pretty(&json!({ "verb": "concentration", "rows": values }));
    }
    let total = rows.len();
    rows.truncate(25);
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            vec![
                row.rel.clone(),
                format!("{} {}:{}", kind_label(row.kind), row.name, row.line),
                row.block_loc.to_string(),
                row.nontest_loc.to_string(),
                format!("{}%", 100 * row.block_loc / row.nontest_loc.max(1)),
                row.items.to_string(),
            ]
        })
        .collect();
    Ok(format!(
        "atlas concentration — depth (largest top-level block) and breadth (item count) per module (top {} of {total})\n\n{}",
        rows.len(),
        render_table(
            &[
                "module",
                "largest block",
                "block-loc",
                "nontest-loc",
                "share",
                "items"
            ],
            &cells,
            2,
        )
    ))
}

/// What a change did to the shape of every module it touched. For each module
/// whose depth, breadth, or non-test LOC moved against a git ref, one row — so
/// "an arc touching a monolith must shrink it" is evidence a landing can quote
/// rather than a promise it makes.
/// Unchanged modules are omitted; a module absent on one side reads as 0.
fn diff(ws: &Workspace, reference: &str, json: bool) -> Result<String> {
    #[derive(Default, Clone, Copy, PartialEq, Eq)]
    struct Shape {
        nontest_loc: usize,
        block_loc: usize,
        items: usize,
    }
    fn shapes(ws: &Workspace) -> BTreeMap<String, Shape> {
        ws.files()
            .map(|file| {
                let shape = Shape {
                    nontest_loc: file.nontest_loc(),
                    block_loc: max_block(file).map_or(0, |(span, _)| span),
                    items: file
                        .production_items()
                        .filter(|item| item.top_level)
                        .count(),
                };
                (file.rel.clone(), shape)
            })
            .collect()
    }

    let before = shapes(&ws.at_ref(reference)?);
    let after = shapes(ws);
    let moved: Vec<(String, Shape, Shape)> = before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|rel| {
            let (old, new) = (
                before.get(rel).copied().unwrap_or_default(),
                after.get(rel).copied().unwrap_or_default(),
            );
            (rel.clone(), old, new)
        })
        .filter(|(_, old, new)| old != new)
        .collect();
    let delta = |old: usize, new: usize| new.cast_signed() - old.cast_signed();
    let signed = |value: isize| {
        if value > 0 {
            format!("+{value}")
        } else {
            value.to_string()
        }
    };

    if json {
        let values: Vec<_> = moved
            .iter()
            .map(|(rel, old, new)| {
                json!({
                    "module": rel,
                    "nontest_loc": [old.nontest_loc, new.nontest_loc],
                    "max_block_loc": [old.block_loc, new.block_loc],
                    "top_level_items": [old.items, new.items],
                })
            })
            .collect();
        return pretty(&json!({ "verb": "diff", "reference": reference, "rows": values }));
    }

    let total: isize = moved
        .iter()
        .map(|(_, old, new)| delta(old.nontest_loc, new.nontest_loc))
        .sum();
    let mut ranked = moved;
    ranked.sort_by_key(|(_, old, new)| -delta(old.nontest_loc, new.nontest_loc).abs());
    let shown = ranked.len().min(25);
    let cells: Vec<Vec<String>> = ranked
        .iter()
        .take(shown)
        .map(|(rel, old, new)| {
            vec![
                rel.clone(),
                signed(delta(old.nontest_loc, new.nontest_loc)),
                signed(delta(old.block_loc, new.block_loc)),
                signed(delta(old.items, new.items)),
                format!("{} → {}", old.nontest_loc, new.nontest_loc),
            ]
        })
        .collect();
    Ok(format!(
        "atlas diff — module shape against {reference} ({shown} of {} moved; net non-test {})\n\n{}",
        ranked.len(),
        signed(total),
        render_table(
            &["module", "Δloc", "Δblock", "Δitems", "non-test"],
            &cells,
            1,
        )
    ))
}

// ── rendering ──────────────────────────────────────────────────────────────

fn pretty(value: &serde_json::Value) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(|err| format!("serialize atlas json: {err}"))
}

/// Fixed-width table: columns from `right_from` onward are right-aligned
/// (0 = all left-aligned). Trailing spaces are trimmed per line so output is
/// byte-stable and clean to grep.
fn render_table(header: &[&str], rows: &[Vec<String>], right_from: usize) -> String {
    let mut widths: Vec<usize> = header.iter().map(|cell| cell.len()).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.chars().count());
        }
    }
    let render_row = |cells: Vec<String>| -> String {
        let mut line = String::new();
        for (index, cell) in cells.iter().enumerate() {
            if index > 0 {
                line.push_str("  ");
            }
            let width = widths[index];
            let pad = width.saturating_sub(cell.chars().count());
            if index >= right_from && right_from > 0 {
                line.push_str(&" ".repeat(pad));
                line.push_str(cell);
            } else {
                line.push_str(cell);
                line.push_str(&" ".repeat(pad));
            }
        }
        line.trim_end().to_owned()
    };
    let mut out = render_row(header.iter().map(|cell| (*cell).to_owned()).collect());
    for row in rows {
        out.push('\n');
        out.push_str(&render_row(row.clone()));
    }
    out
}

fn truncated(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_owned()
    } else {
        let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

const fn kind_label(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Struct => "struct",
        ItemKind::Enum => "enum",
        ItemKind::Trait => "trait",
        ItemKind::TypeAlias => "type",
        ItemKind::Fn => "fn",
        ItemKind::Mod => "mod",
        ItemKind::MacroRules => "macro",
        ItemKind::PubUse => "pub-use",
        ItemKind::Const => "const",
        ItemKind::Static => "static",
        ItemKind::Impl => "impl",
    }
}

const fn visibility_label(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Pub => "pub",
        Visibility::PubCrate => "pub(crate)",
        Visibility::PubSuper => "pub(super)",
        Visibility::Private => "private",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invocation_validation_rejects_silent_flag_and_shape_mismatches() {
        assert!(
            validate_invocation(
                "comments",
                None,
                AtlasFlags {
                    audit: true,
                    ..AtlasFlags::default()
                }
            )
            .is_ok()
        );
        assert!(
            validate_invocation(
                "module",
                Some("src/lib.rs"),
                AtlasFlags {
                    items: true,
                    graph: true,
                    ..AtlasFlags::default()
                }
            )
            .is_ok()
        );

        assert!(
            validate_invocation(
                "census",
                None,
                AtlasFlags {
                    audit: true,
                    ..AtlasFlags::default()
                }
            )
            .expect_err("--audit is irrelevant")
            .contains("does not accept --audit")
        );
        assert!(
            validate_invocation(
                "concentration",
                None,
                AtlasFlags {
                    items: true,
                    ..AtlasFlags::default()
                }
            )
            .expect_err("--items is irrelevant")
            .contains("does not accept --items")
        );
        assert!(
            validate_invocation(
                "comments",
                None,
                AtlasFlags {
                    audit: true,
                    graph: true,
                    ..AtlasFlags::default()
                }
            )
            .expect_err("--graph is irrelevant")
            .contains("does not accept --graph")
        );
        assert!(
            validate_invocation("dump", None, AtlasFlags::default())
                .expect_err("dump requires JSON")
                .contains("requires --json")
        );
        assert!(
            validate_invocation("unknown", None, AtlasFlags::default())
                .expect_err("unknown command")
                .contains("unknown atlas verb")
        );
    }

    #[test]
    fn max_block_finds_the_god_impl_not_the_item_count() {
        // One 8-line impl among many 1-line items: the max must pick the impl
        // (the mean-based grade this replaced would have diluted it away).
        let source = "\
pub struct A;
pub struct B;
pub struct C;

impl A {
    fn one() {}
    fn two() {}
    fn three() {}
    fn four() {}
    fn five() {}
    fn six() {}
}

pub fn tail() {}

#[cfg(test)]
mod tests {
    fn giant_test_helper_that_must_not_win() {}
}
";
        let file = fixture_file("crates/x/src/lib.rs", source);
        let (span, item) = max_block(&file).expect("items exist");
        assert_eq!(item.name, "A");
        assert_eq!(item.kind, ItemKind::Impl);
        // The impl body, plus the blank line before the next item.
        assert!(span >= 8, "span {span} should cover the impl body");
        // The block never reaches past the module's own production lines —
        // the invariant a trailing test region used to break.
        assert!(span <= file.nontest_loc());
    }

    #[test]
    fn test_files_resolve_through_the_rust_module_layout() {
        // `session.rs` declares `#[cfg(test)] mod tests;`, and Rust puts that
        // child in `session/` — not beside its parent. Resolving it in the
        // parent's own directory left the workspace's largest test file
        // (6,349 lines) counted as production.
        assert_eq!(
            child_directory("crates/murail-session/src/session.rs"),
            "crates/murail-session/src/session"
        );
        assert_eq!(
            child_directory("crates/murail-session/src/lib.rs"),
            "crates/murail-session/src"
        );
        assert_eq!(
            child_directory("crates/murail-session/src/session/mod.rs"),
            "crates/murail-session/src/session"
        );

        let mut files = vec![
            fixture_file(
                "crates/x/src/session.rs",
                "pub fn production() {}\n\n#[cfg(test)]\nmod tests;\n",
            ),
            fixture_file("crates/x/src/session/tests.rs", "fn a() {}\nfn b() {}\n"),
        ];
        mark_test_files(&mut files);
        assert!(!files[0].test_file, "the parent is production");
        assert!(files[1].test_file, "the declared child is all tests");
        assert_eq!(files[1].nontest_loc(), 0);
        assert_eq!(files[1].production_items().count(), 0);
    }

    #[test]
    fn fan_edges_exclude_interleaved_tests_without_dropping_later_production() {
        let consumer = "\
use crate::before::Thing;

#[cfg(test)]
mod tests {
    use crate::target::TestOnly;
}

use crate::target::Production;
";
        let test_only = "\
#[cfg(test)]
mod tests {
    use crate::target::TestOnly;
}
";
        let target = "pub struct Production;\n";
        let crate_map = CrateMap {
            name: "fixture".to_owned(),
            src_rel: "crates/fixture/src".to_owned(),
            files: vec![
                fixture_file("crates/fixture/src/consumer.rs", consumer),
                fixture_file("crates/fixture/src/test_only.rs", test_only),
                fixture_file("crates/fixture/src/target.rs", target),
            ],
        };

        assert_eq!(fan_out(&crate_map.files[0]), (2, 2));
        assert_eq!(
            fan_in(&crate_map, &crate_map.files[2]),
            Some(vec!["crates/fixture/src/consumer.rs"])
        );
    }

    #[test]
    fn a_block_never_measures_wider_than_its_module() {
        // Production, then a test module the line grammar records no item
        // for: the block must stop at the region, not run to end of file.
        let source = "\
pub fn only_production() {
    let _ = 1;
}

#[cfg(test)]
mod tests {
    fn a() {}
    fn b() {}
    fn c() {}
    fn d() {}
}
";
        let file = fixture_file("crates/x/src/lib.rs", source);
        let (span, _) = max_block(&file).expect("items exist");
        assert!(
            span <= file.nontest_loc(),
            "block {span} exceeded the module's {} production lines",
            file.nontest_loc()
        );
    }

    fn fixture_file(rel: &str, text: &str) -> FileMap {
        FileMap::of(rel.to_owned(), text.to_owned())
    }

    fn fixture_workspace() -> Workspace {
        let admit = "\
//! Admission for fixture tiles — realization kernel §5.

use crate::grade::Cost;
use std::fmt;

/// An admitted fixture tile.
pub struct Tile {
    cost: Cost,
}

/// Refusal for the tile family.
pub(crate) enum Refusal {
    CrossLane,
}

pub fn admit(cost: Cost) -> Tile {
    Tile { cost }
}

fn private_helper() {}

#[cfg(test)]
mod tests {
    use super::*;
}
";
        let grade = "\
pub struct Cost;

pub use self::Cost as PublicCost;

macro_rules! grade_table {
    () => {};
}
";
        let lib = "\
//! Fixture crate root.

pub mod admit;
pub mod grade;

pub use crate::admit::Tile;
";
        Workspace {
            root: PathBuf::from("/fixture"),
            crates: vec![CrateMap {
                name: "murail-fix".to_owned(),
                src_rel: "crates/murail-fix/src".to_owned(),
                files: vec![
                    fixture_file("crates/murail-fix/src/admit.rs", admit),
                    fixture_file("crates/murail-fix/src/grade.rs", grade),
                    fixture_file("crates/murail-fix/src/lib.rs", lib),
                ],
            }],
        }
    }

    const PUBISH: Filter = Filter {
        pub_only: false,
        include_private: false,
    };
    const ALL: Filter = Filter {
        pub_only: false,
        include_private: true,
    };

    #[test]
    fn census_workspace_rung_is_the_per_crate_golden() {
        let ws = fixture_workspace();
        let graph = build_graph(&ws, PUBISH);
        let output = census(&ws, None, &graph, PUBISH, false).expect("census renders");
        let expected = "\
atlas census — workspace (per crate) · population: pub-ish · loc: non-test
crate       types  fns  alias  macro  loc  doc%
murail-fix      3    1      2      1   34   50%
TOTAL           3    1      2      1   34   50%";
        assert_eq!(output, expected);
    }

    #[test]
    fn census_crate_rung_lists_modules_and_pub_only_narrows() {
        let ws = fixture_workspace();
        let graph = build_graph(&ws, PUBISH);
        let output =
            census(&ws, Some("murail-fix"), &graph, PUBISH, false).expect("census renders");
        let expected = "\
atlas census — murail-fix (per module under crates/murail-fix/src/) · population: pub-ish · loc: non-test
module    types  fns  alias  macro  loc  doc%
admit.rs      2    1      0      0   21   66%
grade.rs      1    0      1      1    7    0%
lib.rs        0    0      1      0    6     -
TOTAL         3    1      2      1   34   50%";
        assert_eq!(output, expected);

        let narrowed = census(
            &ws,
            Some("murail-fix"),
            &graph,
            Filter {
                pub_only: true,
                include_private: false,
            },
            false,
        )
        .expect("census renders");
        // Refusal is pub(crate): the pub-only population drops it.
        assert!(narrowed.contains("admit.rs      1    1"));
    }

    #[test]
    fn dump_serializes_the_existing_item_and_edge_authorities() {
        let ws = fixture_workspace();
        let graph = build_graph(&ws, ALL);
        let output = dump(&ws, &graph, ALL).expect("dump renders");
        let value: serde_json::Value = serde_json::from_str(&output).expect("dump is valid JSON");

        assert_eq!(value["verb"], "dump");
        assert_eq!(value["population"], "all");
        let items = value["items"].as_array().expect("items is an array");
        let refusal = items
            .iter()
            .find(|item| item["name"] == "Refusal")
            .expect("pub(crate) item is in the pub-ish dump");
        assert_eq!(refusal["kind"], "enum");
        assert_eq!(refusal["home"], "murail-fix::admit");
        assert_eq!(refusal["vis"], "crate");
        assert_eq!(refusal["pub"], false);
        assert_eq!(refusal["id"], "crates/murail-fix/src/admit.rs:12:Refusal");
        let private_helper = items
            .iter()
            .find(|item| item["name"] == "private_helper")
            .expect("the complete dump includes private declarations");
        assert_eq!(private_helper["vis"], "private");
        assert_eq!(private_helper["pub"], false);

        let edges = value["edges"].as_array().expect("edges is an array");
        let has_edge = |source: &str, kind: &str, target: &str| {
            edges.iter().any(|edge| {
                edge["source"] == source && edge["kind"] == kind && edge["target"] == target
            })
        };
        assert!(has_edge("admit", "consumes", "Cost"));
        assert!(has_edge("admit", "produces", "Tile"));
        assert!(has_edge("Tile", "composed-of", "Cost"));
        assert!(has_edge("Tile", "facade", "Tile"));
        assert!(has_edge("Cost", "re-exports", "PublicCost"));
        let tile_facade = edges
            .iter()
            .find(|edge| edge["source"] == "Tile" && edge["kind"] == "facade")
            .expect("fixture root exposes Tile through its facade");
        assert!(tile_facade["source_id"].is_string());
        assert_eq!(tile_facade["ambiguous"], false);
        assert_eq!(tile_facade["unresolved"], false);
        assert!(edges.iter().all(|edge| edge.get("source_path").is_some()
            && edge.get("source_id").is_some()
            && edge.get("target_id").is_some()
            && edge.get("ambiguous").is_some()
            && edge.get("unresolved").is_some()));

        let narrowed = dump(
            &ws,
            &build_graph(
                &ws,
                Filter {
                    pub_only: true,
                    include_private: false,
                },
            ),
            Filter {
                pub_only: true,
                include_private: false,
            },
        )
        .expect("pub-only dump renders");
        let narrowed: serde_json::Value =
            serde_json::from_str(&narrowed).expect("pub-only dump is valid JSON");
        assert!(
            narrowed["items"]
                .as_array()
                .expect("items is an array")
                .iter()
                .all(|item| item["name"] != "Refusal")
        );
    }

    #[test]
    fn public_root_reexports_are_the_only_facade_edges() {
        assert_eq!(reexport_edge_kind(Visibility::Pub, true), "facade");
        assert_eq!(reexport_edge_kind(Visibility::PubCrate, true), "re-exports");
    }

    #[test]
    fn name_card_shows_facade_edges_and_doc_gap() {
        let ws = fixture_workspace();
        let graph = build_graph(&ws, PUBISH);
        let output = name_card(&ws, "Tile", &graph, PUBISH, false).expect("name renders");
        let expected = "\
atlas name — Tile · 1 match(es) (pub-ish population) · ~3 refs (approximate, textual)
reexports: crates/murail-fix/src/lib.rs:6
produced-by (1): admit crates/murail-fix/src/admit.rs:16
consumed-by: (none)
composes: (none)
composed-of (1): Cost
crates/murail-fix/src/admit.rs:7  struct  pub
    /// An admitted fixture tile.";
        assert_eq!(output, expected);

        // An undocumented fn renders its gap and its outgoing signature edges.
        let undocumented = name_card(&ws, "admit", &graph, PUBISH, false).expect("name renders");
        assert!(undocumented.contains("/// (MISSING — no doc comment)"));
        assert!(undocumented.contains("consumes (1): Cost"));
        assert!(undocumented.contains("produces (1): Tile"));

        // A component type's card shows who composes it and who consumes it.
        let cost = name_card(&ws, "grade::Cost", &graph, PUBISH, false).expect("name renders");
        assert!(cost.contains("crates/murail-fix/src/grade.rs:1"));
        assert!(cost.contains("consumed-by (1): admit crates/murail-fix/src/admit.rs:16"));
        assert!(cost.contains("composes (1): Tile crates/murail-fix/src/admit.rs:7"));
    }

    #[test]
    fn module_card_shows_header_items_orphans_and_graph() {
        let ws = fixture_workspace();
        let graph = build_graph(&ws, PUBISH);
        let output = module_card(
            &ws,
            "crates/murail-fix/src/admit.rs",
            &graph,
            PUBISH,
            true,
            true,
            false,
        )
        .expect("module renders");
        let expected = "\
atlas module — crates/murail-fix/src/admit.rs (murail-fix)
//! Admission for fixture tiles — realization kernel §5.
nontest-loc: 21 · census (pub-ish): 2 types, 1 fns, 0 aliases, 0 macros · undocumented pub-ish: 1
fan-out: 2 use-imports (1 crate-internal) · fan-in: 1 files: crates/murail-fix/src/lib.rs [heuristic use-edges]
orphans (1): Refusal [census types no other declaration mentions]
graph: 3 edges (declaration-level co-mentions)
  7 Tile -composes-> Cost (crates/murail-fix/src/grade.rs)
  16 admit -consumes-> Cost (crates/murail-fix/src/grade.rs)
  16 admit -produces-> Tile
items:
line  kind    vis         name            doc
7     struct  pub         Tile            /// An admitted fixture tile.
12    enum    pub(crate)  Refusal         /// Refusal for the tile family.
16    fn      pub         admit           (MISSING ///)
20    fn      private     private_helper  (MISSING ///)";
        assert_eq!(output, expected);

        // A header-less module shows the gap, visibly; without --graph the
        // edge list stays off the card.
        let bare = module_card(
            &ws,
            "crates/murail-fix/src/grade.rs",
            &graph,
            PUBISH,
            true,
            false,
            false,
        )
        .expect("module renders");
        assert!(bare.contains("//! MISSING — no module header"));
        assert!(bare.contains("orphans: 0"));
        assert!(!bare.contains("graph:"));
    }

    #[test]
    fn edges_classify_params_returns_and_fields() {
        let ws = fixture_workspace();
        let graph = build_graph(&ws, PUBISH);
        let mut short: Vec<String> = graph
            .edges
            .iter()
            .map(|edge| {
                format!(
                    "{} -{}-> {}",
                    edge.source_name,
                    class_label(edge.class),
                    edge.target
                )
            })
            .collect();
        short.sort();
        assert_eq!(
            short,
            vec![
                "Tile -composes-> Cost",
                "admit -consumes-> Cost",
                "admit -produces-> Tile",
            ]
        );
        // Refusal is mentioned by no other declaration block: the orphan
        // strip carries it, per module.
        assert_eq!(
            graph.orphans.get("crates/murail-fix/src/admit.rs"),
            Some(&vec!["Refusal".to_owned()])
        );
        assert_eq!(graph.orphans.get("crates/murail-fix/src/grade.rs"), None);
    }

    #[test]
    fn edge_resolution_prefers_same_crate_and_marks_cross_crate_duplicates() {
        let shared_a = "pub struct Shared;\n";
        let shared_b = "\
pub struct Shared;

pub fn use_shared(x: Shared) -> Shared {
    x
}
";
        let probe_c = "pub fn probe(x: Shared) {}\n";
        let single_crate = |name: &str, text: &str| CrateMap {
            name: name.to_owned(),
            src_rel: format!("crates/{name}/src"),
            files: vec![fixture_file(&format!("crates/{name}/src/lib.rs"), text)],
        };
        let ws = Workspace {
            root: PathBuf::from("/fixture"),
            crates: vec![
                single_crate("fix-a", shared_a),
                single_crate("fix-b", shared_b),
                single_crate("fix-c", probe_c),
            ],
        };
        let graph = build_graph(&ws, PUBISH);

        // fix-b declares its own Shared: both edges resolve there.
        let from_b: Vec<&Edge> = graph
            .edges
            .iter()
            .filter(|edge| edge.source_name == "use_shared")
            .collect();
        assert_eq!(from_b.len(), 2);
        assert!(from_b.iter().all(|edge| {
            !edge.ambiguous && edge.target_file.as_deref() == Some("crates/fix-b/src/lib.rs")
        }));

        // fix-c has no local Shared and two cross-crate candidates: the
        // textual match cannot pick one, so the edge is marked ambiguous.
        let from_c: Vec<&Edge> = graph
            .edges
            .iter()
            .filter(|edge| edge.source_name == "probe")
            .collect();
        assert_eq!(from_c.len(), 1);
        assert!(from_c[0].ambiguous && from_c[0].target_file.is_none());
    }

    #[test]
    fn external_import_cannot_resolve_to_a_same_named_workspace_type() {
        let workspace_value = "pub struct Value;\npub struct Tile;\n";
        let external_value = "\
use serde_json::Value;

pub fn encode(_: Tile) -> Value {
    todo!()
}
";
        let workspace_crate = |name: &str, text: &str| CrateMap {
            name: name.to_owned(),
            src_rel: format!("crates/{name}/src"),
            files: vec![fixture_file(&format!("crates/{name}/src/lib.rs"), text)],
        };
        let ws = Workspace {
            root: PathBuf::from("/fixture"),
            crates: vec![
                workspace_crate("workspace-value", workspace_value),
                workspace_crate("external-user", external_value),
            ],
        };
        let graph = build_graph(&ws, PUBISH);

        assert!(
            graph
                .edges
                .iter()
                .filter(|edge| edge.source_name == "encode")
                .all(|edge| edge.target != "Value"),
            "an external import must suppress its same-named workspace candidate"
        );
        assert!(
            graph
                .edges
                .iter()
                .any(|edge| edge.source_name == "encode" && edge.target == "Tile"),
            "external binding suppression must preserve unrelated workspace edges"
        );
    }

    #[test]
    fn external_import_bindings_report_only_names_introduced_into_scope() {
        let bindings = |import| {
            let mut bindings = ExternalImportBindings {
                names: BTreeSet::new(),
                has_glob: false,
            };
            bindings.absorb_import(import);
            bindings
        };
        assert_eq!(
            bindings("serde_json::value::Value").names,
            BTreeSet::from(["Value".to_owned()])
        );
        assert_eq!(
            bindings("serde_json::{Map, Value as JsonValue}").names,
            BTreeSet::from(["JsonValue".to_owned(), "Map".to_owned()])
        );
        assert_eq!(
            bindings("serde_json::{self, Value}").names,
            BTreeSet::from(["Value".to_owned(), "serde_json".to_owned()])
        );
        assert!(bindings("serde_json::*").has_glob);
    }

    #[test]
    fn import_text_inside_comments_and_strings_cannot_suppress_graph_edges() {
        let workspace_value = "pub struct Value;\n";
        let user = r##"
/*
use external::Value;
*/
const SOURCE: &str = r#"
use external::Value;
"#;

pub fn encode() -> Value {
    todo!()
}
"##;
        let workspace_crate = |name: &str, text: &str| CrateMap {
            name: name.to_owned(),
            src_rel: format!("crates/{name}/src"),
            files: vec![fixture_file(&format!("crates/{name}/src/lib.rs"), text)],
        };
        let ws = Workspace {
            root: PathBuf::from("/fixture"),
            crates: vec![
                workspace_crate("workspace-value", workspace_value),
                workspace_crate("user", user),
            ],
        };
        let graph = build_graph(&ws, PUBISH);
        assert!(
            graph
                .edges
                .iter()
                .any(|edge| edge.source_name == "encode" && edge.target == "Value")
        );
    }

    #[test]
    fn signature_split_ignores_arrows_inside_bounds() {
        let (params, ret) =
            split_signature("pub fn map<F: Fn(Tile) -> Cost>(f: F, x: Tile) -> Grade {");
        assert!(params.contains("x: Tile"));
        assert!(!params.contains("Grade"));
        assert!(ret.contains("Grade"));
        // No return type: everything is the parameter side.
        let (params, ret) = split_signature("pub fn tick(&mut self, frame: Frame) {");
        assert!(params.contains("Frame"));
        assert!(ret.is_empty());
    }

    #[test]
    fn comments_audit_totals_the_policy_gaps() {
        let output = comments_audit(&fixture_workspace(), PUBISH, false).expect("audit renders");
        // grade.rs misses its header; lib.rs has one citing nothing; the
        // pub-ish census population is 4 items of which 2 lack `///`.
        let expected_tail = "\
crate       files  no-//!  uncited-//!  items  no-///
murail-fix      3       1            1      4       2
TOTAL           3       1            1      4       2";
        assert!(
            output.ends_with(expected_tail),
            "audit output was:\n{output}"
        );
    }

    #[test]
    fn word_occurrences_respect_identifier_boundaries() {
        assert_eq!(
            count_word_occurrences("Tile, LaneTile, Tile::new", "Tile"),
            2
        );
        assert_eq!(count_word_occurrences("TileSet Tile_x", "Tile"), 0);
        assert_eq!(count_word_occurrences("(Tile)", "Tile"), 1);
    }

    #[test]
    fn citation_heuristic_recognizes_runtime_and_spec_mentions() {
        assert!(cites("Implements CR-D104."));
        assert!(cites("See `.design/implementation/specs/x.md`."));
        assert!(cites("Runtime §5 admission."));
        assert!(!cites("Recognition and admission for tiles."));
    }

    #[test]
    fn module_paths_derive_from_src_relative_files() {
        assert_eq!(
            module_path_of("crates/x/src", "crates/x/src/a/b.rs").as_deref(),
            Some("a::b")
        );
        assert_eq!(
            module_path_of("crates/x/src", "crates/x/src/a/mod.rs").as_deref(),
            Some("a")
        );
        assert_eq!(module_path_of("crates/x/src", "crates/x/src/lib.rs"), None);
    }
}
