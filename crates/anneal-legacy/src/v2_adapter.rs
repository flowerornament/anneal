use std::collections::HashMap;

use anneal_core::{
    ConcernFact, ContentFact, EdgeFact, FactBatch, FactBatchMode, FactIdentity, Generation,
    HandleFact, MetaFact, NativeId, OriginUri, Revision, SourceName, SpanFact, fnv1a_64,
};
use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde_json::Value;

use crate::checks;
use crate::cli;
use crate::config;
use crate::extraction::ImplausibleReason;
use crate::graph::{DiGraph, EdgeKind};
use crate::handle::{Handle, HandleKind, HandleMetadata, NodeId, resolved_file};
use crate::parse::{self, PendingEdge};

pub fn extract_markdown_facts(
    root: &Utf8Path,
    corpus: anneal_core::CorpusId,
    source: SourceName,
    generation: Generation,
) -> Result<FactBatch> {
    let config = config::load_config(root.as_std_path())?;
    let mut result = parse::build_graph(root, &config)?;
    let _stats = crate::resolve::resolve_all(
        &mut result.graph,
        &result.label_candidates,
        &result.pending_edges,
        &config,
        root,
        &result.filename_index,
    );
    let pre_cascade_index = crate::resolve::build_node_index(&result.graph);
    let root_str = root.to_string();
    let cascade_results = crate::resolve::cascade_unresolved(
        &mut result.graph,
        &result.pending_edges,
        &pre_cascade_index,
        &root_str,
    );
    let node_index = crate::resolve::build_node_index(&result.graph);

    let mut batch = FactBatch::new(corpus, source, FactBatchMode::FullSnapshot, generation);
    let mut revisions = RevisionCache::new(root);

    for (node_id, handle) in result.graph.nodes() {
        let fact = handle_fact(&batch, &mut revisions, &result, node_id, handle);
        batch.handles.push(fact);
        emit_resolved_file_meta(&mut batch, &mut revisions, &result.graph, handle);
    }

    let edge_order_context = EdgeOrderContext {
        root,
        config: &config,
        result: &result,
        pre_cascade_index: &pre_cascade_index,
        node_index: &node_index,
        cascade_results: &cascade_results,
    };
    emit_ordered_edges(&mut batch, &mut revisions, &edge_order_context);

    for extraction in &result.extractions {
        emit_file_parent_meta(&mut batch, &mut revisions, &extraction.file);
        emit_frontmatter_meta(&mut batch, &mut revisions, root, &extraction.file);
    }
    emit_implausible_ref_meta(&mut batch, &mut revisions, &result)?;
    emit_content_spans(&mut batch, &mut revisions, root, &result);
    emit_concerns(&mut batch, &mut revisions, &config, &result);

    Ok(batch)
}

pub fn status_json_from_facts(root: &Utf8Path, batch: &FactBatch) -> Result<Value> {
    let loaded = LoadedFacts::from_batch(root, batch)?;
    let diagnostics = loaded.diagnostics();
    let snap = crate::snapshot::build_snapshot(
        &loaded.graph,
        &loaded.lattice,
        &loaded.config,
        &diagnostics,
    );
    let output = cli::cmd_status(
        &loaded.graph,
        &loaded.lattice,
        &snap,
        &diagnostics,
        None,
        None,
    );
    let output = output.with_convergence(None);
    serde_json::to_value(cli::JsonEnvelope::new(cli::OutputMeta::full(), output))
        .context("serialize fact-backed status output")
}

pub fn check_json_from_facts(root: &Utf8Path, batch: &FactBatch) -> Result<Value> {
    let loaded = LoadedFacts::from_batch(root, batch)?;
    let diagnostics = loaded.diagnostics();
    let terminal_files = cli::terminal_file_set(&loaded.graph, &loaded.lattice);
    let output = cli::cmd_check(
        diagnostics,
        &cli::CheckFilters {
            errors_only: false,
            suggest: false,
            stale: false,
            obligations: false,
            active_only: true,
        },
        &terminal_files,
    );
    let json_output = cli::build_check_json_output(
        &output,
        &[],
        &cli::CheckJsonOptions {
            include_diagnostics: false,
            diagnostics_limit: 50,
            include_extractions_summary: false,
            include_full_extractions: false,
            full: false,
        },
    );
    serde_json::to_value(json_output).context("serialize fact-backed check output")
}

pub fn get_refs_json_from_facts(root: &Utf8Path, batch: &FactBatch, handle: &str) -> Result<Value> {
    let loaded = LoadedFacts::from_batch(root, batch)?;
    let snippets = cli::SnippetIndex {
        files: &loaded.file_snippets,
        labels: &loaded.label_snippets,
    };
    let Some(data) = cli::cmd_get(&loaded.graph, &loaded.node_index, snippets, handle) else {
        bail!("handle {handle:?} not found in fact-backed graph");
    };
    let output = cli::build_get_json_output(
        &data,
        &cli::GetJsonOptions {
            mode: cli::GetJsonMode::Refs,
            limit_edges: 10,
        },
    );
    serde_json::to_value(output).context("serialize fact-backed get output")
}

struct RevisionCache<'a> {
    root: &'a Utf8Path,
    revisions: HashMap<String, Revision>,
}

impl<'a> RevisionCache<'a> {
    fn new(root: &'a Utf8Path) -> Self {
        Self {
            root,
            revisions: HashMap::new(),
        }
    }

    fn revision_for(&mut self, file: &str) -> Revision {
        self.revisions
            .entry(file.to_string())
            .or_insert_with(|| {
                let path = self.root.join(file);
                let bytes = std::fs::read(path).unwrap_or_default();
                Revision::from(format!("{:016x}", fnv1a_64(&bytes)))
            })
            .clone()
    }
}

fn handle_fact(
    batch: &FactBatch,
    revisions: &mut RevisionCache<'_>,
    result: &parse::BuildResult,
    node_id: NodeId,
    handle: &Handle,
) -> HandleFact {
    let file = handle
        .file_path
        .as_ref()
        .map_or_else(String::new, ToString::to_string);
    let native_id = native_id_for(handle);
    let identity = identity_for(batch, revisions, &native_id, &file);
    let namespace = match &handle.kind {
        HandleKind::Label { prefix, .. } => prefix.clone(),
        _ => String::new(),
    };
    let snippet = match &handle.kind {
        HandleKind::File(path) => result.file_snippets.get(path.as_str()).map(String::as_str),
        HandleKind::Label { .. } => result.label_snippets.get(&handle.id).map(String::as_str),
        _ => None,
    };

    HandleFact {
        identity,
        id: handle.id.clone(),
        kind: handle.kind.as_str().to_string(),
        status: handle.status.clone(),
        namespace,
        file: file.clone(),
        line: declaration_line(result, node_id, handle),
        date: handle.date.map(|date| date.format("%Y-%m-%d").to_string()),
        area: area_for(&file),
        summary: handle.summary(snippet).unwrap_or_default().to_string(),
    }
}

fn emit_resolved_file_meta(
    batch: &mut FactBatch,
    revisions: &mut RevisionCache<'_>,
    graph: &DiGraph,
    handle: &Handle,
) {
    let Some(file) = resolved_file(handle, graph).map(ToString::to_string) else {
        return;
    };
    if file.is_empty() {
        return;
    }
    let native_id = native_id_for(handle);
    batch.meta.push(MetaFact {
        identity: identity_for(batch, revisions, &native_id, &file),
        handle: handle.id.clone(),
        key: "md.resolved_file".to_string(),
        value: file,
    });
}

fn declaration_line(_result: &parse::BuildResult, _node_id: NodeId, handle: &Handle) -> u32 {
    u32::from(matches!(handle.kind, HandleKind::File(_)))
}

fn edge_fact(
    batch: &FactBatch,
    revisions: &mut RevisionCache<'_>,
    source_handle: &Handle,
    to: String,
    kind: &str,
    line: u32,
    ordinal: usize,
) -> EdgeFact {
    let file = source_handle
        .file_path
        .as_ref()
        .map_or_else(String::new, ToString::to_string);
    let native_id = native_id_for_edge(source_handle, kind, &to, line, ordinal);
    EdgeFact {
        identity: identity_for(batch, revisions, &native_id, &file),
        from: source_handle.id.clone(),
        to,
        kind: kind.to_string(),
        file,
        line,
    }
}

fn native_id_for_edge(
    source_handle: &Handle,
    kind: &str,
    target: &str,
    line: u32,
    ordinal: usize,
) -> String {
    format!(
        "{}::edge::{ordinal}::{kind}::{target}::{line}",
        native_id_for(source_handle)
    )
}

struct OrderedEdge {
    source: NodeId,
    target: NodeId,
    kind: EdgeKind,
    line: u32,
}

struct EdgeOrderContext<'a> {
    root: &'a Utf8Path,
    config: &'a config::AnnealConfig,
    result: &'a parse::BuildResult,
    pre_cascade_index: &'a HashMap<String, NodeId>,
    node_index: &'a HashMap<String, NodeId>,
    cascade_results: &'a [crate::resolve::CascadeResult],
}

fn emit_ordered_edges(
    batch: &mut FactBatch,
    revisions: &mut RevisionCache<'_>,
    context: &EdgeOrderContext<'_>,
) {
    let mut remaining = graph_edges(&context.result.graph);
    let mut ordered = Vec::new();

    take_parse_time_external_edges(&context.result.graph, &mut remaining, &mut ordered);
    take_label_edges(
        context.config,
        context.result,
        context.node_index,
        &mut remaining,
        &mut ordered,
    );
    take_version_edges(&context.result.graph, &mut remaining, &mut ordered);
    take_pending_edges(
        context.root,
        context.result,
        context.pre_cascade_index,
        &mut remaining,
        &mut ordered,
    );
    take_cascade_edges(
        context.result,
        context.node_index,
        context.cascade_results,
        &mut remaining,
        &mut ordered,
    );
    ordered.extend(remaining);

    for (ordinal, edge) in ordered.into_iter().enumerate() {
        let source_handle = context.result.graph.node(edge.source);
        let target_handle = context.result.graph.node(edge.target);
        batch.edges.push(edge_fact(
            batch,
            revisions,
            source_handle,
            target_handle.id.clone(),
            edge.kind.as_str(),
            edge.line,
            ordinal,
        ));
    }

    let ordered_count = batch.edges.len();
    for (idx, edge) in context.result.pending_edges.iter().enumerate() {
        if context.node_index.contains_key(&edge.target_identity) {
            continue;
        }
        let source_handle = context.result.graph.node(edge.source);
        // CR-R6: unresolved reference attempts remain stored edge facts so
        // fact-backed diagnostics can reproduce v1 existence checks.
        batch.edges.push(edge_fact(
            batch,
            revisions,
            source_handle,
            edge.target_identity.clone(),
            edge.kind.as_str(),
            edge.line.unwrap_or(0),
            ordered_count + idx,
        ));
    }
}

fn graph_edges(graph: &DiGraph) -> Vec<OrderedEdge> {
    let mut edges = Vec::new();
    for (node_id, _handle) in graph.nodes() {
        for edge in graph.outgoing(node_id) {
            edges.push(OrderedEdge {
                source: edge.source,
                target: edge.target,
                kind: edge.kind.clone(),
                line: 0,
            });
        }
    }
    edges
}

fn take_parse_time_external_edges(
    graph: &DiGraph,
    remaining: &mut Vec<OrderedEdge>,
    ordered: &mut Vec<OrderedEdge>,
) {
    let mut index = 0;
    while index < remaining.len() {
        let target = graph.node(remaining[index].target);
        if matches!(target.kind, HandleKind::External { .. }) {
            ordered.push(remaining.remove(index));
        } else {
            index += 1;
        }
    }
}

fn take_label_edges(
    config: &config::AnnealConfig,
    result: &parse::BuildResult,
    node_index: &HashMap<String, NodeId>,
    remaining: &mut Vec<OrderedEdge>,
    ordered: &mut Vec<OrderedEdge>,
) {
    let namespaces = crate::resolve::infer_namespaces(&result.label_candidates, config);
    let heading_first = result
        .label_candidates
        .iter()
        .filter(|candidate| candidate.is_heading)
        .chain(
            result
                .label_candidates
                .iter()
                .filter(|candidate| !candidate.is_heading),
        );

    for candidate in heading_first {
        if !namespaces.contains(&candidate.prefix) {
            continue;
        }
        let Some(&target) = node_index.get(&format!("{}-{}", candidate.prefix, candidate.number))
        else {
            continue;
        };
        let Some(&source) = node_index.get(candidate.file_path.as_str()) else {
            continue;
        };
        take_edge(remaining, ordered, source, target, &candidate.edge_kind, 0);
    }
}

fn take_version_edges(
    graph: &DiGraph,
    remaining: &mut Vec<OrderedEdge>,
    ordered: &mut Vec<OrderedEdge>,
) {
    for (source, handle) in graph.nodes() {
        if !matches!(handle.kind, HandleKind::Version { .. }) {
            continue;
        }
        for edge in graph.outgoing(source) {
            if edge.kind == EdgeKind::Supersedes {
                take_edge(remaining, ordered, edge.source, edge.target, &edge.kind, 0);
            }
        }
    }
}

fn take_pending_edges(
    root: &Utf8Path,
    result: &parse::BuildResult,
    pre_cascade_index: &HashMap<String, NodeId>,
    remaining: &mut Vec<OrderedEdge>,
    ordered: &mut Vec<OrderedEdge>,
) {
    for edge in &result.pending_edges {
        let Some(target) = resolve_pending_target(root, result, pre_cascade_index, edge) else {
            continue;
        };
        let (source, target) = if edge.inverse {
            (target, edge.source)
        } else {
            (edge.source, target)
        };
        take_edge(
            remaining,
            ordered,
            source,
            target,
            &edge.kind,
            edge.line.unwrap_or(0),
        );
    }
}

fn take_cascade_edges(
    result: &parse::BuildResult,
    node_index: &HashMap<String, NodeId>,
    cascade_results: &[crate::resolve::CascadeResult],
    remaining: &mut Vec<OrderedEdge>,
    ordered: &mut Vec<OrderedEdge>,
) {
    for cascade in cascade_results {
        let Some(edge) = result.pending_edges.get(cascade.edge_index) else {
            continue;
        };
        for candidate in &cascade.candidates {
            let Some(&target) = node_index.get(candidate) else {
                continue;
            };
            let (source, target) = if edge.inverse {
                (target, edge.source)
            } else {
                (edge.source, target)
            };
            if take_edge(
                remaining,
                ordered,
                source,
                target,
                &edge.kind,
                edge.line.unwrap_or(0),
            ) {
                break;
            }
        }
    }
}

fn take_edge(
    remaining: &mut Vec<OrderedEdge>,
    ordered: &mut Vec<OrderedEdge>,
    source: NodeId,
    target: NodeId,
    kind: &EdgeKind,
    line: u32,
) -> bool {
    let Some(index) = remaining
        .iter()
        .position(|edge| edge.source == source && edge.target == target && edge.kind == *kind)
    else {
        return false;
    };
    let mut edge = remaining.remove(index);
    edge.line = line;
    ordered.push(edge);
    true
}

fn resolve_pending_target(
    root: &Utf8Path,
    result: &parse::BuildResult,
    node_index: &HashMap<String, NodeId>,
    edge: &PendingEdge,
) -> Option<NodeId> {
    node_index.get(&edge.target_identity).copied().or_else(|| {
        let target_path = std::path::Path::new(&edge.target_identity);
        if !target_path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
            || edge.target_identity.contains('/')
        {
            return None;
        }
        let referring_file = result.graph.node(edge.source).file_path.as_deref()?;
        resolve_bare_filename(&edge.target_identity, referring_file, root, node_index).or_else(
            || {
                result
                    .filename_index
                    .get(&edge.target_identity)
                    .filter(|paths| paths.len() == 1)
                    .and_then(|paths| node_index.get(paths[0].as_str()).copied())
            },
        )
    })
}

fn resolve_bare_filename(
    reference: &str,
    referring_file: &Utf8Path,
    root: &Utf8Path,
    node_index: &HashMap<String, NodeId>,
) -> Option<NodeId> {
    crate::resolve::resolve_file_path(reference, referring_file, root)
        .and_then(|found| node_index.get(found.as_str()).copied())
        .or_else(|| node_index.get(reference).copied())
}

fn emit_file_parent_meta(batch: &mut FactBatch, revisions: &mut RevisionCache<'_>, file: &str) {
    let parent = Utf8Path::new(file)
        .parent()
        .map_or_else(String::new, ToString::to_string);
    batch.meta.push(MetaFact {
        identity: identity_for(batch, revisions, file, file),
        handle: file.to_string(),
        key: "md.parent_dir".to_string(),
        value: parent,
    });
}

fn emit_frontmatter_meta(
    batch: &mut FactBatch,
    revisions: &mut RevisionCache<'_>,
    root: &Utf8Path,
    file: &str,
) {
    let content = std::fs::read_to_string(root.join(file)).unwrap_or_default();
    let (Some(frontmatter), _) = parse::split_frontmatter(&content) else {
        return;
    };
    let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(frontmatter) else {
        return;
    };
    let Some(mapping) = value.as_mapping() else {
        return;
    };
    let identity = identity_for(batch, revisions, file, file);
    for (key, value) in mapping {
        let Some(key) = key.as_str() else {
            continue;
        };
        for scalar in scalar_values(value) {
            batch.meta.push(MetaFact {
                identity: identity.clone(),
                handle: file.to_string(),
                key: key.to_string(),
                value: scalar,
            });
        }
    }
}

fn emit_implausible_ref_meta(
    batch: &mut FactBatch,
    revisions: &mut RevisionCache<'_>,
    result: &parse::BuildResult,
) -> Result<()> {
    for reference in &result.implausible_refs {
        // CR-D31: adapter-qualified diagnostic evidence lives in stored facts
        // even when it is not a graph relationship.
        let value = serde_json::json!({
            "value": reference.raw_value,
            "reason": reference.reason.to_string(),
            "line": reference.line,
        });
        batch.meta.push(MetaFact {
            identity: identity_for(batch, revisions, &reference.file, &reference.file),
            handle: reference.file.clone(),
            key: "md.implausible_ref".to_string(),
            value: serde_json::to_string(&value)?,
        });
    }
    Ok(())
}

fn emit_content_spans(
    batch: &mut FactBatch,
    revisions: &mut RevisionCache<'_>,
    root: &Utf8Path,
    result: &parse::BuildResult,
) {
    for (node_id, handle) in result.graph.nodes() {
        match &handle.kind {
            HandleKind::File(path) => {
                let path_str = path.as_str();
                let content = std::fs::read_to_string(root.join(path_str)).unwrap_or_default();
                let (frontmatter, body) = parse::split_frontmatter(&content);
                let start_line = frontmatter.map_or(1, |yaml| {
                    u32::try_from(yaml.lines().count())
                        .unwrap_or(u32::MAX)
                        .saturating_add(3)
                });
                let line_count = u32::try_from(body.lines().count()).unwrap_or(u32::MAX);
                let span_id = format!("{path_str}#full");
                let identity = identity_for(batch, revisions, path_str, path_str);
                batch.content.push(ContentFact {
                    identity: identity.clone(),
                    handle: path_str.to_string(),
                    span_id: span_id.clone(),
                    lines: line_count,
                    text: body.to_string(),
                    tokens: token_count(body),
                });
                batch.spans.push(SpanFact {
                    identity,
                    id: span_id,
                    handle: path_str.to_string(),
                    start_line,
                    end_line: start_line.saturating_add(line_count.saturating_sub(1)),
                    summary: result
                        .file_snippets
                        .get(path_str)
                        .cloned()
                        .unwrap_or_default(),
                });
            }
            HandleKind::Label { .. } => {
                let Some(summary) = result.label_snippets.get(&handle.id) else {
                    continue;
                };
                let file = handle.file_path.as_deref().map_or("", Utf8Path::as_str);
                let span_id = format!("{}#definition", handle.id);
                let identity = identity_for(batch, revisions, &native_id_for(handle), file);
                batch.spans.push(SpanFact {
                    identity: identity.clone(),
                    id: span_id.clone(),
                    handle: handle.id.clone(),
                    start_line: declaration_line(result, node_id, handle),
                    end_line: declaration_line(result, node_id, handle),
                    summary: summary.clone(),
                });
                batch.content.push(ContentFact {
                    identity,
                    handle: handle.id.clone(),
                    span_id,
                    lines: 1,
                    text: summary.clone(),
                    tokens: token_count(summary),
                });
            }
            _ => {}
        }
    }
}

fn emit_concerns(
    batch: &mut FactBatch,
    revisions: &mut RevisionCache<'_>,
    config: &config::AnnealConfig,
    result: &parse::BuildResult,
) {
    for (name, members) in &config.concerns {
        for member in members {
            for (_, handle) in result.graph.nodes() {
                if !concern_pattern_matches(member, &handle.id) {
                    continue;
                }
                let file = handle.file_path.as_deref().map_or("", Utf8Path::as_str);
                batch.concerns.push(ConcernFact {
                    identity: identity_for(batch, revisions, &native_id_for(handle), file),
                    name: name.clone(),
                    member: handle.id.clone(),
                });
            }
        }
    }
}

fn concern_pattern_matches(pattern: &str, handle: &str) -> bool {
    pattern == handle
        || pattern
            .strip_suffix('*')
            .is_some_and(|prefix| handle.starts_with(prefix))
}

fn identity_for(
    batch: &FactBatch,
    revisions: &mut RevisionCache<'_>,
    native_id: &str,
    file: &str,
) -> FactIdentity {
    let revision = if file.is_empty() {
        Revision::from("unknown")
    } else {
        revisions.revision_for(file)
    };
    let origin_uri = if file.is_empty() {
        format!("anneal://{native_id}")
    } else {
        format!("file://{}", revisions.root.join(file))
    };
    FactIdentity::new(
        batch.corpus.clone(),
        batch.source.clone(),
        NativeId::from(native_id),
        OriginUri::from(origin_uri),
        revision,
        batch.generation,
    )
}

fn native_id_for(handle: &Handle) -> String {
    handle
        .file_path
        .as_ref()
        .map_or_else(|| handle.id.clone(), ToString::to_string)
}

fn area_for(file: &str) -> String {
    Utf8Path::new(file)
        .components()
        .next()
        .map_or_else(String::new, |component| component.as_str().to_string())
}

fn token_count(text: &str) -> u32 {
    u32::try_from(text.split_whitespace().count()).unwrap_or(u32::MAX)
}

fn scalar_values(value: &serde_yaml_ng::Value) -> Vec<String> {
    match value {
        serde_yaml_ng::Value::String(value) => vec![strip_trailing_parenthetical(value)],
        serde_yaml_ng::Value::Number(value) => vec![value.to_string()],
        serde_yaml_ng::Value::Bool(value) => vec![value.to_string()],
        serde_yaml_ng::Value::Sequence(values) => values.iter().flat_map(scalar_values).collect(),
        _ => Vec::new(),
    }
}

fn strip_trailing_parenthetical(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(idx) = trimmed.rfind(" (")
        && trimmed.ends_with(')')
    {
        return trimmed[..idx].to_string();
    }
    trimmed.to_string()
}

struct LoadedFacts {
    graph: DiGraph,
    node_index: HashMap<String, NodeId>,
    config: config::AnnealConfig,
    lattice: crate::lattice::Lattice,
    pending_edges: Vec<PendingEdge>,
    section_ref_count: usize,
    implausible_refs: Vec<parse::ImplausibleRef>,
    file_snippets: HashMap<String, String>,
    label_snippets: HashMap<String, String>,
}

impl LoadedFacts {
    fn from_batch(root: &Utf8Path, batch: &FactBatch) -> Result<Self> {
        let config = config::load_config(root.as_std_path())?;
        let mut graph = DiGraph::new();
        let meta = metadata_by_handle(batch);
        let content_sizes = content_sizes(batch);
        let mut id_to_node = HashMap::new();

        for fact in &batch.handles {
            let handle = handle_from_fact(fact, &meta, &content_sizes, &id_to_node)?;
            let node_id = graph.add_node(handle);
            id_to_node.insert(fact.id.clone(), node_id);
        }

        let mut pending_edges = Vec::new();
        let mut section_ref_count = 0usize;
        for fact in &batch.edges {
            let Some(&source) = id_to_node.get(&fact.from) else {
                continue;
            };
            if let Some(&target) = id_to_node.get(&fact.to) {
                graph.add_edge(source, target, EdgeKind::from_name(&fact.kind));
            } else {
                if fact.to.starts_with("section:") {
                    section_ref_count += 1;
                }
                pending_edges.push(PendingEdge {
                    source,
                    target_identity: fact.to.clone(),
                    kind: EdgeKind::from_name(&fact.kind),
                    inverse: false,
                    line: (fact.line != 0).then_some(fact.line),
                });
            }
        }

        let observed_statuses = batch
            .handles
            .iter()
            .filter_map(|fact| fact.status.clone())
            .collect();
        let terminal_by_directory = batch
            .handles
            .iter()
            .filter(|fact| {
                fact.file
                    .split('/')
                    .any(|part| matches!(part, "archive" | "history" | "prior"))
            })
            .filter_map(|fact| fact.status.clone())
            .collect();
        let lattice =
            crate::lattice::infer_lattice(observed_statuses, &config, &terminal_by_directory);
        let node_index = crate::resolve::build_node_index(&graph);
        let (file_snippets, label_snippets) = snippets_from_facts(batch);
        let implausible_refs = implausible_refs_from_facts(batch);

        Ok(Self {
            graph,
            node_index,
            config,
            lattice,
            pending_edges,
            section_ref_count,
            implausible_refs,
            file_snippets,
            label_snippets,
        })
    }

    fn diagnostics(&self) -> Vec<checks::Diagnostic> {
        let check_input = checks::CheckInput {
            graph: &self.graph,
            lattice: &self.lattice,
            config: &self.config,
            unresolved_edges: &self.pending_edges,
            section_ref_count: self.section_ref_count,
            implausible_refs: self.implausible_refs.as_slice(),
            cascade_candidates: &HashMap::new(),
            previous_snapshot: None,
        };
        let mut diagnostics = checks::run_checks(&check_input);
        checks::apply_suppressions(&mut diagnostics, &self.config.suppress);
        diagnostics
    }
}

fn implausible_refs_from_facts(batch: &FactBatch) -> Vec<parse::ImplausibleRef> {
    batch
        .meta
        .iter()
        .filter(|fact| fact.key == "md.implausible_ref")
        .filter_map(|fact| {
            let value = serde_json::from_str::<serde_json::Value>(&fact.value).ok()?;
            let raw_value = value.get("value")?.as_str()?.to_string();
            let reason = value
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .and_then(parse_implausible_reason)?;
            let line = value
                .get("line")
                .and_then(serde_json::Value::as_u64)
                .and_then(|line| u32::try_from(line).ok());
            Some(parse::ImplausibleRef {
                file: fact.handle.clone(),
                raw_value,
                reason,
                line,
            })
        })
        .collect()
}

fn parse_implausible_reason(reason: &str) -> Option<ImplausibleReason> {
    match reason {
        "absolute path" => Some(ImplausibleReason::AbsolutePath),
        "wildcard pattern" => Some(ImplausibleReason::WildcardPattern),
        "comma-separated list" => Some(ImplausibleReason::CommaSeparatedList),
        "freeform prose" => Some(ImplausibleReason::FreeformProse),
        _ => None,
    }
}

fn metadata_by_handle(batch: &FactBatch) -> HashMap<String, HandleMetadata> {
    let mut by_handle = HashMap::<String, HandleMetadata>::new();
    for fact in &batch.meta {
        let metadata = by_handle.entry(fact.handle.clone()).or_default();
        match fact.key.as_str() {
            "updated" => {
                metadata.updated = chrono::NaiveDate::parse_from_str(&fact.value, "%Y-%m-%d").ok();
            }
            "superseded-by" => metadata.superseded_by = Some(fact.value.clone()),
            "depends-on" => metadata.depends_on.push(fact.value.clone()),
            "discharges" => metadata.discharges.push(fact.value.clone()),
            "verifies" => metadata.verifies.push(fact.value.clone()),
            "purpose" => metadata.purpose = Some(fact.value.clone()),
            "note" => metadata.note = Some(fact.value.clone()),
            _ => {}
        }
    }
    by_handle
}

fn content_sizes(batch: &FactBatch) -> HashMap<String, u32> {
    batch
        .content
        .iter()
        .filter(|fact| {
            std::path::Path::new(&fact.handle)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        })
        .map(|fact| {
            (
                fact.handle.clone(),
                u32::try_from(fact.text.len()).unwrap_or(u32::MAX),
            )
        })
        .collect()
}

fn handle_from_fact(
    fact: &HandleFact,
    meta: &HashMap<String, HandleMetadata>,
    content_sizes: &HashMap<String, u32>,
    id_to_node: &HashMap<String, NodeId>,
) -> Result<Handle> {
    let file_path = (!fact.file.is_empty()).then(|| Utf8PathBuf::from(fact.file.clone()));
    let metadata = meta.get(&fact.id).cloned().unwrap_or_default();
    let date = fact
        .date
        .as_deref()
        .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok());
    let kind = match fact.kind.as_str() {
        "file" => HandleKind::File(Utf8PathBuf::from(fact.id.clone())),
        "section" => HandleKind::Section {
            parent: file_path
                .as_ref()
                .and_then(|file| id_to_node.get(file.as_str()).copied())
                .unwrap_or_else(|| NodeId::new(0)),
            heading: fact
                .id
                .split('#')
                .nth(1)
                .unwrap_or(fact.id.as_str())
                .replace('-', " "),
        },
        "label" => {
            let (prefix, number) = label_parts(&fact.id)
                .with_context(|| format!("invalid label fact id {}", fact.id))?;
            HandleKind::Label { prefix, number }
        }
        "version" => HandleKind::Version {
            artifact: file_path
                .as_ref()
                .and_then(|file| id_to_node.get(file.as_str()).copied())
                .unwrap_or_else(|| NodeId::new(0)),
            version: version_number(&fact.id).unwrap_or(0),
        },
        "external" => HandleKind::External {
            url: fact.id.clone(),
        },
        other => bail!("unknown handle fact kind {other:?}"),
    };
    Ok(Handle {
        id: fact.id.clone(),
        kind,
        status: fact.status.clone(),
        file_path,
        date,
        size_bytes: content_sizes.get(&fact.id).copied(),
        metadata,
    })
}

fn label_parts(id: &str) -> Option<(String, u32)> {
    let (prefix, number) = id.rsplit_once('-')?;
    Some((prefix.to_string(), number.parse().ok()?))
}

fn version_number(id: &str) -> Option<u32> {
    id.rsplit_once("-v")?.1.parse().ok()
}

fn snippets_from_facts(batch: &FactBatch) -> (HashMap<String, String>, HashMap<String, String>) {
    let kinds = batch
        .handles
        .iter()
        .map(|fact| (fact.id.as_str(), fact.kind.as_str()))
        .collect::<HashMap<_, _>>();
    let mut files = HashMap::new();
    let mut labels = HashMap::new();
    for span in &batch.spans {
        if span.summary.is_empty() {
            continue;
        }
        match kinds.get(span.handle.as_str()).copied() {
            Some("file") => {
                files
                    .entry(span.handle.clone())
                    .or_insert(span.summary.clone());
            }
            Some("label") => {
                labels
                    .entry(span.handle.clone())
                    .or_insert(span.summary.clone());
            }
            _ => {}
        }
    }
    (files, labels)
}
