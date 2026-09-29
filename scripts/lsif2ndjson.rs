#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! anyhow = "1"
//! serde_json = "1"
//! ```
//! Convert rust-analyzer LSIF to contract-A resolves/refs NDJSON.
//! Usage: rust-script scripts/lsif2ndjson.rs INPUT.lsif OUTPUT_DIR BASIS

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

#[derive(Clone, Copy)]
struct Range {
    line: u32,
    col: u32,
    end_line: u32,
    end_col: u32,
    document: u64,
}

#[derive(Default)]
struct Lsif {
    project_root: PathBuf,
    documents: HashMap<u64, String>,
    ranges: HashMap<u64, Range>,
    next: HashMap<u64, u64>,
    definitions: HashMap<u64, u64>,
    references: HashMap<u64, u64>,
    monikers: HashMap<u64, String>,
    moniker_of: HashMap<u64, u64>,
    items: HashMap<u64, Vec<(u64, &'static str)>>,
}

fn number(row: &Value, key: &str) -> Result<u64> {
    row.get(key)
        .and_then(Value::as_u64)
        .with_context(|| format!("LSIF row lacks integer {key}: {row}"))
}

fn uri_path(uri: &str) -> Result<PathBuf> {
    let encoded = uri
        .strip_prefix("file://")
        .with_context(|| format!("expected file URI, got {uri}"))?;
    let mut decoded = Vec::with_capacity(encoded.len());
    let bytes = encoded.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = encoded
                .get(index + 1..index + 3)
                .with_context(|| format!("truncated URI escape in {uri}"))?;
            decoded.push(u8::from_str_radix(hex, 16)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Ok(PathBuf::from(String::from_utf8(decoded)?))
}

fn read_lsif(path: &Path) -> Result<Lsif> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut lsif = Lsif::default();
    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| format!("read LSIF line {}", line_number + 1))?;
        let row: Value = serde_json::from_str(&line)
            .with_context(|| format!("parse LSIF line {}", line_number + 1))?;
        let label = row.get("label").and_then(Value::as_str).unwrap_or("");
        match (row.get("type").and_then(Value::as_str), label) {
            (Some("vertex"), "metaData") => {
                let root = row["projectRoot"]
                    .as_str()
                    .context("metadata projectRoot")?;
                lsif.project_root = uri_path(root)?;
            }
            (Some("vertex"), "document") => {
                let path = uri_path(row["uri"].as_str().context("document URI")?)?;
                let file = path
                    .strip_prefix(&lsif.project_root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();
                lsif.documents.insert(number(&row, "id")?, file);
            }
            (Some("vertex"), "range") => {
                let position = |side: &str, field: &str| -> Result<u32> {
                    Ok(u32::try_from(number(&row[side], field)?)?)
                };
                lsif.ranges.insert(
                    number(&row, "id")?,
                    Range {
                        line: position("start", "line")?,
                        col: position("start", "character")?,
                        end_line: position("end", "line")?,
                        end_col: position("end", "character")?,
                        document: 0,
                    },
                );
            }
            (Some("vertex"), "moniker") => {
                let identifier = row["identifier"].as_str().context("moniker identifier")?;
                lsif.monikers
                    .insert(number(&row, "id")?, identifier.to_string());
            }
            (Some("edge"), "contains") => {
                let document = number(&row, "outV")?;
                if let Some(children) = row["inVs"].as_array() {
                    for child in children {
                        if let Some(range) = child.as_u64().and_then(|id| lsif.ranges.get_mut(&id))
                        {
                            range.document = document;
                        }
                    }
                }
            }
            (Some("edge"), "next") => {
                lsif.next
                    .insert(number(&row, "outV")?, number(&row, "inV")?);
            }
            (Some("edge"), "textDocument/definition") => {
                lsif.definitions
                    .insert(number(&row, "outV")?, number(&row, "inV")?);
            }
            (Some("edge"), "textDocument/references") => {
                lsif.references
                    .insert(number(&row, "outV")?, number(&row, "inV")?);
            }
            (Some("edge"), "moniker") => {
                lsif.moniker_of
                    .insert(number(&row, "outV")?, number(&row, "inV")?);
            }
            (Some("edge"), "item") => {
                let property = match row.get("property").and_then(Value::as_str) {
                    Some("references") => "references",
                    Some("definitions") => "definitions",
                    _ => "definition",
                };
                let result = number(&row, "outV")?;
                if let Some(children) = row["inVs"].as_array() {
                    let items = lsif.items.entry(result).or_default();
                    for child in children {
                        items.push((child.as_u64().context("item range ID")?, property));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(lsif)
}

impl Lsif {
    fn result_set(&self, range: u64) -> Option<u64> {
        let mut current = *self.next.get(&range)?;
        for _ in 0..32 {
            if self.definitions.contains_key(&current) || self.references.contains_key(&current) {
                return Some(current);
            }
            current = *self.next.get(&current)?;
        }
        None
    }

    fn definition_ranges(&self, result_set: u64) -> impl Iterator<Item = u64> + '_ {
        self.definitions
            .get(&result_set)
            .and_then(|result| self.items.get(result))
            .into_iter()
            .flatten()
            .map(|(range, _)| *range)
    }

    fn file_and_range(&self, range_id: u64) -> Option<(&str, Range)> {
        let range = *self.ranges.get(&range_id)?;
        let file = self.documents.get(&range.document)?;
        Some((file, range))
    }
}

fn source_name(
    root: &Path,
    file: &str,
    range: Range,
    cache: &mut HashMap<String, Vec<String>>,
) -> String {
    let lines = cache.entry(file.to_string()).or_insert_with(|| {
        fs::read_to_string(root.join(file))
            .map(|text| text.lines().map(str::to_string).collect())
            .unwrap_or_default()
    });
    if range.line != range.end_line {
        return format!("@col{}", range.col);
    }
    let Some(line) = lines.get(range.line as usize) else {
        return format!("@col{}", range.col);
    };
    let utf16 = line.encode_utf16().collect::<Vec<_>>();
    let start = range.col as usize;
    let end = range.end_col as usize;
    if start >= end || end > utf16.len() {
        return format!("@col{}", range.col);
    }
    String::from_utf16_lossy(&utf16[start..end])
}

fn symbol(
    lsif: &Lsif,
    result_set: u64,
    definition: u64,
    source_cache: &mut HashMap<String, Vec<String>>,
) -> Option<String> {
    if let Some(name) = lsif
        .moniker_of
        .get(&result_set)
        .and_then(|id| lsif.monikers.get(id))
    {
        return Some(name.clone());
    }
    let (file, range) = lsif.file_and_range(definition)?;
    let name = source_name(&lsif.project_root, file, range, source_cache);
    Some(format!("{file}:{}:{name}", range.line))
}

fn emit(lsif: &Lsif, output: &Path, basis: &str) -> Result<(usize, usize)> {
    fs::create_dir_all(output)?;
    let mut resolves = BufWriter::new(File::create(output.join("resolves.ndjson"))?);
    let mut refs = BufWriter::new(File::create(output.join("refs.ndjson"))?);
    writeln!(
        resolves,
        "{}",
        json!({"schema":"architecture-lens.rows.v1","relation":"resolves"})
    )?;
    writeln!(
        refs,
        "{}",
        json!({"schema":"architecture-lens.rows.v1","relation":"refs"})
    )?;
    let mut source_cache = HashMap::new();
    let mut resolve_count = 0;
    let mut ref_count = 0;
    let mut seen_refs = HashSet::new();
    let mut range_ids = lsif.ranges.keys().copied().collect::<Vec<_>>();
    range_ids.sort_unstable();
    for site_id in range_ids {
        let Some((site_file, site)) = lsif.file_and_range(site_id) else {
            continue;
        };
        let Some(result_set) = lsif.result_set(site_id) else {
            continue;
        };
        for definition_id in lsif.definition_ranges(result_set) {
            let Some((def_file, definition)) = lsif.file_and_range(definition_id) else {
                continue;
            };
            let Some(def_symbol) = symbol(lsif, result_set, definition_id, &mut source_cache)
            else {
                continue;
            };
            let witness = format!("{site_file}:{}", site.line + 1);
            let precision = if def_file.starts_with('/') {
                "over"
            } else {
                "exact"
            };
            writeln!(
                resolves,
                "{}",
                json!({
                    "basis":basis,"precision":precision,"evidence":"resolved","witness":witness,
                    "site_file":site_file,"site_line":site.line,"site_col":site.col,
                    "def_file":def_file,"def_line":definition.line,"def_symbol":def_symbol
                })
            )?;
            resolve_count += 1;
        }
    }
    let mut result_sets = lsif.references.keys().copied().collect::<Vec<_>>();
    result_sets.sort_unstable();
    for result_set in result_sets {
        let Some(result_id) = lsif.references.get(&result_set) else {
            continue;
        };
        let Some(items) = lsif.items.get(result_id) else {
            continue;
        };
        let Some(definition_id) = lsif.definition_ranges(result_set).next().or_else(|| {
            items
                .iter()
                .find(|(_, kind)| *kind == "definitions")
                .map(|(id, _)| *id)
        }) else {
            continue;
        };
        let Some((def_file, _)) = lsif.file_and_range(definition_id) else {
            continue;
        };
        let Some(def_symbol) = symbol(lsif, result_set, definition_id, &mut source_cache) else {
            continue;
        };
        let precision = if def_file.starts_with('/') {
            "over"
        } else {
            "exact"
        };
        for (reference_id, kind) in items {
            if *kind != "references" {
                continue;
            }
            let Some((ref_file, reference)) = lsif.file_and_range(*reference_id) else {
                continue;
            };
            if !seen_refs.insert((def_symbol.clone(), ref_file.to_string(), reference.line)) {
                continue;
            }
            writeln!(
                refs,
                "{}",
                json!({
                    "basis":basis,"precision":precision,"evidence":"resolved",
                    "witness":format!("{ref_file}:{}", reference.line + 1),
                    "def_symbol":def_symbol,"ref_file":ref_file,"ref_line":reference.line
                })
            )?;
            ref_count += 1;
        }
    }
    resolves.flush()?;
    refs.flush()?;
    Ok((resolve_count, ref_count))
}

fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 4 {
        bail!("usage: lsif2ndjson.rs INPUT.lsif OUTPUT_DIR BASIS");
    }
    let input = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    let lsif = read_lsif(input)?;
    let (resolves, refs) = emit(&lsif, output, &args[3])?;
    eprintln!(
        "resolves={resolves} refs={refs} documents={} ranges={}",
        lsif.documents.len(),
        lsif.ranges.len()
    );
    Ok(())
}
