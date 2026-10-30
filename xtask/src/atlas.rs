//! Anneal's command syntax and native presentation over pinned Atlas source reports.
//! The shared core owns discovery, lexical facts, navigation and shape calculations.
mod input;
mod reports;
use atlas_core::{
    model::{Evidence, Resolution},
    population::VisibilitySelection,
    queries::navigation::{self as nav, Navigation, Site, Tally},
    source::{ItemKind, Visibility},
};
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
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
    let snapshot = input::capture()?;
    for diagnostic in input::diagnostic_lines(&snapshot) {
        eprintln!("{diagnostic}");
    }
    let output = match (verb.as_str(), argument.as_deref()) {
        ("comments", None) => reports::comments(&snapshot, filter, flags.json)?,
        ("concentration", key) => reports::concentration(&snapshot, key, flags.json)?,
        ("diff", Some(reference)) => reports::diff(&snapshot, reference, flags.json)?,
        _ => {
            let navigation =
                Navigation::new(&snapshot, filter.selection()).map_err(|e| e.to_string())?;
            match (verb.as_str(), argument.as_deref()) {
                ("census", key) => census(&navigation, key, filter, flags.json)?,
                ("name", Some(query)) => name_card(&navigation, query, filter, flags.json)?,
                ("module", Some(path)) => module_card(
                    &navigation,
                    path,
                    filter,
                    flags.items,
                    flags.graph,
                    flags.json,
                )?,
                ("dump", None) => dump(&navigation, &snapshot, filter)?,
                _ => return Err("incomplete atlas invocation".to_owned()),
            }
        }
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

#[derive(Clone, Copy)]
struct Filter {
    pub_only: bool,
    include_private: bool,
}

impl Filter {
    const fn selection(self) -> VisibilitySelection {
        if self.pub_only {
            VisibilitySelection::Public
        } else if self.include_private {
            VisibilitySelection::All
        } else {
            VisibilitySelection::RestrictedOrPublic
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

fn census(
    navigation: &Navigation<'_>,
    key: Option<&str>,
    filter: Filter,
    json: bool,
) -> Result<String> {
    let report = navigation.census(key).map_err(|e| e.to_string())?;
    let nav::Census::Rows(report) = report else {
        return module_card(
            navigation,
            key.ok_or("missing module key")?,
            filter,
            false,
            false,
            json,
        );
    };
    let key_column = if key.is_none() { "crate" } else { "module" };
    let scope = report.scope;
    let rows: Vec<_> = report
        .rows
        .iter()
        .map(|row| (&row.key, row.tally))
        .collect();
    let total = report.total;
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

fn presentation_edges(edges: &[nav::Edge]) -> Vec<Edge> {
    edges
        .iter()
        .filter(|edge| !matches!(edge.resolution, Resolution::Unresolved(_)))
        .map(|edge| {
            let target = if matches!(edge.resolution, Resolution::Unique(_)) {
                edge.candidates.first()
            } else {
                None
            };
            Edge {
                source_file: edge.source.file.clone(),
                source_line: edge.source.line,
                source_name: edge.source.name.clone(),
                class: match edge.evidence {
                    Evidence::ParameterMention => EdgeClass::Consumes,
                    Evidence::ReturnMention => EdgeClass::Produces,
                    Evidence::TypeBodyMention => EdgeClass::Composes,
                },
                target: edge.target.clone(),
                target_file: target.map(|s| s.file.clone()),
                target_line: target.map(|s| s.line),
                ambiguous: matches!(edge.resolution, Resolution::Ambiguous(_)),
            }
        })
        .collect()
}
fn item_id(file: &str, line: usize, name: &str) -> String {
    format!("{file}:{line}:{name}")
}
fn site_id(site: &Site) -> String {
    item_id(&site.file, site.line, &site.name)
}
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

const NAME_EDGE_BUDGET: usize = 8;

fn name_card(
    navigation: &Navigation<'_>,
    query: &str,
    filter: Filter,
    json: bool,
) -> Result<String> {
    let report = navigation.name(query).map_err(|e| e.to_string())?;
    let name = query.rsplit("::").next().unwrap_or(query);
    let shown: Vec<_> = report.matches.iter().map(|site| (site, site)).collect();
    let hidden = report.hidden;
    let reexports: Vec<_> = report
        .reexports
        .iter()
        .map(|s| format!("{}:{}", s.file, s.line))
        .collect();
    let impls: Vec<_> = report
        .impls
        .iter()
        .map(|s| format!("{}:{}", s.file, s.line))
        .collect();
    let defining: BTreeSet<_> = report
        .matches
        .iter()
        .map(|s| (s.file.as_str(), s.line))
        .collect();
    let refs = report.refs_approx;
    let graph = presentation_edges(&report.edges);
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
                    "path": file.file,
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
            file.file,
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

const MODULE_ITEM_BUDGET: usize = 30;

/// `--graph` edge lists above this budget summarize to per-class counts and
/// the most-mentioned targets.
const MODULE_GRAPH_BUDGET: usize = 40;

#[allow(
    clippy::fn_params_excessive_bools,
    reason = "the two bools mirror the two CLI flags (--items, --graph); a config struct would only rename them"
)]
fn module_card(
    navigation: &Navigation<'_>,
    path: &str,
    filter: Filter,
    show_items: bool,
    show_graph: bool,
    json: bool,
) -> Result<String> {
    let report = navigation.module(path).map_err(|e| e.to_string())?;
    let context = report
        .contexts
        .first()
        .ok_or("module lacks package context")?;
    let header = context.header.first().cloned();
    let tally = context.tally;
    let undocumented = context.undocumented;
    let (imports, internal) = (context.imports, context.internal_imports);
    let fan_in_files = &context.fan_in_files;
    let mut orphans = context.unmentioned_names.clone();
    orphans.sort();
    let edges = presentation_edges(&context.edges);
    let mut module_edges: Vec<_> = edges.iter().collect();
    module_edges.sort_by(|a, b| {
        (a.source_line, a.class, &a.target).cmp(&(b.source_line, b.class, &b.target))
    });
    if json {
        let items: Vec<_> = context
            .items
            .iter()
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
            "path": report.path,
            "crate": context.package,
            "header": header,
            "nontest_loc": tally.nontest_loc,
            "census": tally_json(&report.path, &tally),
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
    lines.push(format!(
        "atlas module — {} ({})",
        report.path, context.package
    ));
    lines.push(header.map_or_else(
        || "//! MISSING — no module header".to_owned(),
        |first| format!("//! {first}"),
    ));
    lines.push(format!(
        "nontest-loc: {} · census ({}): {} types, {} fns, {} aliases, {} macros · undocumented {}: {undocumented}",
        tally.nontest_loc,
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
                    (Some(target_file), _) if target_file != &report.path => {
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
    let item_count = context.items.len();
    if !show_items && item_count > MODULE_ITEM_BUDGET {
        let mut kinds: std::collections::BTreeMap<&str, (usize, usize)> =
            std::collections::BTreeMap::new();
        for item in &context.items {
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
    let rows: Vec<Vec<String>> = context
        .items
        .iter()
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
        ItemKind::Union => "union",
        ItemKind::Use => "use",
        ItemKind::Extern => "extern",
        ItemKind::MacroCall | ItemKind::MacroRules => "macro",
        ItemKind::Struct => "struct",
        ItemKind::Enum => "enum",
        ItemKind::Trait => "trait",
        ItemKind::TypeAlias => "type",
        ItemKind::Fn => "fn",
        ItemKind::Mod => "mod",
        ItemKind::PubUse => "pub-use",
        ItemKind::Const => "const",
        ItemKind::Static => "static",
        ItemKind::Impl => "impl",
    }
}

const fn visibility_label(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Pub => "pub",
        Visibility::PubCrate | Visibility::PubIn => "pub(crate)",
        Visibility::PubSuper => "pub(super)",
        Visibility::Private => "private",
    }
}

fn dump(
    navigation: &Navigation<'_>,
    snapshot: &atlas_core::discovery::Snapshot,
    filter: Filter,
) -> Result<String> {
    let report = navigation.dump().map_err(|e| e.to_string())?;
    let ids: BTreeMap<_, _> = report
        .declarations
        .iter()
        .map(|d| (d.id.as_str(), site_id(&d.site)))
        .collect();
    let mut items: Vec<_> = report
        .declarations
        .iter()
        .map(|d| DumpItem {
            id: site_id(&d.site),
            name: d.site.name.clone(),
            kind: kind_label(d.site.kind),
            home: d.home.clone(),
            file: d.site.file.clone(),
            line: d.site.line,
            vis: dump_visibility_label(d.site.visibility),
            r#pub: d.site.visibility == Visibility::Pub,
        })
        .collect();
    let mut edges: Vec<_> = presentation_edges(&report.edges)
        .into_iter()
        .map(|edge| DumpEdge {
            source_id: Some(item_id(
                &edge.source_file,
                edge.source_line,
                &edge.source_name,
            )),
            source: edge.source_name,
            source_path: None,
            target_id: edge
                .target_file
                .as_ref()
                .zip(edge.target_line)
                .map(|(file, line)| item_id(file, line, &edge.target)),
            target: edge.target,
            target_file: edge.target_file,
            ambiguous: edge.ambiguous,
            unresolved: false,
            kind: match edge.class {
                EdgeClass::Consumes => "consumes",
                EdgeClass::Produces => "produces",
                EdgeClass::Composes => "composed-of",
            },
            file: edge.source_file,
            line: edge.source_line,
        })
        .collect();
    for export in report.exports {
        let target_id = item_id(&export.site.file, export.site.line, &export.local_name);
        let home = display_home(snapshot, &export.site)?;
        items.push(DumpItem {
            id: target_id.clone(),
            name: export.local_name.clone(),
            kind: "re-export",
            home,
            file: export.site.file.clone(),
            line: export.site.line,
            vis: dump_visibility_label(export.site.visibility),
            r#pub: export.site.visibility == Visibility::Pub,
        });
        let source_id = match &export.target {
            Resolution::Unique(id) => {
                Some(ids.get(id.as_str()).ok_or("export target absent")?.clone())
            }
            _ => None,
        };
        edges.push(DumpEdge {
            source: export.source_name,
            source_path: Some(export.source_path),
            source_id,
            target: export.local_name,
            target_id: Some(target_id),
            target_file: Some(export.site.file.clone()),
            ambiguous: matches!(export.target, Resolution::Ambiguous(_)),
            unresolved: matches!(export.target, Resolution::Unresolved(_)),
            kind: match export.kind {
                nav::ExportKind::Facade => "facade",
                nav::ExportKind::Reexport => "re-exports",
            },
            file: export.site.file,
            line: export.site.line,
        });
    }
    items.sort_by(|a, b| {
        (&a.file, a.line, &a.name, a.kind).cmp(&(&b.file, b.line, &b.name, b.kind))
    });
    edges.sort_by(|a, b| {
        (&a.file, a.line, &a.source, a.kind, &a.target)
            .cmp(&(&b.file, b.line, &b.source, b.kind, &b.target))
    });
    let value = Dump {
        verb: "dump",
        population: filter.label(),
        items,
        edges,
    };
    let mut bytes = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"");
    value
        .serialize(&mut serde_json::Serializer::with_formatter(
            &mut bytes, formatter,
        ))
        .map_err(|e| format!("serialize atlas dump: {e}"))?;
    String::from_utf8(bytes).map_err(|e| format!("encode atlas dump as UTF-8: {e}"))
}
const fn dump_visibility_label(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Pub => "pub",
        Visibility::PubCrate | Visibility::PubIn => "crate",
        Visibility::PubSuper => "super",
        Visibility::Private => "private",
    }
}

#[cfg(test)]
mod tests;

fn display_home(snapshot: &atlas_core::discovery::Snapshot, site: &Site) -> Result<String> {
    let package = snapshot
        .metadata()
        .workspace_packages()
        .into_iter()
        .find(|package| package.name == site.package)
        .ok_or("export package absent")?;
    let src = package
        .manifest_path
        .parent()
        .ok_or("manifest without parent")?
        .join("src");
    let src = src
        .strip_prefix(&snapshot.metadata().workspace_root)
        .map_err(|e| e.to_string())?;
    let path = site
        .file
        .strip_prefix(&format!("{src}/"))
        .ok_or("export outside source tree")?;
    let stem = path
        .strip_suffix(".rs")
        .ok_or("export source is not Rust")?;
    Ok(format!(
        "{}::{}",
        site.package,
        stem.strip_suffix("/mod").unwrap_or(stem).replace('/', "::")
    ))
}
