//! `code_spike` — prove rustdoc JSON can behave as an anneal corpus.
//!
//! This is intentionally fixture-subset spike code, not the future
//! `anneal-code` adapter. It reads a committed rustdoc JSON file, emits
//! source facts into the existing runtime, and prints convergence-oriented
//! queries over the resulting code corpus.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use anneal_core::runtime::prelude::standard_prelude_program;
use anneal_core::runtime::{
    Database, EvalOptions, Evaluator, Row, analyze, parse_program, write_ndjson,
};
use anneal_core::{
    ConfigFact, ContentFact, CorpusId, EdgeFact, FactBatch, FactBatchMode, FactIdentity, FactStore,
    Generation, HandleFact, MetaFact, NativeId, OriginUri, Revision, SourceName, SpanFact,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

const SOURCE_NAME: &str = "rustdoc-json-spike";
const CORPUS_ID: &str = "rustdoc-toy";

const CODE_RULES: &str = r#"
pipeline_position_for("unstable", 0).
pipeline_position_for("deprecated", 1).
pipeline_position_for("stable", 2).

potential_weight("code_unstable", 4).
potential_weight("code_deprecated", 5).
potential_weight("code_used_by_api", 2).

entropy_priority("code_deprecated", 0).
entropy_priority("code_unstable", 1).
entropy_priority("code_used_by_api", 2).

entropy(h, "code_unstable") :=
  active(h),
  *meta{handle: h, key: "code.stability", value: "unstable"}.

entropy(h, "code_deprecated") :=
  active(h),
  *meta{handle: h, key: "code.stability", value: "deprecated"}.

entropy(h, "code_used_by_api") :=
  active(h),
  *edge{from: from, to: h, kind: "DependsOn"}.

api_risk_component(h, 4) :=
  active(h),
  *meta{handle: h, key: "code.stability", value: "unstable"}.

api_risk_component(h, 5) :=
  active(h),
  *meta{handle: h, key: "code.stability", value: "deprecated"}.

api_risk_component(h, 1) :=
  active(h),
  *edge{from: from, to: h, kind: "DependsOn"}.

api_frontier(h, energy, why) :=
  active(h),
  primary_entropy(h, why),
  energy = Sum{ w : api_risk_component(h, w) }.
"#;

#[derive(Debug, Deserialize)]
struct RustdocJson {
    root: u64,
    index: BTreeMap<String, RustdocItem>,
    paths: BTreeMap<String, RustdocPath>,
}

#[derive(Debug, Deserialize)]
struct RustdocItem {
    id: u64,
    crate_id: u64,
    name: Option<String>,
    span: Option<RustdocSpan>,
    visibility: JsonValue,
    docs: Option<String>,
    #[serde(default)]
    links: BTreeMap<String, u64>,
    #[serde(default)]
    attrs: Vec<BTreeMap<String, String>>,
    deprecation: Option<RustdocDeprecation>,
    inner: BTreeMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
struct RustdocPath {
    path: Vec<String>,
    kind: String,
}

#[derive(Debug, Deserialize)]
struct RustdocSpan {
    filename: String,
    begin: [u32; 2],
    end: [u32; 2],
}

#[derive(Debug, Deserialize)]
struct RustdocDeprecation {
    since: Option<String>,
    note: Option<String>,
}

#[derive(Debug)]
struct CodeItem {
    rustdoc_id: u64,
    handle: String,
    item_kind: String,
    status: String,
    visibility: String,
    since: Option<String>,
    deprecated_note: Option<String>,
    file: String,
    start_line: u32,
    end_line: u32,
    summary: String,
    docs: Option<String>,
    has_doctest: bool,
}

#[derive(Serialize)]
struct SpikeReport {
    fixture: String,
    facts: FactCounts,
    status_histogram: BTreeMap<String, usize>,
    note: &'static str,
}

#[derive(Serialize)]
struct FactCounts {
    handles: usize,
    edges: usize,
    spans: usize,
    content: usize,
    meta: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    run()
}

fn run() -> Result<(), Box<dyn Error>> {
    let fixture = fixture_path();
    let fixture_text = fs::read_to_string(&fixture)?;
    let rustdoc: RustdocJson = serde_json::from_str(&fixture_text)?;
    let batch = build_fact_batch(&rustdoc);
    let status_histogram = status_histogram(&batch);
    let counts = FactCounts {
        handles: batch.handles.len(),
        edges: batch.edges.len(),
        spans: batch.spans.len(),
        content: batch.content.len(),
        meta: batch.meta.len(),
    };

    let mut store = FactStore::default();
    store.merge(batch)?;
    let corpus = CorpusId::from(CORPUS_ID);
    store.replace_configs(&corpus, convergence_config())?;
    let database = Database::from_store(&store);

    let mut out = BufWriter::new(io::stdout().lock());
    serde_json::to_writer_pretty(
        &mut out,
        &SpikeReport {
            fixture: fixture.display().to_string(),
            facts: counts,
            status_histogram,
            note: "rustdoc stability attributes are being treated as the code corpus lifecycle",
        },
    )?;
    out.write_all(b"\n")?;

    write_section(&mut out, "status-like active handles")?;
    write_ndjson(
        &mut out,
        eval_rows(
            database.clone(),
            r"? *handle{id: h, status: status, kind: kind, summary: summary}, active(h).",
        )?,
    )?;

    write_section(&mut out, "settled handles")?;
    write_ndjson(
        &mut out,
        eval_rows(
            database.clone(),
            r"? *handle{id: h, status: status, kind: kind, summary: summary}, settled(h).",
        )?,
    )?;

    write_section(&mut out, "frontier via extended entropy")?;
    write_ndjson(
        &mut out,
        eval_rows(
            database.clone(),
            r"? frontier(h, energy), *handle{id: h, status: status}, primary_entropy(h, why).",
        )?,
    )?;

    write_section(&mut out, "api_frontier exact reverse-dependency risk")?;
    write_ndjson(
        &mut out,
        eval_rows(
            database,
            r"? api_frontier(h, energy, why), *handle{id: h, status: status}.",
        )?,
    )?;

    Ok(())
}

fn fixture_path() -> PathBuf {
    if let Some(arg) = std::env::args().nth(1) {
        return PathBuf::from(arg);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/rustdoc-toy/rustdoc_toy.json")
}

fn write_section(out: &mut impl Write, label: &str) -> io::Result<()> {
    writeln!(out, "\n--- {label} ---")
}

fn eval_rows(database: Database, query: &str) -> Result<Vec<Row>, Box<dyn Error>> {
    let mut program = standard_prelude_program()?;
    program
        .statements
        .extend(parse_program("code-spike-rules", CODE_RULES)?.statements);
    program
        .statements
        .extend(parse_program("code-spike-query", query)?.statements);
    let analyzed = analyze(program)?;
    let query = analyzed.queries().next().cloned().expect("query present");
    let mut evaluator = Evaluator::with_options(analyzed, database, EvalOptions::default());
    evaluator.run_fixpoint()?;
    Ok(evaluator.eval_query(&query)?.rows)
}

fn convergence_config() -> Vec<ConfigFact> {
    [
        ("convergence.active", "unstable", None),
        ("convergence.active", "deprecated", None),
        ("convergence.terminal", "stable", None),
        ("convergence.ordering", "unstable", Some(0)),
        ("convergence.ordering", "deprecated", Some(1)),
        ("convergence.ordering", "stable", Some(2)),
    ]
    .into_iter()
    .map(|(key, value, ordinal)| ConfigFact {
        corpus: CORPUS_ID.into(),
        key: key.to_string(),
        value: value.to_string(),
        ordinal,
    })
    .collect()
}

fn build_fact_batch(rustdoc: &RustdocJson) -> FactBatch {
    let source = SourceName::from(SOURCE_NAME);
    let generation = Generation::initial();
    let path_by_id = path_by_id(rustdoc);
    let parent_by_item = parent_by_item(rustdoc);
    let items = code_items(rustdoc, &path_by_id, &parent_by_item);
    let handles_by_id = items
        .iter()
        .map(|item| (item.rustdoc_id, item.handle.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut batch = FactBatch::new(
        CORPUS_ID.into(),
        source.clone(),
        FactBatchMode::FullSnapshot,
        generation,
    );
    let edge_context = EdgeContext {
        source: &source,
        generation,
    };

    for item in &items {
        emit_code_item_facts(&mut batch, &edge_context, item);
    }

    let mut seen_edges = BTreeSet::new();
    for item in &items {
        emit_code_item_edges(
            &mut batch,
            &edge_context,
            &mut seen_edges,
            item,
            rustdoc,
            &handles_by_id,
            &items,
        );
    }

    batch
}

struct EdgeContext<'a> {
    source: &'a SourceName,
    generation: Generation,
}

fn emit_code_item_facts(batch: &mut FactBatch, edge_context: &EdgeContext<'_>, item: &CodeItem) {
    let identity = identity(
        edge_context.source,
        edge_context.generation,
        &format!("item:{}", item.handle),
        &format!("rustdoc-json://{CORPUS_ID}/{}", item.rustdoc_id),
    );
    batch.handles.push(HandleFact {
        identity: identity.clone(),
        id: item.handle.clone(),
        kind: "file".to_string(),
        status: Some(item.status.clone()),
        namespace: item.item_kind.clone(),
        file: item.file.clone(),
        line: item.start_line,
        date: None,
        area: item.item_kind.clone(),
        summary: item.summary.clone(),
    });
    push_meta(batch, &identity, &item.handle, "status", &item.status);
    push_meta(
        batch,
        &identity,
        &item.handle,
        "code.item_kind",
        &item.item_kind,
    );
    push_meta(batch, &identity, &item.handle, "code.path", &item.handle);
    push_meta(
        batch,
        &identity,
        &item.handle,
        "code.visibility",
        &item.visibility,
    );
    push_meta(
        batch,
        &identity,
        &item.handle,
        "code.stability",
        &item.status,
    );
    if let Some(since) = &item.since {
        push_meta(batch, &identity, &item.handle, "code.since", since);
    }
    if let Some(note) = &item.deprecated_note {
        push_meta(batch, &identity, &item.handle, "code.deprecated_note", note);
    }
    emit_doc_span(batch, &identity, item);
    if item.has_doctest {
        push_doctest(batch, edge_context, item);
    }
}

fn emit_doc_span(batch: &mut FactBatch, identity: &FactIdentity, item: &CodeItem) {
    let Some(docs) = &item.docs else {
        return;
    };
    let span_id = format!("{}#docs", item.handle);
    batch.spans.push(SpanFact {
        identity: identity.clone(),
        id: span_id.clone(),
        handle: item.handle.clone(),
        start_line: item.start_line,
        end_line: item.end_line,
        summary: item.summary.clone(),
    });
    batch.content.push(ContentFact {
        identity: identity.clone(),
        handle: item.handle.clone(),
        span_id,
        lines: item.end_line.saturating_sub(item.start_line).max(1),
        text: docs.clone(),
        tokens: token_count(docs),
    });
}

fn emit_code_item_edges(
    batch: &mut FactBatch,
    edge_context: &EdgeContext<'_>,
    seen_edges: &mut BTreeSet<(String, String, String)>,
    item: &CodeItem,
    rustdoc: &RustdocJson,
    handles_by_id: &BTreeMap<u64, String>,
    items: &[CodeItem],
) {
    let Some(raw_item) = rustdoc.index.get(&item.rustdoc_id.to_string()) else {
        return;
    };
    for target in dependency_targets(raw_item, handles_by_id) {
        if target != item.handle {
            push_edge(
                batch,
                edge_context,
                seen_edges,
                &item.handle,
                &target,
                "DependsOn",
                item,
            );
        }
    }
    if let Some(target) = item
        .deprecated_note
        .as_deref()
        .and_then(|note| parse_use_target(note, items))
    {
        push_edge(
            batch,
            edge_context,
            seen_edges,
            &target,
            &item.handle,
            "Supersedes",
            item,
        );
    }
}

fn path_by_id(rustdoc: &RustdocJson) -> BTreeMap<u64, (String, String)> {
    rustdoc
        .paths
        .iter()
        .filter_map(|(id, path)| {
            id.parse::<u64>()
                .ok()
                .map(|id| (id, (path.path.join("::"), path.kind.clone())))
        })
        .collect()
}

fn parent_by_item(rustdoc: &RustdocJson) -> BTreeMap<u64, u64> {
    let mut parents = BTreeMap::new();
    for item in rustdoc.index.values() {
        let Some(impl_data) = item.inner.get("impl") else {
            continue;
        };
        let Some(parent_id) = impl_data
            .get("for")
            .and_then(|value| value.get("resolved_path"))
            .and_then(|value| value.get("id"))
            .and_then(JsonValue::as_u64)
        else {
            continue;
        };
        if let Some(items) = impl_data.get("items").and_then(JsonValue::as_array) {
            for child in items.iter().filter_map(JsonValue::as_u64) {
                parents.insert(child, parent_id);
            }
        }
    }
    parents
}

fn code_items(
    rustdoc: &RustdocJson,
    path_by_id: &BTreeMap<u64, (String, String)>,
    parent_by_item: &BTreeMap<u64, u64>,
) -> Vec<CodeItem> {
    let mut items = rustdoc
        .index
        .values()
        .filter(|item| item.crate_id == 0)
        .filter_map(|item| code_item(item, rustdoc.root, path_by_id, parent_by_item))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.handle.cmp(&right.handle));
    items
}

fn code_item(
    item: &RustdocItem,
    root: u64,
    path_by_id: &BTreeMap<u64, (String, String)>,
    parent_by_item: &BTreeMap<u64, u64>,
) -> Option<CodeItem> {
    let raw_kind = item.inner.keys().next()?.as_str();
    if raw_kind == "impl" {
        return None;
    }
    let name = item.name.as_deref()?;
    let (handle, path_kind) = if let Some((path, kind)) = path_by_id.get(&item.id) {
        (path.clone(), kind.clone())
    } else {
        let parent_id = parent_by_item.get(&item.id)?;
        let (parent, _) = path_by_id.get(parent_id)?;
        (format!("{parent}::{name}"), "method".to_string())
    };
    if item.id != root
        && !matches!(
            path_kind.as_str(),
            "module" | "struct" | "enum" | "trait" | "function" | "method"
        )
    {
        return None;
    }
    let stability = stability(item);
    let span = item.span.as_ref();
    let summary = item
        .docs
        .as_deref()
        .and_then(|docs| docs.lines().find(|line| !line.trim().is_empty()))
        .unwrap_or(name)
        .to_string();

    Some(CodeItem {
        rustdoc_id: item.id,
        handle,
        item_kind: if path_kind == "function" && parent_by_item.contains_key(&item.id) {
            "method".to_string()
        } else {
            path_kind
        },
        status: stability.status,
        visibility: visibility_string(&item.visibility),
        since: stability.since,
        deprecated_note: item
            .deprecation
            .as_ref()
            .and_then(|deprecation| deprecation.note.clone()),
        file: span.map_or_else(|| "rustdoc-json".to_string(), |span| span.filename.clone()),
        start_line: span.map_or(1, |span| span.begin[0]),
        end_line: span.map_or(1, |span| span.end[0]),
        has_doctest: item
            .docs
            .as_deref()
            .is_some_and(|docs| docs.contains("```")),
        docs: item.docs.clone(),
        summary,
    })
}

struct Stability {
    status: String,
    since: Option<String>,
}

fn stability(item: &RustdocItem) -> Stability {
    if item.deprecation.is_some() {
        return Stability {
            status: "deprecated".to_string(),
            since: item.deprecation.as_ref().and_then(|dep| dep.since.clone()),
        };
    }
    let attrs = item
        .attrs
        .iter()
        .flat_map(|attr| attr.values())
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    if attrs.contains("Unstable") {
        return Stability {
            status: "unstable".to_string(),
            since: None,
        };
    }
    Stability {
        status: "stable".to_string(),
        since: stable_since(&attrs),
    }
}

fn stable_since(attrs: &str) -> Option<String> {
    let marker = "major: ";
    let major = attrs.split(marker).nth(1)?.split(',').next()?.trim();
    let minor = attrs.split("minor: ").nth(1)?.split(',').next()?.trim();
    let patch = attrs
        .split("patch: ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .trim();
    Some(format!(
        "{major}.{minor}.{}",
        patch
            .trim_end_matches("})},")
            .trim_end_matches("})}")
            .trim_end_matches('}')
    ))
}

fn visibility_string(value: &JsonValue) -> String {
    match value {
        JsonValue::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn dependency_targets(
    item: &RustdocItem,
    handles_by_id: &BTreeMap<u64, String>,
) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    collect_resolved_path_ids(
        &JsonValue::Object(item.inner.clone().into_iter().collect()),
        &mut ids,
    );
    ids.extend(item.links.values().copied());
    ids.into_iter()
        .filter_map(|id| handles_by_id.get(&id).cloned())
        .collect()
}

fn collect_resolved_path_ids(value: &JsonValue, ids: &mut BTreeSet<u64>) {
    match value {
        JsonValue::Object(map) => {
            if let Some(id) = map
                .get("resolved_path")
                .and_then(|value| value.get("id"))
                .and_then(JsonValue::as_u64)
            {
                ids.insert(id);
            }
            for child in map.values() {
                collect_resolved_path_ids(child, ids);
            }
        }
        JsonValue::Array(values) => {
            for child in values {
                collect_resolved_path_ids(child, ids);
            }
        }
        _ => {}
    }
}

fn parse_use_target(note: &str, items: &[CodeItem]) -> Option<String> {
    let target = note.strip_prefix("use ")?.trim().trim_end_matches('.');
    items
        .iter()
        .find(|item| item.handle == target)
        .map(|item| item.handle.clone())
}

fn push_meta(batch: &mut FactBatch, identity: &FactIdentity, handle: &str, key: &str, value: &str) {
    batch.meta.push(MetaFact {
        identity: identity.clone(),
        handle: handle.to_string(),
        key: key.to_string(),
        value: value.to_string(),
    });
}

fn push_doctest(batch: &mut FactBatch, edge_context: &EdgeContext<'_>, item: &CodeItem) {
    let doctest = format!("{}#doctest", item.handle);
    let identity = identity(
        edge_context.source,
        edge_context.generation,
        &format!("doctest:{}", item.handle),
        &format!("rustdoc-json://{CORPUS_ID}/{}#doctest", item.rustdoc_id),
    );
    batch.handles.push(HandleFact {
        identity: identity.clone(),
        id: doctest.clone(),
        kind: "file".to_string(),
        status: Some("stable".to_string()),
        namespace: "doctest".to_string(),
        file: item.file.clone(),
        line: item.start_line,
        date: None,
        area: "doctest".to_string(),
        summary: format!("doctest for {}", item.handle),
    });
    push_meta(batch, &identity, &doctest, "code.item_kind", "doctest");
    push_meta(batch, &identity, &doctest, "status", "stable");
    let mut seen = BTreeSet::new();
    push_edge(
        batch,
        edge_context,
        &mut seen,
        &item.handle,
        &doctest,
        "Verifies",
        item,
    );
}

fn push_edge(
    batch: &mut FactBatch,
    edge_context: &EdgeContext<'_>,
    seen: &mut BTreeSet<(String, String, String)>,
    from: &str,
    to: &str,
    kind: &str,
    item: &CodeItem,
) {
    if !seen.insert((from.to_string(), to.to_string(), kind.to_string())) {
        return;
    }
    batch.edges.push(EdgeFact {
        identity: identity(
            edge_context.source,
            edge_context.generation,
            &format!("edge:{from}:{kind}:{to}"),
            &format!("rustdoc-json://{CORPUS_ID}/{}", item.rustdoc_id),
        ),
        from: from.to_string(),
        to: to.to_string(),
        kind: kind.to_string(),
        file: item.file.clone(),
        line: item.start_line,
    });
}

fn identity(
    source: &SourceName,
    generation: Generation,
    native_id: &str,
    origin_uri: &str,
) -> FactIdentity {
    FactIdentity::new(
        CORPUS_ID.into(),
        source.clone(),
        NativeId::from(native_id),
        OriginUri::from(origin_uri),
        Revision::from("rustdoc-toy-fixture"),
        generation,
    )
}

fn token_count(text: &str) -> u32 {
    u32::try_from(text.split_whitespace().count()).unwrap_or(u32::MAX)
}

fn status_histogram(batch: &FactBatch) -> BTreeMap<String, usize> {
    let mut histogram = BTreeMap::new();
    for handle in &batch.handles {
        let status = handle.status.as_deref().unwrap_or("null");
        *histogram.entry(status.to_string()).or_insert(0) += 1;
    }
    histogram
}
