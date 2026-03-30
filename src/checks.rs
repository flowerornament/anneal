use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;

use crate::config::AnnealConfig;
use crate::graph::{DiGraph, EdgeKind};
use crate::handle::{HandleKind, NodeId};
use crate::lattice::{self, FreshnessLevel, Lattice};
use crate::parse::{ImplausibleRef, PendingEdge};

/// Structured evidence attached to diagnostics for JSON consumers (DIAG-02).
///
/// Each variant corresponds to a diagnostic code and carries the data that
/// produced the diagnostic. Human output uses the `message` string; JSON
/// consumers use `evidence` for programmatic access.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
pub(crate) enum Evidence {
    /// E001: broken reference with resolution cascade candidates.
    BrokenRef {
        target: String,
        candidates: Vec<String>,
    },
    /// W001: stale reference (active -> terminal).
    StaleRef {
        source_status: String,
        target_status: String,
    },
    /// W002: confidence gap in pipeline ordering.
    ConfidenceGap {
        source_status: String,
        source_level: usize,
        target_status: String,
        target_level: usize,
    },
    /// W004: implausible frontmatter value.
    Implausible {
        value: String,
        reason: String,
    },
}

/// Severity level for diagnostics, ordered so errors sort first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) enum Severity {
    Error = 0,
    Warning = 1,
    Info = 2,
    Suggestion = 3,
}

/// A single diagnostic produced by a check rule (CHECK-06).
///
/// Each diagnostic has a severity, error code, human message, and optional
/// file location. Format matches spec section 12.1 compiler-style output.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Diagnostic {
    pub(crate) severity: Severity,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
    pub(crate) evidence: Option<Evidence>,
}

impl Diagnostic {
    /// Print in compiler-style format per spec section 12.1:
    /// ```text
    /// error[E001]: broken reference: OQ-99 not found
    ///   -> formal-model/v17.md
    /// ```
    pub(crate) fn print_human(&self, w: &mut dyn std::io::Write) -> std::io::Result<()> {
        use crate::style::S;
        let (prefix, style) = match self.severity {
            Severity::Error => ("error", &S.error),
            Severity::Warning => ("warn", &S.warning),
            Severity::Info => ("info", &S.info),
            Severity::Suggestion => ("suggestion", &S.suggestion),
        };
        write!(
            w,
            "{}{}{}",
            style.apply_to(prefix),
            S.dim.apply_to(format_args!("[{}]", self.code)),
            format_args!(": {}", self.message),
        )?;
        if let Some(ref file) = self.file {
            write!(w, "\n  {} {file}", S.dim.apply_to("->"))?;
            if let Some(line) = self.line {
                write!(w, ":{line}")?;
            }
        }
        writeln!(w)
    }
}

// ---------------------------------------------------------------------------
// CHECK-01: Existence (KB-R1)
// ---------------------------------------------------------------------------

/// Check existence: every edge target must resolve.
///
/// Per D-01: section references (target starting with "section:") get a single
/// I001 info summary, not per-reference errors. All other unresolved pending
/// edges produce E001 errors.
fn check_existence(
    graph: &DiGraph,
    unresolved_edges: &[PendingEdge],
    section_ref_count: usize,
    cascade_candidates: &HashMap<String, Vec<String>>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

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
            evidence: None,
        });
    }

    for edge in unresolved_edges {
        if edge.target_identity.starts_with("section:") {
            continue;
        }
        let file = graph
            .node(edge.source)
            .file_path
            .as_ref()
            .map(ToString::to_string);

        let candidates = cascade_candidates
            .get(&edge.target_identity)
            .cloned()
            .unwrap_or_default();

        let candidate_msg = if candidates.is_empty() {
            String::new()
        } else {
            format!("; similar handle exists: {}", candidates.join(", "))
        };

        diagnostics.push(Diagnostic {
            severity: Severity::Error,
            code: "E001",
            message: format!(
                "broken reference: {} not found{}",
                edge.target_identity, candidate_msg
            ),
            file,
            line: edge.line,
            evidence: Some(Evidence::BrokenRef {
                target: edge.target_identity.clone(),
                candidates,
            }),
        });
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// W004: Plausibility filter
// ---------------------------------------------------------------------------

/// W004: Implausible frontmatter values that were filtered before resolution.
///
/// These are frontmatter edge targets that could not plausibly be handle
/// references: absolute paths, freeform prose, wildcard patterns, etc.
fn check_plausibility(implausible_refs: &[ImplausibleRef]) -> Vec<Diagnostic> {
    implausible_refs
        .iter()
        .map(|r| Diagnostic {
            severity: Severity::Warning,
            code: "W004",
            message: format!(
                "implausible frontmatter value {:?} ({})",
                r.raw_value, r.reason
            ),
            file: Some(r.file.clone()),
            line: None,
            evidence: Some(Evidence::Implausible {
                value: r.raw_value.clone(),
                reason: r.reason.clone(),
            }),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// CHECK-02: Staleness (KB-R2)
// ---------------------------------------------------------------------------

/// Check staleness: active handle referencing terminal handle.
///
/// For each outgoing edge, if source has an active status and target has a
/// terminal status, emit W001.
fn check_staleness(graph: &DiGraph, lattice: &Lattice) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for (node_id, handle) in graph.nodes() {
        let Some(ref source_status) = handle.status else {
            continue;
        };
        if !lattice.active.contains(source_status) {
            continue;
        }

        for edge in graph.outgoing(node_id) {
            let target = graph.node(edge.target);
            if target.is_terminal(lattice) {
                let target_status = target.status.as_deref().unwrap_or("unknown");
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    code: "W001",
                    message: format!(
                        "stale reference: {} (active) references {} ({}, terminal)",
                        handle.id, target.id, target_status
                    ),
                    file: handle.file_path.as_ref().map(ToString::to_string),
                    line: None,
                    evidence: Some(Evidence::StaleRef {
                        source_status: source_status.clone(),
                        target_status: target_status.to_string(),
                    }),
                });
            }
        }
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// CHECK-03: Confidence gap (KB-R3)
// ---------------------------------------------------------------------------

/// Check confidence gap: DependsOn edge where source state > target state.
///
/// Only applies when the lattice has a non-empty ordering and both handles
/// have statuses with known levels. Uses `state_level()` from lattice.rs.
fn check_confidence_gap(graph: &DiGraph, lattice: &Lattice) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if lattice.ordering.is_empty() {
        return diagnostics;
    }

    for (node_id, handle) in graph.nodes() {
        let Some(ref source_status) = handle.status else {
            continue;
        };
        let Some(source_level) = lattice::state_level(source_status, lattice) else {
            continue;
        };

        for edge in graph.edges_by_kind(node_id, EdgeKind::DependsOn) {
            let target = graph.node(edge.target);
            let Some(ref target_status) = target.status else {
                continue;
            };
            let Some(target_level) = lattice::state_level(target_status, lattice) else {
                continue;
            };

            if source_level > target_level {
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    code: "W002",
                    message: format!(
                        "confidence gap: {} ({}) depends on {} ({})",
                        handle.id, source_status, target.id, target_status
                    ),
                    file: handle.file_path.as_ref().map(ToString::to_string),
                    line: None,
                    evidence: Some(Evidence::ConfidenceGap {
                        source_status: source_status.clone(),
                        source_level,
                        target_status: target_status.clone(),
                        target_level,
                    }),
                });
            }
        }
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// CHECK-04: Linearity (KB-R4)
// ---------------------------------------------------------------------------

/// Check linearity: linear handles must be discharged exactly once.
///
/// Builds a set of linear namespace prefixes from config. For each Label handle
/// in a linear namespace: count incoming Discharges edges. Skip if terminal
/// (mooted). Zero = E002. Multiple = I002.
fn check_linearity(graph: &DiGraph, config: &AnnealConfig, lattice: &Lattice) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let linear_namespaces = config.handles.linear_set();

    if linear_namespaces.is_empty() {
        return diagnostics;
    }

    for (node_id, handle) in graph.nodes() {
        let HandleKind::Label { ref prefix, .. } = handle.kind else {
            continue;
        };

        if !linear_namespaces.contains(prefix.as_str()) {
            continue;
        }

        // Mooted: terminal status means obligation is automatically discharged
        if handle.is_terminal(lattice) {
            continue;
        }

        let discharge_count = graph
            .incoming(node_id)
            .iter()
            .filter(|e| e.kind == EdgeKind::Discharges)
            .count();

        if discharge_count == 0 {
            diagnostics.push(Diagnostic {
                severity: Severity::Error,
                code: "E002",
                message: format!(
                    "undischarged obligation: {} has no Discharges edge",
                    handle.id
                ),
                file: handle.file_path.as_ref().map(ToString::to_string),
                line: None,
                evidence: None,
            });
        } else if discharge_count >= 2 {
            diagnostics.push(Diagnostic {
                severity: Severity::Info,
                code: "I002",
                message: format!(
                    "multiple discharges: {} discharged {discharge_count} times (affine)",
                    handle.id
                ),
                file: handle.file_path.as_ref().map(ToString::to_string),
                line: None,
                evidence: None,
            });
        }
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// CHECK-05: Convention adoption (KB-R5)
// ---------------------------------------------------------------------------

/// Check convention adoption: warn about missing frontmatter when >50% of
/// siblings in the same directory have it.
///
/// Groups File handles by parent directory, computes adoption rate, and emits
/// W003 for files without frontmatter in high-adoption directories.
fn check_conventions(graph: &DiGraph) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Group file handles by parent directory
    // Key: directory path, Value: (total_count, with_frontmatter_count, files_without_fm)
    let mut by_dir: HashMap<String, (usize, usize, Vec<NodeId>)> = HashMap::new();

    for (node_id, handle) in graph.nodes() {
        let HandleKind::File(ref path) = handle.kind else {
            continue;
        };

        let dir = path.parent().map_or_else(String::new, ToString::to_string);

        let entry = by_dir.entry(dir).or_insert((0, 0, Vec::new()));
        entry.0 += 1; // total
        if handle.status.is_some() {
            entry.1 += 1; // with frontmatter
        } else {
            entry.2.push(node_id); // missing frontmatter
        }
    }

    for (total, with_fm, missing_nodes) in by_dir.values() {
        if *total < 2 {
            continue;
        }

        let rate = lattice::frontmatter_adoption_rate(*total, *with_fm);
        if rate <= 0.5 {
            continue;
        }

        for &node_id in missing_nodes {
            let handle = graph.node(node_id);
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                code: "W003",
                message: format!(
                    "missing frontmatter: {} has no status field ({with_fm}/{total} siblings have frontmatter)",
                    handle.id
                ),
                file: handle.file_path.as_ref().map(ToString::to_string),
                line: None,
                evidence: None,
            });
        }
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// SUGGEST-01: Orphaned handles (KB-E8)
// ---------------------------------------------------------------------------

/// Suggest orphaned handles: labels and versions with no incoming edges (D-17).
///
/// File handles are roots (always "orphaned" by definition). Section handles
/// are structural (created from headings, rarely cross-referenced). Only labels
/// and versions with no incoming edges represent genuinely disconnected knowledge.
fn suggest_orphaned(graph: &DiGraph) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for (node_id, handle) in graph.nodes() {
        // Only labels and versions — files are roots, sections are structural
        if !matches!(
            handle.kind,
            HandleKind::Label { .. } | HandleKind::Version { .. }
        ) {
            continue;
        }

        if graph.incoming(node_id).is_empty() {
            diagnostics.push(Diagnostic {
                severity: Severity::Suggestion,
                code: "S001",
                message: format!("orphaned handle: {} has no incoming edges", handle.id),
                file: handle.file_path.as_ref().map(ToString::to_string),
                line: None,
                evidence: None,
            });
        }
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// SUGGEST-02: Candidate namespaces
// ---------------------------------------------------------------------------

/// Suggest candidate namespaces: recurring label-like prefixes not yet confirmed.
///
/// Groups Label handles by prefix. Prefixes not in confirmed or rejected with
/// count >= 3 are candidates. One diagnostic per candidate prefix.
fn suggest_candidate_namespaces(graph: &DiGraph, config: &AnnealConfig) -> Vec<Diagnostic> {
    let confirmed = config.handles.confirmed_set();
    let rejected: HashSet<&str> = config.handles.rejected.iter().map(String::as_str).collect();

    // Count labels per prefix
    let mut prefix_counts: HashMap<&str, usize> = HashMap::new();
    for (_, handle) in graph.nodes() {
        if let HandleKind::Label { ref prefix, .. } = handle.kind {
            *prefix_counts.entry(prefix.as_str()).or_insert(0) += 1;
        }
    }

    let mut diagnostics = Vec::new();
    // Sort for deterministic output
    let mut candidates: Vec<_> = prefix_counts
        .into_iter()
        .filter(|(prefix, count)| {
            *count >= 3 && !confirmed.contains(prefix) && !rejected.contains(prefix)
        })
        .collect();
    candidates.sort_by_key(|(prefix, _)| *prefix);

    for (prefix, count) in candidates {
        diagnostics.push(Diagnostic {
            severity: Severity::Suggestion,
            code: "S002",
            message: format!(
                "candidate namespace: {prefix} ({count} labels found, not in confirmed namespaces)"
            ),
            file: None,
            line: None,
            evidence: None,
        });
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// SUGGEST-03: Pipeline stalls (KB-E4)
// ---------------------------------------------------------------------------

/// Suggest pipeline stalls: ordering levels with high population and no
/// DependsOn outflow to the next level.
fn suggest_pipeline_stalls(graph: &DiGraph, lattice: &Lattice) -> Vec<Diagnostic> {
    if lattice.ordering.is_empty() {
        return Vec::new();
    }

    // Group handles by their ordering level
    let mut by_level: HashMap<usize, Vec<NodeId>> = HashMap::new();
    for (node_id, handle) in graph.nodes() {
        if let Some(ref status) = handle.status
            && let Some(level) = lattice::state_level(status, lattice)
        {
            by_level.entry(level).or_default().push(node_id);
        }
    }

    let mut diagnostics = Vec::new();

    // Check each level except the last for stalls
    for level_idx in 0..lattice.ordering.len().saturating_sub(1) {
        let Some(handles_at_level) = by_level.get(&level_idx) else {
            continue;
        };

        if handles_at_level.len() < 3 {
            continue;
        }

        let next_level = level_idx + 1;

        // Count handles that have at least one DependsOn edge to a handle at the next level
        let has_outflow = handles_at_level.iter().any(|&node_id| {
            graph
                .edges_by_kind(node_id, EdgeKind::DependsOn)
                .any(|edge| {
                    let target = graph.node(edge.target);
                    if let Some(ref target_status) = target.status {
                        lattice::state_level(target_status, lattice) == Some(next_level)
                    } else {
                        false
                    }
                })
        });

        if !has_outflow {
            diagnostics.push(Diagnostic {
                severity: Severity::Suggestion,
                code: "S003",
                message: format!(
                    "pipeline stall: {} handles at status '{}' with no dependencies at next level '{}'",
                    handles_at_level.len(),
                    lattice.ordering[level_idx],
                    lattice.ordering[next_level]
                ),
                file: None,
                line: None,
                evidence: None,
            });
        }
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// SUGGEST-04: Abandoned namespaces (KB-E8)
// ---------------------------------------------------------------------------

/// Suggest abandoned namespaces: all members are terminal or stale.
///
/// A namespace is abandoned if every member is either terminal (status in
/// lattice.terminal) or stale (freshness beyond error threshold). Labels
/// with no updated date and no terminal status are NOT considered abandoned.
fn suggest_abandoned_namespaces(
    graph: &DiGraph,
    lattice: &Lattice,
    config: &AnnealConfig,
) -> Vec<Diagnostic> {
    let confirmed = config.handles.confirmed_set();

    // Group Label handles by prefix (confirmed namespaces only)
    let mut by_prefix: BTreeMap<&str, Vec<(NodeId, &crate::handle::Handle)>> = BTreeMap::new();
    for (node_id, handle) in graph.nodes() {
        if let HandleKind::Label { ref prefix, .. } = handle.kind
            && confirmed.contains(prefix.as_str())
        {
            by_prefix
                .entry(prefix.as_str())
                .or_default()
                .push((node_id, handle));
        }
    }

    let mut diagnostics = Vec::new();

    for (prefix, members) in &by_prefix {
        if members.len() < 2 {
            continue;
        }

        let all_abandoned = members.iter().all(|(_, handle)| {
            // Terminal status -> abandoned
            if handle.is_terminal(lattice) {
                return true;
            }

            // Stale beyond error threshold -> abandoned
            // Label handles don't have filesystem mtime, pass None
            let freshness =
                lattice::compute_freshness(handle.metadata.updated, None, &config.freshness);
            if freshness.level == FreshnessLevel::Stale {
                return true;
            }

            // No updated date and not terminal -> NOT abandoned (conservative)
            false
        });

        if all_abandoned {
            diagnostics.push(Diagnostic {
                severity: Severity::Suggestion,
                code: "S004",
                message: format!(
                    "abandoned namespace: all {} members of {prefix} are terminal or stale",
                    members.len()
                ),
                file: None,
                line: None,
                evidence: None,
            });
        }
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// SUGGEST-05: Concern group candidates
// ---------------------------------------------------------------------------

/// Suggest concern group candidates: label prefixes co-occurring across files.
///
/// Builds a co-occurrence map from File handles to their referenced label
/// prefixes. Pairs co-occurring in >= 3 files are candidates, unless already
/// in the same concern group.
fn suggest_concern_groups(graph: &DiGraph, config: &AnnealConfig) -> Vec<Diagnostic> {
    // Build set of existing concern group pairs for exclusion
    let mut existing_pairs: HashSet<(&str, &str)> = HashSet::new();
    for members in config.concerns.values() {
        for (i, a) in members.iter().enumerate() {
            for b in &members[i + 1..] {
                let (lo, hi) = if a <= b {
                    (a.as_str(), b.as_str())
                } else {
                    (b.as_str(), a.as_str())
                };
                existing_pairs.insert((lo, hi));
            }
        }
    }

    // For each File handle, collect label prefixes it references
    let mut file_prefixes: Vec<HashSet<&str>> = Vec::new();
    for (node_id, handle) in graph.nodes() {
        if !matches!(handle.kind, HandleKind::File(_)) {
            continue;
        }

        let mut prefixes = HashSet::new();
        for edge in graph.outgoing(node_id) {
            if matches!(edge.kind, EdgeKind::Cites | EdgeKind::DependsOn) {
                let target = graph.node(edge.target);
                if let HandleKind::Label { ref prefix, .. } = target.kind {
                    prefixes.insert(prefix.as_str());
                }
            }
        }

        if prefixes.len() >= 2 {
            file_prefixes.push(prefixes);
        }
    }

    // Count co-occurrences for all prefix pairs
    let mut pair_counts: HashMap<(&str, &str), usize> = HashMap::new();
    for prefixes in &file_prefixes {
        let mut sorted: Vec<&str> = prefixes.iter().copied().collect();
        sorted.sort_unstable();
        for (i, &a) in sorted.iter().enumerate() {
            for &b in &sorted[i + 1..] {
                *pair_counts.entry((a, b)).or_insert(0) += 1;
            }
        }
    }

    // Filter to pairs with >= 3 co-occurrences, excluding existing concern groups
    let mut candidates: Vec<((&str, &str), usize)> = pair_counts
        .into_iter()
        .filter(|((a, b), count)| *count >= 3 && !existing_pairs.contains(&(*a, *b)))
        .collect();

    // Sort by count descending, then by pair name for determinism
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    // Limit to top 5 pairs to avoid noise
    let mut diagnostics = Vec::new();
    for ((prefix_a, prefix_b), count) in candidates.into_iter().take(5) {
        diagnostics.push(Diagnostic {
            severity: Severity::Suggestion,
            code: "S005",
            message: format!(
                "concern group candidate: {prefix_a} and {prefix_b} co-occur in {count} files"
            ),
            file: None,
            line: None,
            evidence: None,
        });
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// Suggestion entry point
// ---------------------------------------------------------------------------

/// Run all five suggestion rules and return diagnostics.
pub(crate) fn run_suggestions(
    graph: &DiGraph,
    lattice: &Lattice,
    config: &AnnealConfig,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(suggest_orphaned(graph));
    diagnostics.extend(suggest_candidate_namespaces(graph, config));
    diagnostics.extend(suggest_pipeline_stalls(graph, lattice));
    diagnostics.extend(suggest_abandoned_namespaces(graph, lattice, config));
    diagnostics.extend(suggest_concern_groups(graph, config));
    diagnostics
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run all five check rules plus suggestions and return sorted diagnostics.
///
/// Diagnostics are sorted by severity: errors first, then warnings, then info,
/// then suggestions.
pub(crate) fn run_checks(
    graph: &DiGraph,
    lattice: &Lattice,
    config: &AnnealConfig,
    unresolved_edges: &[PendingEdge],
    section_ref_count: usize,
    implausible_refs: &[ImplausibleRef],
    cascade_candidates: &HashMap<String, Vec<String>>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(check_existence(
        graph,
        unresolved_edges,
        section_ref_count,
        cascade_candidates,
    ));
    diagnostics.extend(check_plausibility(implausible_refs));
    diagnostics.extend(check_staleness(graph, lattice));
    diagnostics.extend(check_confidence_gap(graph, lattice));
    diagnostics.extend(check_linearity(graph, config, lattice));
    diagnostics.extend(check_conventions(graph));
    diagnostics.extend(run_suggestions(graph, lattice, config));
    diagnostics.sort_by_key(|d| d.severity);
    diagnostics
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AnnealConfig, HandlesConfig};
    use crate::graph::DiGraph;
    use crate::handle::Handle;
    use crate::lattice::Lattice;
    use crate::parse::PendingEdge;

    fn make_lattice(active: &[&str], terminal: &[&str], ordering: &[&str]) -> Lattice {
        Lattice {
            observed_statuses: active
                .iter()
                .chain(terminal.iter())
                .copied()
                .map(String::from)
                .collect(),
            active: active.iter().copied().map(String::from).collect(),
            terminal: terminal.iter().copied().map(String::from).collect(),
            ordering: ordering.iter().copied().map(String::from).collect(),
            kind: crate::lattice::LatticeKind::Confidence,
        }
    }

    fn make_file_handle(id: &str, status: Option<&str>) -> Handle {
        Handle::test_file(id, status)
    }

    fn make_label_handle(prefix: &str, number: u32, status: Option<&str>) -> Handle {
        Handle::test_label(prefix, number, status)
    }

    // -----------------------------------------------------------------------
    // CHECK-01: Existence
    // -----------------------------------------------------------------------

    #[test]
    fn e001_for_unresolved_non_section_edge() {
        let mut graph = DiGraph::new();
        let source = graph.add_node(make_file_handle("doc.md", None));

        let unresolved = vec![PendingEdge {
            source,
            target_identity: "OQ-99".to_string(),
            kind: EdgeKind::Cites,
            inverse: false,
            line: Some(42),
        }];

        let cascade = HashMap::new();
        let diags = check_existence(&graph, &unresolved, 0, &cascade);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].code, "E001");
        assert!(diags[0].message.contains("OQ-99"));
        assert_eq!(
            diags[0].line,
            Some(42),
            "E001 diagnostic should carry PendingEdge line number"
        );
    }

    #[test]
    fn i001_for_section_refs() {
        let graph = DiGraph::new();
        let unresolved: Vec<PendingEdge> = Vec::new();

        let cascade = HashMap::new();
        let diags = check_existence(&graph, &unresolved, 42, &cascade);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
        assert_eq!(diags[0].code, "I001");
        assert!(diags[0].message.contains("42"));
    }

    // -----------------------------------------------------------------------
    // CHECK-02: Staleness
    // -----------------------------------------------------------------------

    #[test]
    fn w001_active_references_terminal() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("active.md", Some("draft")));
        let b = graph.add_node(make_file_handle("terminal.md", Some("archived")));
        graph.add_edge(a, b, EdgeKind::Cites);

        let lattice = make_lattice(&["draft"], &["archived"], &[]);

        let diags = check_staleness(&graph, &lattice);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].code, "W001");
        assert!(diags[0].message.contains("active.md"));
        assert!(diags[0].message.contains("terminal.md"));
    }

    // -----------------------------------------------------------------------
    // CHECK-03: Confidence gap
    // -----------------------------------------------------------------------

    #[test]
    fn w002_source_higher_than_target() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("formal.md", Some("formal")));
        let b = graph.add_node(make_file_handle("provisional.md", Some("provisional")));
        graph.add_edge(a, b, EdgeKind::DependsOn);

        // ordering: provisional(0) < draft(1) < formal(2)
        let lattice = make_lattice(
            &["provisional", "draft", "formal"],
            &[],
            &["provisional", "draft", "formal"],
        );

        let diags = check_confidence_gap(&graph, &lattice);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].code, "W002");
        assert!(diags[0].message.contains("formal.md"));
        assert!(diags[0].message.contains("provisional.md"));
    }

    #[test]
    fn w002_not_produced_when_ordering_empty() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(make_file_handle("formal.md", Some("formal")));
        let b = graph.add_node(make_file_handle("provisional.md", Some("provisional")));
        graph.add_edge(a, b, EdgeKind::DependsOn);

        // No ordering -- cannot determine levels
        let lattice = make_lattice(&["provisional", "formal"], &[], &[]);

        let diags = check_confidence_gap(&graph, &lattice);
        assert!(
            diags.is_empty(),
            "W002 should not be produced when lattice has no ordering"
        );
    }

    // -----------------------------------------------------------------------
    // CHECK-04: Linearity
    // -----------------------------------------------------------------------

    #[test]
    fn e002_undischarged_obligation() {
        let mut graph = DiGraph::new();
        let _label = graph.add_node(make_label_handle("OBL", 1, None));

        let config = AnnealConfig {
            handles: HandlesConfig {
                linear: vec!["OBL".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };
        let lattice = make_lattice(&[], &[], &[]);

        let diags = check_linearity(&graph, &config, &lattice);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].code, "E002");
        assert!(diags[0].message.contains("OBL-1"));
    }

    #[test]
    fn e002_not_produced_for_terminal_handle() {
        let mut graph = DiGraph::new();
        let _label = graph.add_node(make_label_handle("OBL", 1, Some("archived")));

        let config = AnnealConfig {
            handles: HandlesConfig {
                linear: vec!["OBL".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };
        let lattice = make_lattice(&[], &["archived"], &[]);

        let diags = check_linearity(&graph, &config, &lattice);
        assert!(
            diags.is_empty(),
            "E002 should not be produced for handles with terminal status (mooted)"
        );
    }

    #[test]
    fn i002_multiple_discharges() {
        let mut graph = DiGraph::new();
        let label = graph.add_node(make_label_handle("OBL", 1, None));
        let discharger1 = graph.add_node(make_file_handle("proof1.md", None));
        let discharger2 = graph.add_node(make_file_handle("proof2.md", None));
        graph.add_edge(discharger1, label, EdgeKind::Discharges);
        graph.add_edge(discharger2, label, EdgeKind::Discharges);

        let config = AnnealConfig {
            handles: HandlesConfig {
                linear: vec!["OBL".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };
        let lattice = make_lattice(&[], &[], &[]);

        let diags = check_linearity(&graph, &config, &lattice);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
        assert_eq!(diags[0].code, "I002");
        assert!(diags[0].message.contains("OBL-1"));
        assert!(diags[0].message.contains("2 times"));
    }

    // -----------------------------------------------------------------------
    // CHECK-05: Convention adoption
    // -----------------------------------------------------------------------

    #[test]
    fn w003_missing_frontmatter_above_threshold() {
        let mut graph = DiGraph::new();
        // 3 files in same dir: 2 have status, 1 does not -> 66% adoption
        let _a = graph.add_node(make_file_handle("dir/a.md", Some("draft")));
        let _b = graph.add_node(make_file_handle("dir/b.md", Some("final")));
        let _c = graph.add_node(make_file_handle("dir/c.md", None));

        let diags = check_conventions(&graph);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].code, "W003");
        assert!(diags[0].message.contains("dir/c.md"));
    }

    #[test]
    fn w003_not_produced_below_threshold() {
        let mut graph = DiGraph::new();
        // 3 files in same dir: 1 has status, 2 do not -> 33% adoption
        let _a = graph.add_node(make_file_handle("dir/a.md", Some("draft")));
        let _b = graph.add_node(make_file_handle("dir/b.md", None));
        let _c = graph.add_node(make_file_handle("dir/c.md", None));

        let diags = check_conventions(&graph);
        assert!(
            diags.is_empty(),
            "W003 should not be produced when adoption rate is <= 50%"
        );
    }

    // -----------------------------------------------------------------------
    // SUGGEST-01: Orphaned handles
    // -----------------------------------------------------------------------

    #[test]
    fn suggest_s001_for_orphaned_label() {
        let mut graph = DiGraph::new();
        // Label with no incoming edges -> orphaned
        let _label = graph.add_node(make_label_handle("OQ", 1, None));

        let diags = suggest_orphaned(&graph);
        assert_eq!(
            diags.len(),
            1,
            "Expected 1 S001 diagnostic for orphaned label"
        );
        assert_eq!(diags[0].severity, Severity::Suggestion);
        assert_eq!(diags[0].code, "S001");
        assert!(diags[0].message.contains("OQ-1"));
    }

    #[test]
    fn suggest_s001_not_for_file_handles() {
        let mut graph = DiGraph::new();
        // File handles are roots -- never orphaned
        let _file = graph.add_node(make_file_handle("doc.md", None));

        let diags = suggest_orphaned(&graph);
        assert!(
            diags.is_empty(),
            "S001 should not be produced for File handles (they are roots)"
        );
    }

    #[test]
    fn suggest_s001_not_for_handles_with_incoming() {
        let mut graph = DiGraph::new();
        let file = graph.add_node(make_file_handle("doc.md", None));
        let label = graph.add_node(make_label_handle("OQ", 1, None));
        graph.add_edge(file, label, EdgeKind::Cites);

        let diags = suggest_orphaned(&graph);
        assert!(
            diags.is_empty(),
            "S001 should not be produced for handles with incoming edges"
        );
    }

    // -----------------------------------------------------------------------
    // SUGGEST-02: Candidate namespaces
    // -----------------------------------------------------------------------

    #[test]
    fn suggest_s002_for_recurring_unconfirmed_prefix() {
        let mut graph = DiGraph::new();
        // 3 labels with prefix "NEW" -- not in confirmed namespaces
        let _a = graph.add_node(make_label_handle("NEW", 1, None));
        let _b = graph.add_node(make_label_handle("NEW", 2, None));
        let _c = graph.add_node(make_label_handle("NEW", 3, None));

        let config = AnnealConfig::default();

        let diags = suggest_candidate_namespaces(&graph, &config);
        assert_eq!(
            diags.len(),
            1,
            "Expected 1 S002 diagnostic for candidate namespace"
        );
        assert_eq!(diags[0].severity, Severity::Suggestion);
        assert_eq!(diags[0].code, "S002");
        assert!(diags[0].message.contains("NEW"));
    }

    #[test]
    fn suggest_s002_not_for_confirmed_prefix() {
        let mut graph = DiGraph::new();
        let _a = graph.add_node(make_label_handle("OQ", 1, None));
        let _b = graph.add_node(make_label_handle("OQ", 2, None));
        let _c = graph.add_node(make_label_handle("OQ", 3, None));

        let config = AnnealConfig {
            handles: HandlesConfig {
                confirmed: vec!["OQ".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };

        let diags = suggest_candidate_namespaces(&graph, &config);
        assert!(
            diags.is_empty(),
            "S002 should not be produced for already-confirmed prefixes"
        );
    }

    // -----------------------------------------------------------------------
    // SUGGEST-03: Pipeline stalls
    // -----------------------------------------------------------------------

    #[test]
    fn suggest_s003_stall_at_level_with_no_outflow() {
        let mut graph = DiGraph::new();
        // 3 handles at "draft" level, none with DependsOn to "review" level
        let _a = graph.add_node(make_file_handle("a.md", Some("draft")));
        let _b = graph.add_node(make_file_handle("b.md", Some("draft")));
        let _c = graph.add_node(make_file_handle("c.md", Some("draft")));
        // One handle at next level
        let _d = graph.add_node(make_file_handle("d.md", Some("review")));

        let lattice = make_lattice(&["draft", "review"], &[], &["draft", "review"]);

        let diags = suggest_pipeline_stalls(&graph, &lattice);
        assert_eq!(
            diags.len(),
            1,
            "Expected 1 S003 diagnostic for pipeline stall"
        );
        assert_eq!(diags[0].severity, Severity::Suggestion);
        assert_eq!(diags[0].code, "S003");
        assert!(diags[0].message.contains("draft"));
    }

    #[test]
    fn suggest_s003_empty_when_no_ordering() {
        let mut graph = DiGraph::new();
        let _a = graph.add_node(make_file_handle("a.md", Some("draft")));

        let lattice = make_lattice(&["draft"], &[], &[]);

        let diags = suggest_pipeline_stalls(&graph, &lattice);
        assert!(
            diags.is_empty(),
            "S003 should not be produced when ordering is empty (no pipeline)"
        );
    }

    // -----------------------------------------------------------------------
    // SUGGEST-04: Abandoned namespaces
    // -----------------------------------------------------------------------

    #[test]
    fn suggest_s004_all_members_terminal() {
        let mut graph = DiGraph::new();
        let _a = graph.add_node(make_label_handle("OLD", 1, Some("archived")));
        let _b = graph.add_node(make_label_handle("OLD", 2, Some("archived")));

        let config = AnnealConfig {
            handles: HandlesConfig {
                confirmed: vec!["OLD".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };
        let lattice = make_lattice(&[], &["archived"], &[]);

        let diags = suggest_abandoned_namespaces(&graph, &lattice, &config);
        assert_eq!(
            diags.len(),
            1,
            "Expected 1 S004 diagnostic for abandoned namespace"
        );
        assert_eq!(diags[0].severity, Severity::Suggestion);
        assert_eq!(diags[0].code, "S004");
        assert!(diags[0].message.contains("OLD"));
    }

    #[test]
    fn suggest_s004_all_members_stale() {
        let mut graph = DiGraph::new();
        // Create handles with old updated dates (stale beyond error threshold of 90 days)
        let mut h1 = make_label_handle("STALE", 1, Some("draft"));
        h1.metadata.updated =
            Some(chrono::NaiveDate::from_ymd_opt(2020, 1, 1).expect("valid date"));
        let mut h2 = make_label_handle("STALE", 2, Some("draft"));
        h2.metadata.updated =
            Some(chrono::NaiveDate::from_ymd_opt(2020, 1, 1).expect("valid date"));
        let _a = graph.add_node(h1);
        let _b = graph.add_node(h2);

        let config = AnnealConfig {
            handles: HandlesConfig {
                confirmed: vec!["STALE".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };
        let lattice = make_lattice(&["draft"], &[], &[]);

        let diags = suggest_abandoned_namespaces(&graph, &lattice, &config);
        assert_eq!(
            diags.len(),
            1,
            "Expected 1 S004 diagnostic for stale namespace (all members beyond freshness threshold)"
        );
        assert_eq!(diags[0].code, "S004");
    }

    #[test]
    fn suggest_s004_not_for_fresh_active_members() {
        let mut graph = DiGraph::new();
        // Fresh active handles -- should NOT be flagged
        let mut h1 = make_label_handle("ACTIVE", 1, Some("draft"));
        h1.metadata.updated = Some(chrono::Local::now().date_naive());
        let mut h2 = make_label_handle("ACTIVE", 2, Some("draft"));
        h2.metadata.updated = Some(chrono::Local::now().date_naive());
        let _a = graph.add_node(h1);
        let _b = graph.add_node(h2);

        let config = AnnealConfig {
            handles: HandlesConfig {
                confirmed: vec!["ACTIVE".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };
        let lattice = make_lattice(&["draft"], &[], &[]);

        let diags = suggest_abandoned_namespaces(&graph, &lattice, &config);
        assert!(
            diags.is_empty(),
            "S004 should not be produced when some members are fresh and active"
        );
    }

    // -----------------------------------------------------------------------
    // SUGGEST-05: Concern group candidates
    // -----------------------------------------------------------------------

    #[test]
    fn suggest_s005_cooccurring_prefixes() {
        let mut graph = DiGraph::new();
        // 3 files each reference both "OQ" and "FM" labels
        for i in 0..3 {
            let file = graph.add_node(make_file_handle(&format!("doc{i}.md"), None));
            let oq = graph.add_node(make_label_handle("OQ", i + 1, None));
            let fm = graph.add_node(make_label_handle("FM", i + 1, None));
            graph.add_edge(file, oq, EdgeKind::Cites);
            graph.add_edge(file, fm, EdgeKind::Cites);
        }

        let config = AnnealConfig::default();

        let diags = suggest_concern_groups(&graph, &config);
        assert_eq!(
            diags.len(),
            1,
            "Expected 1 S005 diagnostic for co-occurring prefixes"
        );
        assert_eq!(diags[0].severity, Severity::Suggestion);
        assert_eq!(diags[0].code, "S005");
        assert!(
            diags[0].message.contains("OQ") && diags[0].message.contains("FM"),
            "S005 message should mention both co-occurring prefixes"
        );
    }

    // -----------------------------------------------------------------------
    // run_suggestions + run_checks integration
    // -----------------------------------------------------------------------

    #[test]
    fn suggest_run_checks_includes_suggestions() {
        let mut graph = DiGraph::new();
        // Orphaned label -> S001
        let _label = graph.add_node(make_label_handle("LONE", 1, None));

        let lattice = make_lattice(&[], &[], &[]);
        let config = AnnealConfig::default();
        let unresolved: Vec<PendingEdge> = Vec::new();

        let cascade = HashMap::new();
        let diags = run_checks(&graph, &lattice, &config, &unresolved, 0, &[], &cascade);
        let suggestion_count = diags
            .iter()
            .filter(|d| d.severity == Severity::Suggestion)
            .count();
        assert!(
            suggestion_count >= 1,
            "run_checks should include suggestions from run_suggestions, got {suggestion_count}"
        );
    }

    // -----------------------------------------------------------------------
    // run_checks integration
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Evidence serialization
    // -----------------------------------------------------------------------

    #[test]
    fn evidence_none_serializes_as_null() {
        let diag = Diagnostic {
            severity: Severity::Error,
            code: "E001",
            message: "test".to_string(),
            file: None,
            line: None,
            evidence: None,
        };
        let json = serde_json::to_value(&diag).expect("serialize");
        assert!(json["evidence"].is_null(), "evidence: None should serialize as null");
        // Existing fields still present
        assert_eq!(json["code"], "E001");
        assert_eq!(json["message"], "test");
    }

    #[test]
    fn evidence_broken_ref_serializes_with_type_tag() {
        let diag = Diagnostic {
            severity: Severity::Error,
            code: "E001",
            message: "test".to_string(),
            file: Some("doc.md".to_string()),
            line: Some(10),
            evidence: Some(Evidence::BrokenRef {
                target: "OQ-99".to_string(),
                candidates: vec!["OQ-9".to_string()],
            }),
        };
        let json = serde_json::to_value(&diag).expect("serialize");
        let ev = &json["evidence"];
        assert_eq!(ev["type"], "BrokenRef");
        assert_eq!(ev["target"], "OQ-99");
        assert_eq!(ev["candidates"][0], "OQ-9");
        // Existing fields unchanged
        assert_eq!(json["severity"], "Error");
        assert_eq!(json["code"], "E001");
        assert_eq!(json["file"], "doc.md");
        assert_eq!(json["line"], 10);
    }

    #[test]
    fn check_existence_with_candidates_produces_evidence() {
        let mut graph = DiGraph::new();
        let source = graph.add_node(make_file_handle("doc.md", None));

        let unresolved = vec![PendingEdge {
            source,
            target_identity: "OQ-99".to_string(),
            kind: EdgeKind::Cites,
            inverse: false,
            line: Some(5),
        }];

        let mut cascade = HashMap::new();
        cascade.insert("OQ-99".to_string(), vec!["OQ-9".to_string()]);

        let diags = check_existence(&graph, &unresolved, 0, &cascade);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("similar handle exists: OQ-9"));
        match &diags[0].evidence {
            Some(Evidence::BrokenRef { target, candidates }) => {
                assert_eq!(target, "OQ-99");
                assert_eq!(candidates, &["OQ-9"]);
            }
            other => panic!("Expected Evidence::BrokenRef, got {other:?}"),
        }
    }

    #[test]
    fn check_existence_without_candidates_produces_empty_candidates() {
        let mut graph = DiGraph::new();
        let source = graph.add_node(make_file_handle("doc.md", None));

        let unresolved = vec![PendingEdge {
            source,
            target_identity: "MISSING-1".to_string(),
            kind: EdgeKind::Cites,
            inverse: false,
            line: None,
        }];

        let cascade = HashMap::new();
        let diags = check_existence(&graph, &unresolved, 0, &cascade);
        assert_eq!(diags.len(), 1);
        assert!(!diags[0].message.contains("similar handle"));
        match &diags[0].evidence {
            Some(Evidence::BrokenRef { target, candidates }) => {
                assert_eq!(target, "MISSING-1");
                assert!(candidates.is_empty());
            }
            other => panic!("Expected Evidence::BrokenRef with empty candidates, got {other:?}"),
        }
    }

    #[test]
    fn run_checks_sorts_by_severity() {
        let mut graph = DiGraph::new();
        // Create a scenario producing all three severities:
        // E001 from unresolved edge
        let source = graph.add_node(make_file_handle("doc.md", Some("draft")));
        // W001 from stale reference
        let terminal = graph.add_node(make_file_handle("old.md", Some("archived")));
        graph.add_edge(source, terminal, EdgeKind::Cites);

        let lattice = make_lattice(&["draft"], &["archived"], &[]);
        let config = AnnealConfig::default();

        let unresolved = vec![PendingEdge {
            source,
            target_identity: "missing-ref".to_string(),
            kind: EdgeKind::Cites,
            inverse: false,
            line: None,
        }];

        let cascade = HashMap::new();
        let diags = run_checks(&graph, &lattice, &config, &unresolved, 5, &[], &cascade);

        // Should have: E001 (error), W001 (warning), I001 (info)
        assert!(
            diags.len() >= 3,
            "Expected at least 3 diagnostics, got {}",
            diags.len()
        );

        // Verify ordering: errors before warnings before info
        let mut last_severity = Severity::Error;
        for d in &diags {
            assert!(
                d.severity >= last_severity,
                "Diagnostics not sorted by severity: {:?} came after {:?}",
                d.severity,
                last_severity
            );
            last_severity = d.severity;
        }
    }
}
