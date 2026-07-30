//! Semantic navigation over impact and file-supersession relations.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anneal_core::runtime::NumberValue;
use anneal_core::runtime::{Row, Value};
use anneal_core::{EdgeFact, FactStore, ImpactTraversalPolicy};

#[cfg(test)]
mod tests;

/// Metadata key linking a non-file handle to its containing file handle.
pub(super) const RESOLVED_FILE_META_KEY: &str = "md.resolved_file";
/// Edge kind defining the file supersession graph.
pub(super) const SUPERSEDES_EDGE_KIND: &str = "Supersedes";

#[derive(Clone, Debug, PartialEq, Eq)]
struct ImpactDependency {
    handle: String,
    depth: u32,
    kind: String,
    file: String,
    line: u32,
}

/// Render the reverse impact closure rooted at `handle` as handle-view rows.
pub(super) fn handle_impact_rows(store: &FactStore, handle: &str) -> Vec<Row> {
    compute_handle_impact(store, handle)
        .into_iter()
        .map(|dependency| impact_dependency_row(handle, dependency))
        .collect()
}

fn compute_handle_impact(store: &FactStore, handle: &str) -> Vec<ImpactDependency> {
    let traverse = ImpactTraversalPolicy::from_config_facts(store.configs());
    let mut incoming = BTreeMap::<&str, Vec<&EdgeFact>>::new();
    for edge in store.edges() {
        if traverse.traverses(edge.kind.as_str()) {
            incoming.entry(edge.to.as_str()).or_default().push(edge);
        }
    }

    let mut dependencies = Vec::new();
    let mut seen = BTreeSet::from([handle.to_string()]);
    let mut queue = VecDeque::from([(handle.to_string(), 0_u32)]);
    while let Some((current, depth)) = queue.pop_front() {
        let Some(edges) = incoming.get(current.as_str()) else {
            continue;
        };
        for edge in edges {
            if !seen.insert(edge.from.to_string()) {
                continue;
            }
            let next_depth = depth.saturating_add(1);
            dependencies.push(ImpactDependency {
                handle: edge.from.to_string(),
                depth: next_depth,
                kind: edge.kind.clone(),
                file: edge.file.clone(),
                line: edge.line,
            });
            queue.push_back((edge.from.to_string(), next_depth));
        }
    }
    dependencies
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LineageNode {
    handle: String,
    role: &'static str,
    depth: u32,
    disposition: &'static str,
    is_head: bool,
    status: Option<String>,
    file: String,
    line: u32,
    summary: String,
}

/// Render the normalized file lineage around `handle` as handle-view rows.
pub(super) fn handle_lineage_rows(store: &FactStore, handle: &str) -> Vec<Row> {
    let Some(root) = resolve_lineage_file_handle(store, handle) else {
        return Vec::new();
    };
    compute_file_lineage(store, root.as_str())
        .into_iter()
        .map(|node| lineage_node_row(handle, root.as_str(), node))
        .collect()
}

fn resolve_lineage_file_handle(store: &FactStore, handle: &str) -> Option<String> {
    let file_handles = store
        .handles()
        .iter()
        .filter(|fact| fact.kind == "file")
        .map(|fact| fact.id.as_str())
        .collect::<BTreeSet<_>>();
    if file_handles.contains(handle) {
        return Some(handle.to_string());
    }

    if let Some(resolved) = store
        .meta()
        .iter()
        .find(|fact| fact.handle.as_str() == handle && fact.key == RESOLVED_FILE_META_KEY)
        .map(|fact| fact.value.as_str())
        && file_handles.contains(resolved)
    {
        return Some(resolved.to_string());
    }

    let stem_matches = file_handles
        .iter()
        .filter(|candidate| file_handle_stem(candidate).is_some_and(|stem| stem == handle))
        .copied()
        .collect::<Vec<_>>();
    match stem_matches.as_slice() {
        [single] => Some((*single).to_string()),
        _ => None,
    }
}

fn file_handle_stem(handle: &str) -> Option<&str> {
    let file_name = handle.rsplit('/').next().unwrap_or(handle);
    file_name.strip_suffix(".md")
}

fn compute_file_lineage(store: &FactStore, root: &str) -> Vec<LineageNode> {
    let file_handles = store
        .handles()
        .iter()
        .filter(|fact| fact.kind == "file")
        .map(|fact| fact.id.as_str())
        .collect::<BTreeSet<_>>();
    let handle_index = store
        .handles()
        .iter()
        .map(|fact| (fact.id.as_str(), fact))
        .collect::<BTreeMap<_, _>>();
    let mut successors = BTreeMap::<&str, BTreeSet<&str>>::new();
    let mut predecessors = BTreeMap::<&str, BTreeSet<&str>>::new();
    for edge in store.edges() {
        if edge.kind != SUPERSEDES_EDGE_KIND
            || !file_handles.contains(edge.from.as_str())
            || !file_handles.contains(edge.to.as_str())
        {
            continue;
        }
        successors
            .entry(edge.from.as_str())
            .or_default()
            .insert(edge.to.as_str());
        predecessors
            .entry(edge.to.as_str())
            .or_default()
            .insert(edge.from.as_str());
    }

    let successor_depths = lineage_distances(root, &successors);
    let predecessor_depths = lineage_distances(root, &predecessors);
    let mut all_handles = BTreeSet::from([root]);
    all_handles.extend(successor_depths.keys().copied());
    all_handles.extend(predecessor_depths.keys().copied());

    all_handles
        .into_iter()
        .filter_map(|handle| {
            let fact = handle_index.get(handle).copied()?;
            let successor_depth = successor_depths.get(handle).copied();
            let predecessor_depth = predecessor_depths.get(handle).copied();
            let role = if handle == root {
                "root"
            } else if successor_depth.is_some() {
                "successor"
            } else if predecessor_depth.is_some() {
                "predecessor"
            } else {
                "related"
            };
            let depth = successor_depth.or(predecessor_depth).unwrap_or(0);
            let is_superseded = successors
                .get(handle)
                .is_some_and(|edges| !edges.is_empty());
            let is_head = !is_superseded
                && predecessors
                    .get(handle)
                    .is_some_and(|edges| !edges.is_empty());
            let disposition = if is_superseded {
                "superseded"
            } else if is_head {
                "current_head"
            } else {
                "current"
            };
            Some(LineageNode {
                handle: handle.to_string(),
                role,
                depth,
                disposition,
                is_head,
                status: fact.status.clone(),
                file: fact.file.clone(),
                line: fact.line,
                summary: fact.summary.clone(),
            })
        })
        .collect()
}

fn lineage_distances<'a>(
    root: &'a str,
    graph: &BTreeMap<&'a str, BTreeSet<&'a str>>,
) -> BTreeMap<&'a str, u32> {
    let mut distances = BTreeMap::new();
    let mut seen = BTreeSet::from([root]);
    let mut queue = VecDeque::from([(root, 0_u32)]);
    while let Some((current, depth)) = queue.pop_front() {
        let Some(next_nodes) = graph.get(current) else {
            continue;
        };
        for next in next_nodes {
            if !seen.insert(*next) {
                continue;
            }
            let next_depth = depth.saturating_add(1);
            distances.insert(*next, next_depth);
            queue.push_back((*next, next_depth));
        }
    }
    distances
}

fn lineage_node_row(requested: &str, normalized_root: &str, node: LineageNode) -> Row {
    Row {
        fields: BTreeMap::from([
            ("h".to_string(), Value::String(requested.to_string())),
            ("relation".to_string(), Value::String("lineage".to_string())),
            ("other".to_string(), Value::String(node.handle)),
            (
                "kind".to_string(),
                Value::String(SUPERSEDES_EDGE_KIND.to_string()),
            ),
            (
                "status".to_string(),
                node.status.map_or(Value::Null, Value::String),
            ),
            ("file".to_string(), Value::String(node.file)),
            (
                "line".to_string(),
                Value::Number(NumberValue::Int(i64::from(node.line))),
            ),
            ("summary".to_string(), Value::String(node.summary)),
            ("role".to_string(), Value::String(node.role.to_string())),
            (
                "depth".to_string(),
                Value::Number(NumberValue::Int(i64::from(node.depth))),
            ),
            (
                "disposition".to_string(),
                Value::String(node.disposition.to_string()),
            ),
            ("head".to_string(), Value::Bool(node.is_head)),
            (
                "normalized_root".to_string(),
                Value::String(normalized_root.to_string()),
            ),
        ]),
        derivation: None,
    }
}

fn impact_dependency_row(root: &str, dependency: ImpactDependency) -> Row {
    Row {
        fields: BTreeMap::from([
            ("h".to_string(), Value::String(root.to_string())),
            ("relation".to_string(), Value::String("impact".to_string())),
            ("other".to_string(), Value::String(dependency.handle)),
            ("kind".to_string(), Value::String(dependency.kind)),
            ("status".to_string(), Value::Null),
            ("file".to_string(), Value::String(dependency.file)),
            (
                "line".to_string(),
                Value::Number(NumberValue::Int(i64::from(dependency.line))),
            ),
            ("summary".to_string(), Value::String(String::new())),
            (
                "depth".to_string(),
                Value::Number(NumberValue::Int(i64::from(dependency.depth))),
            ),
        ]),
        derivation: None,
    }
}
