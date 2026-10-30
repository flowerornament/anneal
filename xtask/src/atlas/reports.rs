//! Native report schemas and presentation over shared documentation and outline measures.
use super::{Filter, Result, input, pretty, render_table};
use atlas_core::{
    discovery::Snapshot,
    documentation,
    population::{Population, TestSelection},
    review,
    revision::Revision,
    source::ItemKind,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
const CITATION_MARKERS: &[&str] = &[".design", "corpus-runtime", "CR-", "§"];
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

pub(super) fn comments(snapshot: &Snapshot, filter: Filter, json: bool) -> Result<String> {
    let policy = documentation::Policy {
        items: Population {
            visibility: filter.selection(),
            tests: TestSelection::OutsideTestModules,
        },
        headers: documentation::HeaderPopulation::AllFiles,
        header_scope: documentation::HeaderScope::File,
        item_kinds: &[
            ItemKind::Struct,
            ItemKind::Enum,
            ItemKind::Union,
            ItemKind::Trait,
            ItemKind::TypeAlias,
            ItemKind::Fn,
        ],
        citation_markers: Some(CITATION_MARKERS),
    };
    let mut packages = snapshot.metadata().workspace_packages();
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    let rows: Vec<_> = packages
        .into_iter()
        .map(|package| {
            let scope = package
                .manifest_path
                .parent()
                .ok_or("manifest without parent")?
                .join("src");
            let scope = scope
                .strip_prefix(&snapshot.metadata().workspace_root)
                .map_err(|e| e.to_string())?;
            let report = documentation::audit(snapshot, scope.as_str(), &policy)
                .map_err(|e| e.to_string())?;
            let mut row = AuditRow {
                files: report.checked_files,
                items: report.checked_items,
                ..AuditRow::default()
            };
            for finding in report.findings {
                match finding {
                    documentation::Finding::MissingHeader { .. } => row.missing_header += 1,
                    documentation::Finding::UncitedHeader { .. } => row.uncited_header += 1,
                    documentation::Finding::MissingOuterLineDoc { .. } => row.undocumented += 1,
                }
            }
            Ok((package.name.to_string(), row))
        })
        .collect::<Result<Vec<_>>>()?;
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

pub(super) fn concentration(snapshot: &Snapshot, key: Option<&str>, json: bool) -> Result<String> {
    struct Row {
        rel: String,
        nontest_loc: usize,
        block_loc: usize,
        items: usize,
        kind: String,
        name: String,
        line: usize,
    }
    let mut rows = Vec::new();
    let roots: Vec<_> = snapshot
        .metadata()
        .workspace_packages()
        .into_iter()
        .map(|package| {
            let root = package
                .manifest_path
                .parent()
                .ok_or("manifest without parent")?
                .join("src");
            let rel = root
                .strip_prefix(&snapshot.metadata().workspace_root)
                .map_err(|e| e.to_string())?;
            Ok((package.name.to_string(), rel.to_string()))
        })
        .collect::<Result<_>>()?;
    let outlines = roots
        .iter()
        .filter(|(name, root)| key.is_none_or(|key| name == key || root.starts_with(key)))
        .map(|(_, root)| review::outline(snapshot, root).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>>>()?;
    for file in outlines.into_iter().flat_map(|outline| outline.files) {
        let selected = roots.iter().any(|(name, root)| {
            key.is_none_or(|key| name == key || root.starts_with(key))
                && file.file.starts_with(&format!("{root}/"))
        });
        if !selected {
            continue;
        }
        if let Some(block) = file.largest_block {
            let kind = file
                .items
                .iter()
                .find(|item| item.declaration == block.line)
                .ok_or("outline block declaration absent")?
                .kind
                .clone();
            rows.push(Row {
                rel: file.file,
                nontest_loc: file.non_test_lines,
                block_loc: block.lines,
                items: file.top_level_items,
                kind,
                name: block.name,
                line: block.line,
            });
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
                    "kind": outline_kind(&row.kind),
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
                format!("{} {}:{}", outline_kind(&row.kind), row.name, row.line),
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

pub(super) fn diff(snapshot: &Snapshot, reference: &str, json: bool) -> Result<String> {
    #[derive(Default, Clone, Copy, PartialEq, Eq)]
    struct Shape {
        nontest_loc: usize,
        block_loc: usize,
        items: usize,
    }
    fn shapes(snapshot: &Snapshot) -> Result<BTreeMap<String, Shape>> {
        let selected: BTreeSet<_> = snapshot
            .files(atlas_core::population::FileSelection::SourceTree)
            .map(|f| f.path.as_str())
            .collect();
        Ok(review::outline(snapshot, ".")
            .map_err(|e| e.to_string())?
            .files
            .into_iter()
            .filter(|file| selected.contains(file.file.as_str()))
            .map(|file| {
                (
                    file.file,
                    Shape {
                        nontest_loc: file.non_test_lines,
                        block_loc: file.largest_block.map_or(0, |block| block.lines),
                        items: file.top_level_items,
                    },
                )
            })
            .collect())
    }
    let revision = Revision::read(snapshot.metadata().workspace_root.as_std_path(), reference)
        .map_err(|e| e.to_string())?;
    let historical = input::historical(snapshot, &revision)?;
    for diagnostic in input::diagnostic_lines(&historical) {
        eprintln!("{diagnostic}");
    }
    let before = shapes(&historical)?;
    let after = shapes(snapshot)?;
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

fn outline_kind(kind: &str) -> &str {
    match kind {
        "Struct" => "struct",
        "Enum" => "enum",
        "Union" => "union",
        "Trait" => "trait",
        "TypeAlias" => "type",
        "Fn" => "fn",
        "Mod" => "mod",
        "MacroRules" | "MacroInvocation" => "macro",
        "PubUse" => "pub-use",
        "Use" => "use",
        "Const" => "const",
        "Static" => "static",
        "Impl" => "impl",
        other => other,
    }
}
