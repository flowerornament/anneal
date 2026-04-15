use std::collections::{BTreeMap, HashMap, HashSet};

use clap::ValueEnum;
use serde::Serialize;

use crate::config::AnnealConfig;
use crate::graph::{DiGraph, EdgeKind};
use crate::handle::{HandleKind, NodeId, resolved_file};
use crate::identity::{diagnostic_id, suggestion_id};
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
    /// W001: stale dependency (active DependsOn -> terminal).
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
    Implausible { value: String, reason: String },
    /// S001-S005: structured evidence for suggestions.
    Suggestion {
        #[serde(flatten)]
        suggestion: SuggestionEvidence,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SuggestionEvidence {
    OrphanedHandle {
        handle: String,
    },
    CandidateNamespace {
        prefix: String,
        count: usize,
    },
    PipelineStall {
        status: String,
        count: usize,
        next_status: Option<String>,
        based_on_history: bool,
    },
    AbandonedNamespace {
        prefix: String,
        member_count: usize,
        terminal_members: usize,
        stale_members: usize,
    },
    ConcernGroupCandidate {
        left_prefix: String,
        right_prefix: String,
        file_count: usize,
    },
}

/// Typed diagnostic code for exhaustive handling.
///
/// Each variant corresponds to a diagnostic code string (e.g. `E001`).  The
/// enum is `Copy` and serialises as the string form so JSON output is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum DiagnosticCode {
    E001,
    E002,
    W001,
    W002,
    W003,
    W004,
    I001,
    I002,
    S001,
    S002,
    S003,
    S004,
    S005,
}

impl DiagnosticCode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::E001 => "E001",
            Self::E002 => "E002",
            Self::W001 => "W001",
            Self::W002 => "W002",
            Self::W003 => "W003",
            Self::W004 => "W004",
            Self::I001 => "I001",
            Self::I002 => "I002",
            Self::S001 => "S001",
            Self::S002 => "S002",
            Self::S003 => "S003",
            Self::S004 => "S004",
            Self::S005 => "S005",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "E001" => Some(Self::E001),
            "E002" => Some(Self::E002),
            "W001" => Some(Self::W001),
            "W002" => Some(Self::W002),
            "W003" => Some(Self::W003),
            "W004" => Some(Self::W004),
            "I001" => Some(Self::I001),
            "I002" => Some(Self::I002),
            "S001" => Some(Self::S001),
            "S002" => Some(Self::S002),
            "S003" => Some(Self::S003),
            "S004" => Some(Self::S004),
            "S005" => Some(Self::S005),
            _ => None,
        }
    }
}

impl std::fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for DiagnosticCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Severity level for diagnostics, ordered so errors sort first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Severity {
    Error = 0,
    Warning = 1,
    Info = 2,
    Suggestion = 3,
}

impl Severity {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Suggestion => "suggestion",
        }
    }
}

/// A single diagnostic produced by a check rule (CHECK-06).
///
/// Each diagnostic has a severity, error code, human message, and optional
/// file location. Format matches spec section 12.1 compiler-style output.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Diagnostic {
    pub(crate) severity: Severity,
    pub(crate) code: DiagnosticCode,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
    pub(crate) evidence: Option<Evidence>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DiagnosticRecord {
    pub(crate) diagnostic_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) suggestion_id: Option<String>,
    pub(crate) severity: String,
    pub(crate) code: DiagnosticCode,
    pub(crate) message: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
    pub(crate) evidence: Option<Evidence>,
}

impl DiagnosticRecord {
    pub(crate) fn from_diagnostic(diagnostic: &Diagnostic) -> Self {
        Self {
            diagnostic_id: diagnostic_id(diagnostic),
            suggestion_id: suggestion_id(diagnostic),
            severity: diagnostic.severity.as_str().to_string(),
            code: diagnostic.code,
            message: diagnostic.message.clone(),
            file: diagnostic.file.clone(),
            line: diagnostic.line,
            evidence: diagnostic.evidence.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DiagnosticSelection {
    pub(crate) existence: bool,
    pub(crate) plausibility: bool,
    pub(crate) staleness: bool,
    pub(crate) confidence_gap: bool,
    pub(crate) linearity: bool,
    pub(crate) conventions: bool,
    pub(crate) suggestions: bool,
}

impl DiagnosticSelection {
    pub(crate) fn all() -> Self {
        Self {
            existence: true,
            plausibility: true,
            staleness: true,
            confidence_gap: true,
            linearity: true,
            conventions: true,
            suggestions: true,
        }
    }

    pub(crate) fn none() -> Self {
        Self {
            existence: false,
            plausibility: false,
            staleness: false,
            confidence_gap: false,
            linearity: false,
            conventions: false,
            suggestions: false,
        }
    }

    pub(crate) fn includes_suggestions(self) -> bool {
        self.suggestions
    }

    pub(crate) fn widen_for_stale_alias(&mut self) {
        self.staleness = true;
    }

    pub(crate) fn widen_for_obligation_alias(&mut self) {
        self.linearity = true;
    }

    pub(crate) fn suggestions_only(code: Option<&str>) -> Self {
        let mut selection = Self::none();
        if let Some(code) = code {
            selection.widen_for_code(code);
            if selection.includes_suggestions() {
                selection
            } else {
                Self::none()
            }
        } else {
            Self {
                suggestions: true,
                ..Self::none()
            }
        }
    }

    pub(crate) fn widen_for_code(&mut self, code: &str) {
        if let Some(parsed) = DiagnosticCode::parse(code) {
            match diagnostic_descriptor(parsed).family {
                DiagnosticFamily::Existence => self.existence = true,
                DiagnosticFamily::Plausibility => self.plausibility = true,
                DiagnosticFamily::Staleness => self.staleness = true,
                DiagnosticFamily::ConfidenceGap => self.confidence_gap = true,
                DiagnosticFamily::Linearity => self.linearity = true,
                DiagnosticFamily::Conventions => self.conventions = true,
                DiagnosticFamily::Suggestion => self.suggestions = true,
            }
        }
    }

    pub(crate) fn widen_for_severity(&mut self, severity: Severity) {
        match severity {
            Severity::Error | Severity::Info => {
                self.existence = true;
                self.linearity = true;
            }
            Severity::Warning => {
                self.plausibility = true;
                self.staleness = true;
                self.confidence_gap = true;
                self.conventions = true;
            }
            Severity::Suggestion => self.suggestions = true,
        }
    }
}

pub(crate) fn is_stale_code(code: DiagnosticCode) -> bool {
    diagnostic_descriptor(code).stale_alias
}

pub(crate) fn is_obligation_code(code: DiagnosticCode) -> bool {
    diagnostic_descriptor(code).obligation_alias
}

pub(crate) fn diagnostic_rule_name(code: DiagnosticCode) -> &'static str {
    diagnostic_descriptor(code).rule_name
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiagnosticFamily {
    Existence,
    Plausibility,
    Staleness,
    ConfidenceGap,
    Linearity,
    Conventions,
    Suggestion,
}

#[derive(Clone, Copy, Debug)]
struct DiagnosticDescriptor {
    family: DiagnosticFamily,
    rule_name: &'static str,
    stale_alias: bool,
    obligation_alias: bool,
}

fn diagnostic_descriptor(code: DiagnosticCode) -> DiagnosticDescriptor {
    match code {
        DiagnosticCode::I001 | DiagnosticCode::E001 => DiagnosticDescriptor {
            family: DiagnosticFamily::Existence,
            rule_name: "KB-R1 existence",
            stale_alias: false,
            obligation_alias: false,
        },
        DiagnosticCode::W004 => DiagnosticDescriptor {
            family: DiagnosticFamily::Plausibility,
            rule_name: "plausibility filter",
            stale_alias: false,
            obligation_alias: false,
        },
        DiagnosticCode::W001 => DiagnosticDescriptor {
            family: DiagnosticFamily::Staleness,
            rule_name: "KB-R2 staleness",
            stale_alias: true,
            obligation_alias: false,
        },
        DiagnosticCode::W002 => DiagnosticDescriptor {
            family: DiagnosticFamily::ConfidenceGap,
            rule_name: "KB-R3 confidence gap",
            stale_alias: false,
            obligation_alias: false,
        },
        DiagnosticCode::E002 | DiagnosticCode::I002 => DiagnosticDescriptor {
            family: DiagnosticFamily::Linearity,
            rule_name: "KB-R4 linearity",
            stale_alias: false,
            obligation_alias: true,
        },
        DiagnosticCode::W003 => DiagnosticDescriptor {
            family: DiagnosticFamily::Conventions,
            rule_name: "KB-R5 convention adoption",
            stale_alias: false,
            obligation_alias: false,
        },
        DiagnosticCode::S001 => DiagnosticDescriptor {
            family: DiagnosticFamily::Suggestion,
            rule_name: "SUGGEST-01 orphaned handles",
            stale_alias: false,
            obligation_alias: false,
        },
        DiagnosticCode::S002 => DiagnosticDescriptor {
            family: DiagnosticFamily::Suggestion,
            rule_name: "SUGGEST-02 candidate namespaces",
            stale_alias: false,
            obligation_alias: false,
        },
        DiagnosticCode::S003 => DiagnosticDescriptor {
            family: DiagnosticFamily::Suggestion,
            rule_name: "SUGGEST-03 pipeline stalls",
            stale_alias: false,
            obligation_alias: false,
        },
        DiagnosticCode::S004 => DiagnosticDescriptor {
            family: DiagnosticFamily::Suggestion,
            rule_name: "SUGGEST-04 abandoned namespaces",
            stale_alias: false,
            obligation_alias: false,
        },
        DiagnosticCode::S005 => DiagnosticDescriptor {
            family: DiagnosticFamily::Suggestion,
            rule_name: "SUGGEST-05 concern group candidates",
            stale_alias: false,
            obligation_alias: false,
        },
    }
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

pub(crate) fn confidence_gap_levels(
    edge_kind: &EdgeKind,
    source_level: Option<usize>,
    target_level: Option<usize>,
) -> Option<(usize, usize)> {
    if *edge_kind != EdgeKind::DependsOn {
        return None;
    }
    let (Some(source_level), Some(target_level)) = (source_level, target_level) else {
        return None;
    };
    (source_level > target_level).then_some((source_level, target_level))
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
    section_ref_file: Option<&str>,
    cascade_candidates: &HashMap<String, Vec<String>>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut bare_filename_index: Option<HashMap<String, Vec<String>>> = None;

    if section_ref_count > 0 {
        diagnostics.push(Diagnostic {
            severity: Severity::Info,
            code: DiagnosticCode::I001,
            message: format!(
                "{section_ref_count} section references use section notation, \
                 not resolvable to heading slugs"
            ),
            file: section_ref_file.map(ToString::to_string),
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

        let bare_filename_candidates = if edge.target_identity.contains('/') {
            Vec::new()
        } else {
            let index = bare_filename_index.get_or_insert_with(|| build_bare_filename_index(graph));
            bare_filename_candidates(index, &edge.target_identity)
        };

        let candidates = merge_candidates(
            cascade_candidates
                .get(&edge.target_identity)
                .cloned()
                .unwrap_or_default(),
            bare_filename_candidates,
        );

        let candidate_msg = if candidates.is_empty() {
            String::new()
        } else {
            format!(
                "; {}",
                format_candidate_suggestion(&edge.target_identity, &candidates)
            )
        };

        diagnostics.push(Diagnostic {
            severity: Severity::Error,
            code: DiagnosticCode::E001,
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

fn build_bare_filename_index(graph: &DiGraph) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for (_, handle) in graph.nodes() {
        let HandleKind::File(path) = &handle.kind else {
            continue;
        };
        let Some(filename) = path.file_name() else {
            continue;
        };
        index
            .entry(filename.to_string())
            .or_default()
            .push(path.as_str().to_string());
    }
    index
}

fn bare_filename_candidates(
    bare_filename_index: &HashMap<String, Vec<String>>,
    target: &str,
) -> Vec<String> {
    if target.contains('/') {
        return Vec::new();
    }

    bare_filename_index
        .get(target)
        .into_iter()
        .flatten()
        .filter(|path| path.as_str() != target)
        .cloned()
        .collect()
}

fn merge_candidates(primary: Vec<String>, secondary: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    primary
        .into_iter()
        .chain(secondary)
        .filter(|candidate| seen.insert(candidate.clone()))
        .collect()
}

fn format_candidate_suggestion(target: &str, candidates: &[String]) -> String {
    let is_bare_filename = !target.contains('/')
        && std::path::Path::new(target)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));

    if is_bare_filename {
        match candidates {
            [candidate] => return format!("did you mean {candidate}?"),
            [first, rest @ ..] if rest.iter().all(|candidate| candidate.contains('/')) => {
                return format!("did you mean one of: {}?", candidates.join(", "));
            }
            _ => {}
        }
    }

    format!("similar handle exists: {}", candidates.join(", "))
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
            code: DiagnosticCode::W004,
            message: format!(
                "implausible frontmatter value {:?} ({})",
                r.raw_value, r.reason
            ),
            file: Some(r.file.clone()),
            line: r.line,
            evidence: Some(Evidence::Implausible {
                value: r.raw_value.clone(),
                reason: r.reason.to_string(),
            }),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// CHECK-02: Staleness (KB-R2)
// ---------------------------------------------------------------------------

/// Check staleness: active handle with DependsOn edge to terminal handle.
///
/// For each outgoing DependsOn edge, if source has an active status and target has a
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
            if edge.kind != EdgeKind::DependsOn {
                continue;
            }
            let target = graph.node(edge.target);
            if target.is_terminal(lattice) {
                let target_status = target.status.as_deref().unwrap_or("unknown");
                let file = resolved_file(handle, graph)
                    .or_else(|| resolved_file(target, graph))
                    .map(ToString::to_string);

                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    code: DiagnosticCode::W001,
                    message: format!(
                        "stale dependency: {} (active) depends on {} ({}, terminal)",
                        handle.id, target.id, target_status
                    ),
                    file,
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

    // Build state-level lookup once to avoid repeated linear scans.
    let state_levels: HashMap<&str, usize> = lattice
        .ordering
        .iter()
        .enumerate()
        .map(|(index, status)| (status.as_str(), index))
        .collect();

    for (node_id, handle) in graph.nodes() {
        let Some(ref source_status) = handle.status else {
            continue;
        };
        let Some(&source_level) = state_levels.get(source_status.as_str()) else {
            continue;
        };

        for edge in graph.edges_by_kind(node_id, EdgeKind::DependsOn) {
            let target = graph.node(edge.target);
            let Some(ref target_status) = target.status else {
                continue;
            };
            let Some(&target_level) = state_levels.get(target_status.as_str()) else {
                continue;
            };

            if let Some((source_level, target_level)) =
                confidence_gap_levels(&EdgeKind::DependsOn, Some(source_level), Some(target_level))
            {
                let file = resolved_file(handle, graph)
                    .or_else(|| resolved_file(target, graph))
                    .map(ToString::to_string);
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    code: DiagnosticCode::W002,
                    message: format!(
                        "confidence gap: {} ({}) depends on {} ({})",
                        handle.id, source_status, target.id, target_status
                    ),
                    file,
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

        // Label file_path, or fall back to first incoming edge source's file
        let file = handle
            .file_path
            .as_ref()
            .map(ToString::to_string)
            .or_else(|| {
                graph.incoming(node_id).iter().find_map(|edge| {
                    graph
                        .node(edge.source)
                        .file_path
                        .as_ref()
                        .map(ToString::to_string)
                })
            });

        if discharge_count == 0 {
            diagnostics.push(Diagnostic {
                severity: Severity::Error,
                code: DiagnosticCode::E002,
                message: format!(
                    "undischarged obligation: {} has no Discharges edge",
                    handle.id
                ),
                file: file.clone(),
                line: None,
                evidence: None,
            });
        } else if discharge_count >= 2 {
            diagnostics.push(Diagnostic {
                severity: Severity::Info,
                code: DiagnosticCode::I002,
                message: format!(
                    "multiple discharges: {} discharged {discharge_count} times (affine)",
                    handle.id
                ),
                file,
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
                code: DiagnosticCode::W003,
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
            // Use handle's own file_path, or fall back to artifact file
            // (version handles), or any reachable edge target's file
            let file = handle
                .file_path
                .as_ref()
                .map(ToString::to_string)
                .or_else(|| {
                    // For Version handles, the artifact field points to the parent file
                    if let HandleKind::Version { artifact, .. } = &handle.kind {
                        return graph
                            .node(*artifact)
                            .file_path
                            .as_ref()
                            .map(ToString::to_string);
                    }
                    // Fall back to first outgoing edge target's file
                    graph.outgoing(node_id).iter().find_map(|edge| {
                        graph
                            .node(edge.target)
                            .file_path
                            .as_ref()
                            .map(ToString::to_string)
                    })
                });

            diagnostics.push(Diagnostic {
                severity: Severity::Suggestion,
                code: DiagnosticCode::S001,
                message: format!("orphaned handle: {} has no incoming edges", handle.id),
                file,
                line: None,
                evidence: Some(Evidence::Suggestion {
                    suggestion: SuggestionEvidence::OrphanedHandle {
                        handle: handle.id.clone(),
                    },
                }),
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
    let mut prefix_counts: HashMap<&str, (usize, Option<String>)> = HashMap::new();
    for (_, handle) in graph.nodes() {
        if let HandleKind::Label { ref prefix, .. } = handle.kind {
            let entry = prefix_counts
                .entry(prefix.as_str())
                .or_insert((0, handle.file_path.as_ref().map(ToString::to_string)));
            entry.0 += 1;
            if entry.1.is_none() {
                entry.1 = handle.file_path.as_ref().map(ToString::to_string);
            }
        }
    }

    let mut diagnostics = Vec::new();
    // Sort for deterministic output
    let mut candidates: Vec<_> = prefix_counts
        .into_iter()
        .filter(|(prefix, (count, _))| {
            *count >= 3 && !confirmed.contains(prefix) && !rejected.contains(prefix)
        })
        .collect();
    candidates.sort_by_key(|(prefix, _)| *prefix);

    for (prefix, (count, representative_file)) in candidates {
        diagnostics.push(Diagnostic {
            severity: Severity::Suggestion,
            code: DiagnosticCode::S002,
            message: format!(
                "candidate namespace: {prefix} ({count} labels found, not in confirmed namespaces)"
            ),
            file: representative_file,
            line: None,
            evidence: Some(Evidence::Suggestion {
                suggestion: SuggestionEvidence::CandidateNamespace {
                    prefix: prefix.to_string(),
                    count,
                },
            }),
        });
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// SUGGEST-03: Pipeline stalls (KB-E4)
// ---------------------------------------------------------------------------

/// Suggest pipeline stalls: ordering levels with high population and no
/// forward movement signal.
fn suggest_pipeline_stalls(
    graph: &DiGraph,
    lattice: &Lattice,
    previous_snapshot: Option<&crate::snapshot::Snapshot>,
) -> Vec<Diagnostic> {
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

        let status_name = &lattice.ordering[level_idx];
        let is_stall = if let Some(previous) = previous_snapshot {
            let prev_count = previous.states.get(status_name).copied().unwrap_or(0);
            let current_count = handles_at_level.len();
            current_count >= prev_count && prev_count >= 3
        } else {
            let next_level = level_idx + 1;
            !handles_at_level.iter().any(|&node_id| {
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
            })
        };

        if is_stall {
            let representative = handles_at_level
                .first()
                .and_then(|&nid| graph.node(nid).file_path.as_ref().map(ToString::to_string));

            let message = if previous_snapshot.is_some() {
                format!(
                    "pipeline stall at '{status_name}': {} handles (unchanged from previous snapshot)",
                    handles_at_level.len()
                )
            } else {
                let next_level = level_idx + 1;
                format!(
                    "pipeline stall: {} handles at status '{}' with no dependencies at next level '{}'",
                    handles_at_level.len(),
                    status_name,
                    lattice.ordering[next_level]
                )
            };

            diagnostics.push(Diagnostic {
                severity: Severity::Suggestion,
                code: DiagnosticCode::S003,
                message,
                file: representative,
                line: None,
                evidence: Some(Evidence::Suggestion {
                    suggestion: SuggestionEvidence::PipelineStall {
                        status: status_name.clone(),
                        count: handles_at_level.len(),
                        next_status: previous_snapshot
                            .is_none()
                            .then(|| lattice.ordering[level_idx + 1].clone()),
                        based_on_history: previous_snapshot.is_some(),
                    },
                }),
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

        let mut terminal_members = 0;
        let mut stale_members = 0;
        let all_abandoned = members.iter().all(|(_, handle)| {
            // Terminal status -> abandoned
            if handle.is_terminal(lattice) {
                terminal_members += 1;
                return true;
            }

            // Stale beyond error threshold -> abandoned
            // Label handles don't have filesystem mtime, pass None
            let freshness =
                lattice::compute_freshness(handle.metadata.updated, None, &config.freshness);
            if freshness.level == FreshnessLevel::Stale {
                stale_members += 1;
                return true;
            }

            // No updated date and not terminal -> NOT abandoned (conservative)
            false
        });

        if all_abandoned {
            let representative = members
                .first()
                .and_then(|(_, handle)| handle.file_path.as_ref().map(ToString::to_string));

            diagnostics.push(Diagnostic {
                severity: Severity::Suggestion,
                code: DiagnosticCode::S004,
                message: format!(
                    "abandoned namespace: all {} members of {prefix} are terminal or stale",
                    members.len()
                ),
                file: representative,
                line: None,
                evidence: Some(Evidence::Suggestion {
                    suggestion: SuggestionEvidence::AbandonedNamespace {
                        prefix: (*prefix).to_string(),
                        member_count: members.len(),
                        terminal_members,
                        stale_members,
                    },
                }),
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
    let mut file_prefixes: Vec<(HashSet<&str>, Option<String>)> = Vec::new();
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
            file_prefixes.push((prefixes, handle.file_path.as_ref().map(ToString::to_string)));
        }
    }

    // Count co-occurrences for all prefix pairs
    type PairCount<'a> = HashMap<(&'a str, &'a str), (usize, Option<String>)>;
    let mut pair_counts: PairCount<'_> = HashMap::new();
    for (prefixes, representative_file) in &file_prefixes {
        let mut sorted: Vec<&str> = prefixes.iter().copied().collect();
        sorted.sort_unstable();
        for (i, &a) in sorted.iter().enumerate() {
            for &b in &sorted[i + 1..] {
                let entry = pair_counts
                    .entry((a, b))
                    .or_insert((0, representative_file.clone()));
                entry.0 += 1;
                if entry.1.is_none() {
                    entry.1.clone_from(representative_file);
                }
            }
        }
    }

    // Filter to pairs with >= 3 co-occurrences, excluding existing concern groups
    type PairCandidate<'a> = Vec<((&'a str, &'a str), (usize, Option<String>))>;
    let mut candidates: PairCandidate<'_> = pair_counts
        .into_iter()
        .filter(|((a, b), (count, _))| *count >= 3 && !existing_pairs.contains(&(*a, *b)))
        .collect();

    // Sort by count descending, then by pair name for determinism
    candidates.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));

    // Limit to top 5 pairs to avoid noise
    let mut diagnostics = Vec::new();
    for ((prefix_a, prefix_b), (count, representative_file)) in candidates.into_iter().take(5) {
        diagnostics.push(Diagnostic {
            severity: Severity::Suggestion,
            code: DiagnosticCode::S005,
            message: format!(
                "concern group candidate: {prefix_a} and {prefix_b} co-occur in {count} files"
            ),
            file: representative_file,
            line: None,
            evidence: Some(Evidence::Suggestion {
                suggestion: SuggestionEvidence::ConcernGroupCandidate {
                    left_prefix: prefix_a.to_string(),
                    right_prefix: prefix_b.to_string(),
                    file_count: count,
                },
            }),
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
    previous_snapshot: Option<&crate::snapshot::Snapshot>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(suggest_orphaned(graph));
    diagnostics.extend(suggest_candidate_namespaces(graph, config));
    diagnostics.extend(suggest_pipeline_stalls(graph, lattice, previous_snapshot));
    diagnostics.extend(suggest_abandoned_namespaces(graph, lattice, config));
    diagnostics.extend(suggest_concern_groups(graph, config));
    diagnostics
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Bundled inputs for [`run_checks`] and [`run_checks_with_selection`].
pub(crate) struct CheckInput<'a> {
    pub(crate) graph: &'a DiGraph,
    pub(crate) lattice: &'a Lattice,
    pub(crate) config: &'a AnnealConfig,
    pub(crate) unresolved_edges: &'a [PendingEdge],
    pub(crate) section_ref_count: usize,
    pub(crate) section_ref_file: Option<&'a str>,
    pub(crate) implausible_refs: &'a [ImplausibleRef],
    pub(crate) cascade_candidates: &'a HashMap<String, Vec<String>>,
    pub(crate) previous_snapshot: Option<&'a crate::snapshot::Snapshot>,
}

/// Run all five check rules plus suggestions and return sorted diagnostics.
///
/// Diagnostics are sorted by severity: errors first, then warnings, then info,
/// then suggestions.
pub(crate) fn run_checks(input: &CheckInput<'_>) -> Vec<Diagnostic> {
    run_checks_with_selection(input, DiagnosticSelection::all())
}

pub(crate) fn run_checks_with_selection(
    input: &CheckInput<'_>,
    selection: DiagnosticSelection,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if selection.existence {
        diagnostics.extend(check_existence(
            input.graph,
            input.unresolved_edges,
            input.section_ref_count,
            input.section_ref_file,
            input.cascade_candidates,
        ));
    }
    if selection.plausibility {
        diagnostics.extend(check_plausibility(input.implausible_refs));
    }
    if selection.staleness {
        diagnostics.extend(check_staleness(input.graph, input.lattice));
    }
    if selection.confidence_gap {
        diagnostics.extend(check_confidence_gap(input.graph, input.lattice));
    }
    if selection.linearity {
        diagnostics.extend(check_linearity(input.graph, input.config, input.lattice));
    }
    if selection.conventions {
        diagnostics.extend(check_conventions(input.graph));
    }
    if selection.suggestions {
        diagnostics.extend(run_suggestions(
            input.graph,
            input.lattice,
            input.config,
            input.previous_snapshot,
        ));
    }
    diagnostics.sort_by_key(|d| d.severity);
    diagnostics
}

pub(crate) fn apply_suppressions(
    diagnostics: &mut Vec<Diagnostic>,
    suppress: &crate::config::SuppressConfig,
) {
    if suppress.codes.is_empty() && suppress.rules.is_empty() {
        return;
    }

    diagnostics.retain(|diagnostic| {
        if suppress
            .codes
            .iter()
            .any(|code| code == diagnostic.code.as_str())
        {
            return false;
        }

        for rule in &suppress.rules {
            if diagnostic.code.as_str() == rule.code.as_str()
                && diagnostic.message.contains(&rule.target)
            {
                return false;
            }
        }

        true
    });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AnnealConfig, HandlesConfig, SuppressConfig, SuppressRule};
    use crate::graph::DiGraph;
    use crate::handle::Handle;
    use crate::lattice::Lattice;
    use crate::parse::PendingEdge;
    use crate::snapshot::{
        DiagnosticCounts, EdgeCounts, HandleCounts, NamespaceStats, ObligationCounts, Snapshot,
    };

    // -----------------------------------------------------------------------
    // CHECK-01: Existence
    // -----------------------------------------------------------------------

    #[test]
    fn e001_for_unresolved_non_section_edge() {
        let mut graph = DiGraph::new();
        let source = graph.add_node(Handle::test_file("doc.md", None));

        let unresolved = vec![PendingEdge {
            source,
            target_identity: "OQ-99".to_string(),
            kind: EdgeKind::Cites,
            inverse: false,
            line: Some(42),
        }];

        let cascade = HashMap::new();
        let diags = check_existence(&graph, &unresolved, 0, None, &cascade);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].code, DiagnosticCode::E001);
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
        let diags = check_existence(&graph, &unresolved, 42, Some("doc.md"), &cascade);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
        assert_eq!(diags[0].code, DiagnosticCode::I001);
        assert_eq!(
            diags[0].file,
            Some("doc.md".to_string()),
            "I001 should carry representative file"
        );
        assert_eq!(
            diags[0].line, None,
            "I001 line should be None (corpus-level)"
        );
        assert!(diags[0].message.contains("42"));
    }

    // -----------------------------------------------------------------------
    // CHECK-02: Staleness
    // -----------------------------------------------------------------------

    #[test]
    fn w001_active_depends_on_terminal() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("active.md", Some("draft")));
        let b = graph.add_node(Handle::test_file("terminal.md", Some("archived")));
        graph.add_edge(a, b, EdgeKind::DependsOn);

        let lattice = Lattice::test_with_ordering(&["draft"], &["archived"], &[]);

        let diags = check_staleness(&graph, &lattice);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].code, DiagnosticCode::W001);
        assert!(diags[0].message.contains("active.md"));
        assert!(diags[0].message.contains("terminal.md"));
    }

    #[test]
    fn w001_not_emitted_for_cites_edge() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("synthesis.md", Some("draft")));
        let b = graph.add_node(Handle::test_file("research.md", Some("archived")));
        graph.add_edge(a, b, EdgeKind::Cites);

        let lattice = Lattice::test_with_ordering(&["draft"], &["archived"], &[]);

        let diags = check_staleness(&graph, &lattice);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn w001_not_emitted_for_custom_edge() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("active.md", Some("draft")));
        let b = graph.add_node(Handle::test_file("terminal.md", Some("archived")));
        graph.add_edge(a, b, EdgeKind::Custom("Synthesizes".to_string()));

        let lattice = Lattice::test_with_ordering(&["draft"], &["archived"], &[]);

        let diags = check_staleness(&graph, &lattice);
        assert_eq!(diags.len(), 0);
    }

    // -----------------------------------------------------------------------
    // CHECK-03: Confidence gap
    // -----------------------------------------------------------------------

    #[test]
    fn w002_source_higher_than_target() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("formal.md", Some("formal")));
        let b = graph.add_node(Handle::test_file("provisional.md", Some("provisional")));
        graph.add_edge(a, b, EdgeKind::DependsOn);

        // ordering: provisional(0) < draft(1) < formal(2)
        let lattice = Lattice::test_with_ordering(
            &["provisional", "draft", "formal"],
            &[],
            &["provisional", "draft", "formal"],
        );

        let diags = check_confidence_gap(&graph, &lattice);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].code, DiagnosticCode::W002);
        assert!(diags[0].message.contains("formal.md"));
        assert!(diags[0].message.contains("provisional.md"));
    }

    #[test]
    fn w002_not_produced_when_ordering_empty() {
        let mut graph = DiGraph::new();
        let a = graph.add_node(Handle::test_file("formal.md", Some("formal")));
        let b = graph.add_node(Handle::test_file("provisional.md", Some("provisional")));
        graph.add_edge(a, b, EdgeKind::DependsOn);

        // No ordering -- cannot determine levels
        let lattice = Lattice::test_with_ordering(&["provisional", "formal"], &[], &[]);

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
        let _label = graph.add_node(Handle::test_label("OBL", 1, None));

        let config = AnnealConfig {
            handles: HandlesConfig {
                linear: vec!["OBL".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };
        let lattice = Lattice::test_with_ordering(&[], &[], &[]);

        let diags = check_linearity(&graph, &config, &lattice);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].code, DiagnosticCode::E002);
        assert!(diags[0].message.contains("OBL-1"));
    }

    #[test]
    fn e002_not_produced_for_terminal_handle() {
        let mut graph = DiGraph::new();
        let _label = graph.add_node(Handle::test_label("OBL", 1, Some("archived")));

        let config = AnnealConfig {
            handles: HandlesConfig {
                linear: vec!["OBL".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };
        let lattice = Lattice::test_with_ordering(&[], &["archived"], &[]);

        let diags = check_linearity(&graph, &config, &lattice);
        assert!(
            diags.is_empty(),
            "E002 should not be produced for handles with terminal status (mooted)"
        );
    }

    #[test]
    fn i002_multiple_discharges() {
        let mut graph = DiGraph::new();
        let label = graph.add_node(Handle::test_label("OBL", 1, None));
        let discharger1 = graph.add_node(Handle::test_file("proof1.md", None));
        let discharger2 = graph.add_node(Handle::test_file("proof2.md", None));
        graph.add_edge(discharger1, label, EdgeKind::Discharges);
        graph.add_edge(discharger2, label, EdgeKind::Discharges);

        let config = AnnealConfig {
            handles: HandlesConfig {
                linear: vec!["OBL".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };
        let lattice = Lattice::test_with_ordering(&[], &[], &[]);

        let diags = check_linearity(&graph, &config, &lattice);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
        assert_eq!(diags[0].code, DiagnosticCode::I002);
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
        let _a = graph.add_node(Handle::test_file("dir/a.md", Some("draft")));
        let _b = graph.add_node(Handle::test_file("dir/b.md", Some("final")));
        let _c = graph.add_node(Handle::test_file("dir/c.md", None));

        let diags = check_conventions(&graph);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].code, DiagnosticCode::W003);
        assert!(diags[0].message.contains("dir/c.md"));
    }

    #[test]
    fn w003_not_produced_below_threshold() {
        let mut graph = DiGraph::new();
        // 3 files in same dir: 1 has status, 2 do not -> 33% adoption
        let _a = graph.add_node(Handle::test_file("dir/a.md", Some("draft")));
        let _b = graph.add_node(Handle::test_file("dir/b.md", None));
        let _c = graph.add_node(Handle::test_file("dir/c.md", None));

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
        let _label = graph.add_node(Handle::test_label("OQ", 1, None));

        let diags = suggest_orphaned(&graph);
        assert_eq!(
            diags.len(),
            1,
            "Expected 1 S001 diagnostic for orphaned label"
        );
        assert_eq!(diags[0].severity, Severity::Suggestion);
        assert_eq!(diags[0].code, DiagnosticCode::S001);
        assert!(diags[0].message.contains("OQ-1"));
        match &diags[0].evidence {
            Some(Evidence::Suggestion {
                suggestion: SuggestionEvidence::OrphanedHandle { handle },
            }) => assert_eq!(handle, "OQ-1"),
            other => panic!("Expected orphaned-handle evidence, got {other:?}"),
        }
    }

    #[test]
    fn suggest_s001_not_for_file_handles() {
        let mut graph = DiGraph::new();
        // File handles are roots -- never orphaned
        let _file = graph.add_node(Handle::test_file("doc.md", None));

        let diags = suggest_orphaned(&graph);
        assert!(
            diags.is_empty(),
            "S001 should not be produced for File handles (they are roots)"
        );
    }

    #[test]
    fn suggest_s001_not_for_handles_with_incoming() {
        let mut graph = DiGraph::new();
        let file = graph.add_node(Handle::test_file("doc.md", None));
        let label = graph.add_node(Handle::test_label("OQ", 1, None));
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
        let _a = graph.add_node(Handle::test_label("NEW", 1, None));
        let _b = graph.add_node(Handle::test_label("NEW", 2, None));
        let _c = graph.add_node(Handle::test_label("NEW", 3, None));

        let config = AnnealConfig::default();

        let diags = suggest_candidate_namespaces(&graph, &config);
        assert_eq!(
            diags.len(),
            1,
            "Expected 1 S002 diagnostic for candidate namespace"
        );
        assert_eq!(diags[0].severity, Severity::Suggestion);
        assert_eq!(diags[0].code, DiagnosticCode::S002);
        assert!(diags[0].message.contains("NEW"));
        match &diags[0].evidence {
            Some(Evidence::Suggestion {
                suggestion: SuggestionEvidence::CandidateNamespace { prefix, count },
            }) => {
                assert_eq!(prefix, "NEW");
                assert_eq!(*count, 3);
            }
            other => panic!("Expected candidate-namespace evidence, got {other:?}"),
        }
    }

    #[test]
    fn suggest_s002_not_for_confirmed_prefix() {
        let mut graph = DiGraph::new();
        let _a = graph.add_node(Handle::test_label("OQ", 1, None));
        let _b = graph.add_node(Handle::test_label("OQ", 2, None));
        let _c = graph.add_node(Handle::test_label("OQ", 3, None));

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
        let _a = graph.add_node(Handle::test_file("a.md", Some("draft")));
        let _b = graph.add_node(Handle::test_file("b.md", Some("draft")));
        let _c = graph.add_node(Handle::test_file("c.md", Some("draft")));
        // One handle at next level
        let _d = graph.add_node(Handle::test_file("d.md", Some("review")));

        let lattice = Lattice::test_with_ordering(&["draft", "review"], &[], &["draft", "review"]);

        let diags = suggest_pipeline_stalls(&graph, &lattice, None);
        assert_eq!(
            diags.len(),
            1,
            "Expected 1 S003 diagnostic for pipeline stall"
        );
        assert_eq!(diags[0].severity, Severity::Suggestion);
        assert_eq!(diags[0].code, DiagnosticCode::S003);
        assert!(diags[0].message.contains("draft"));
        match &diags[0].evidence {
            Some(Evidence::Suggestion {
                suggestion:
                    SuggestionEvidence::PipelineStall {
                        status,
                        count,
                        next_status,
                        based_on_history,
                    },
            }) => {
                assert_eq!(status, "draft");
                assert_eq!(*count, 3);
                assert_eq!(next_status.as_deref(), Some("review"));
                assert!(!based_on_history);
            }
            other => panic!("Expected pipeline-stall evidence, got {other:?}"),
        }
    }

    #[test]
    fn suggest_s003_empty_when_no_ordering() {
        let mut graph = DiGraph::new();
        let _a = graph.add_node(Handle::test_file("a.md", Some("draft")));

        let lattice = Lattice::test_with_ordering(&["draft"], &[], &[]);

        let diags = suggest_pipeline_stalls(&graph, &lattice, None);
        assert!(
            diags.is_empty(),
            "S003 should not be produced when ordering is empty (no pipeline)"
        );
    }

    fn make_snapshot(states: &[(&str, usize)]) -> Snapshot {
        Snapshot {
            timestamp: "2026-03-30T00:00:00Z".to_string(),
            handles: HandleCounts {
                total: states.iter().map(|(_, count)| count).sum(),
                active: states.iter().map(|(_, count)| count).sum(),
                frozen: 0,
            },
            edges: EdgeCounts { total: 0 },
            states: states
                .iter()
                .map(|(status, count)| ((*status).to_string(), *count))
                .collect(),
            obligations: ObligationCounts {
                outstanding: 0,
                discharged: 0,
                mooted: 0,
            },
            diagnostics: DiagnosticCounts {
                errors: 0,
                warnings: 0,
            },
            namespaces: HashMap::<String, NamespaceStats>::new(),
        }
    }

    #[test]
    fn suggest_s003_static_fallback_without_history() {
        let mut graph = DiGraph::new();
        let _a = graph.add_node(Handle::test_file("a.md", Some("draft")));
        let _b = graph.add_node(Handle::test_file("b.md", Some("draft")));
        let _c = graph.add_node(Handle::test_file("c.md", Some("draft")));
        let _d = graph.add_node(Handle::test_file("d.md", Some("review")));

        let lattice = Lattice::test_with_ordering(&["draft", "review"], &[], &["draft", "review"]);

        let diags = suggest_pipeline_stalls(&graph, &lattice, None);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0]
                .message
                .contains("pipeline stall: 3 handles at status 'draft'")
        );
    }

    #[test]
    fn suggest_s003_uses_temporal_signal_when_population_unchanged() {
        let mut graph = DiGraph::new();
        let _a = graph.add_node(Handle::test_file("a.md", Some("draft")));
        let _b = graph.add_node(Handle::test_file("b.md", Some("draft")));
        let _c = graph.add_node(Handle::test_file("c.md", Some("draft")));

        let lattice = Lattice::test_with_ordering(&["draft", "review"], &[], &["draft", "review"]);
        let previous = make_snapshot(&[("draft", 3)]);

        let diags = suggest_pipeline_stalls(&graph, &lattice, Some(&previous));
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0]
                .message
                .contains("unchanged from previous snapshot")
        );
    }

    #[test]
    fn suggest_s003_skips_temporal_signal_when_population_decreases() {
        let mut graph = DiGraph::new();
        let _a = graph.add_node(Handle::test_file("a.md", Some("draft")));
        let _b = graph.add_node(Handle::test_file("b.md", Some("draft")));
        let _c = graph.add_node(Handle::test_file("d.md", Some("review")));

        let lattice = Lattice::test_with_ordering(&["draft", "review"], &[], &["draft", "review"]);
        let previous = make_snapshot(&[("draft", 4)]);

        let diags = suggest_pipeline_stalls(&graph, &lattice, Some(&previous));
        assert!(diags.is_empty());
    }

    // -----------------------------------------------------------------------
    // SUGGEST-04: Abandoned namespaces
    // -----------------------------------------------------------------------

    #[test]
    fn suggest_s004_all_members_terminal() {
        let mut graph = DiGraph::new();
        let _a = graph.add_node(Handle::test_label("OLD", 1, Some("archived")));
        let _b = graph.add_node(Handle::test_label("OLD", 2, Some("archived")));

        let config = AnnealConfig {
            handles: HandlesConfig {
                confirmed: vec!["OLD".to_string()],
                ..HandlesConfig::default()
            },
            ..AnnealConfig::default()
        };
        let lattice = Lattice::test_with_ordering(&[], &["archived"], &[]);

        let diags = suggest_abandoned_namespaces(&graph, &lattice, &config);
        assert_eq!(
            diags.len(),
            1,
            "Expected 1 S004 diagnostic for abandoned namespace"
        );
        assert_eq!(diags[0].severity, Severity::Suggestion);
        assert_eq!(diags[0].code, DiagnosticCode::S004);
        assert!(diags[0].message.contains("OLD"));
        match &diags[0].evidence {
            Some(Evidence::Suggestion {
                suggestion:
                    SuggestionEvidence::AbandonedNamespace {
                        prefix,
                        member_count,
                        terminal_members,
                        stale_members,
                    },
            }) => {
                assert_eq!(prefix, "OLD");
                assert_eq!(*member_count, 2);
                assert_eq!(*terminal_members, 2);
                assert_eq!(*stale_members, 0);
            }
            other => panic!("Expected abandoned-namespace evidence, got {other:?}"),
        }
    }

    #[test]
    fn suggest_s004_all_members_stale() {
        let mut graph = DiGraph::new();
        // Create handles with old updated dates (stale beyond error threshold of 90 days)
        let mut h1 = Handle::test_label("STALE", 1, Some("draft"));
        h1.metadata.updated =
            Some(chrono::NaiveDate::from_ymd_opt(2020, 1, 1).expect("valid date"));
        let mut h2 = Handle::test_label("STALE", 2, Some("draft"));
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
        let lattice = Lattice::test_with_ordering(&["draft"], &[], &[]);

        let diags = suggest_abandoned_namespaces(&graph, &lattice, &config);
        assert_eq!(
            diags.len(),
            1,
            "Expected 1 S004 diagnostic for stale namespace (all members beyond freshness threshold)"
        );
        assert_eq!(diags[0].code, DiagnosticCode::S004);
    }

    #[test]
    fn suggest_s004_not_for_fresh_active_members() {
        let mut graph = DiGraph::new();
        // Fresh active handles -- should NOT be flagged
        let mut h1 = Handle::test_label("ACTIVE", 1, Some("draft"));
        h1.metadata.updated = Some(chrono::Local::now().date_naive());
        let mut h2 = Handle::test_label("ACTIVE", 2, Some("draft"));
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
        let lattice = Lattice::test_with_ordering(&["draft"], &[], &[]);

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
            let file = graph.add_node(Handle::test_file(&format!("doc{i}.md"), None));
            let oq = graph.add_node(Handle::test_label("OQ", i + 1, None));
            let fm = graph.add_node(Handle::test_label("FM", i + 1, None));
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
        assert_eq!(diags[0].code, DiagnosticCode::S005);
        assert!(
            diags[0].message.contains("OQ") && diags[0].message.contains("FM"),
            "S005 message should mention both co-occurring prefixes"
        );
        match &diags[0].evidence {
            Some(Evidence::Suggestion {
                suggestion:
                    SuggestionEvidence::ConcernGroupCandidate {
                        left_prefix,
                        right_prefix,
                        file_count,
                    },
            }) => {
                assert_eq!(left_prefix, "FM");
                assert_eq!(right_prefix, "OQ");
                assert_eq!(*file_count, 3);
            }
            other => panic!("Expected concern-group evidence, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // run_suggestions + run_checks integration
    // -----------------------------------------------------------------------

    #[test]
    fn suggest_run_checks_includes_suggestions() {
        let mut graph = DiGraph::new();
        // Orphaned label -> S001
        let _label = graph.add_node(Handle::test_label("LONE", 1, None));

        let lattice = Lattice::test_with_ordering(&[], &[], &[]);
        let config = AnnealConfig::default();
        let unresolved: Vec<PendingEdge> = Vec::new();

        let cascade = HashMap::new();
        let input = CheckInput {
            graph: &graph,
            lattice: &lattice,
            config: &config,
            unresolved_edges: &unresolved,
            section_ref_count: 0,
            section_ref_file: None,
            implausible_refs: &[],
            cascade_candidates: &cascade,
            previous_snapshot: None,
        };
        let diags = run_checks(&input);
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
            code: DiagnosticCode::E001,
            message: "test".to_string(),
            file: None,
            line: None,
            evidence: None,
        };
        let json = serde_json::to_value(&diag).expect("serialize");
        assert!(
            json["evidence"].is_null(),
            "evidence: None should serialize as null"
        );
        // Existing fields still present
        assert_eq!(json["code"], "E001");
        assert_eq!(json["message"], "test");
    }

    #[test]
    fn evidence_broken_ref_serializes_with_type_tag() {
        let diag = Diagnostic {
            severity: Severity::Error,
            code: DiagnosticCode::E001,
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
        assert_eq!(json["severity"], "error");
        assert_eq!(json["code"], "E001");
        assert_eq!(json["file"], "doc.md");
        assert_eq!(json["line"], 10);
    }

    #[test]
    fn evidence_suggestion_serializes_with_nested_kind() {
        let diag = Diagnostic {
            severity: Severity::Suggestion,
            code: DiagnosticCode::S002,
            message: "candidate namespace".to_string(),
            file: Some("labels.md".to_string()),
            line: None,
            evidence: Some(Evidence::Suggestion {
                suggestion: SuggestionEvidence::CandidateNamespace {
                    prefix: "NEW".to_string(),
                    count: 3,
                },
            }),
        };
        let json = serde_json::to_value(&diag).expect("serialize");
        let ev = &json["evidence"];
        assert_eq!(ev["type"], "Suggestion");
        assert_eq!(ev["kind"], "candidate_namespace");
        assert_eq!(ev["prefix"], "NEW");
        assert_eq!(ev["count"], 3);
    }

    #[test]
    fn check_existence_with_candidates_produces_evidence() {
        let mut graph = DiGraph::new();
        let source = graph.add_node(Handle::test_file("doc.md", None));

        let unresolved = vec![PendingEdge {
            source,
            target_identity: "OQ-99".to_string(),
            kind: EdgeKind::Cites,
            inverse: false,
            line: Some(5),
        }];

        let mut cascade = HashMap::new();
        cascade.insert("OQ-99".to_string(), vec!["OQ-9".to_string()]);

        let diags = check_existence(&graph, &unresolved, 0, None, &cascade);
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
        let source = graph.add_node(Handle::test_file("doc.md", None));

        let unresolved = vec![PendingEdge {
            source,
            target_identity: "MISSING-1".to_string(),
            kind: EdgeKind::Cites,
            inverse: false,
            line: None,
        }];

        let cascade = HashMap::new();
        let diags = check_existence(&graph, &unresolved, 0, None, &cascade);
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
    fn check_existence_suggests_subdirectory_file_for_bare_filename() {
        let mut graph = DiGraph::new();
        let source = graph.add_node(Handle::test_file("doc.md", None));
        let _nested = graph.add_node(Handle::test_file("notes/foo.md", None));

        let unresolved = vec![PendingEdge {
            source,
            target_identity: "foo.md".to_string(),
            kind: EdgeKind::Cites,
            inverse: false,
            line: Some(7),
        }];

        let diags = check_existence(&graph, &unresolved, 0, None, &HashMap::new());
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("did you mean notes/foo.md?"));
        match &diags[0].evidence {
            Some(Evidence::BrokenRef { target, candidates }) => {
                assert_eq!(target, "foo.md");
                assert_eq!(candidates, &["notes/foo.md"]);
            }
            other => panic!("Expected Evidence::BrokenRef, got {other:?}"),
        }
    }

    #[test]
    fn check_existence_keeps_generic_candidates_for_non_bare_target() {
        let mut graph = DiGraph::new();
        let source = graph.add_node(Handle::test_file("doc.md", None));

        let unresolved = vec![PendingEdge {
            source,
            target_identity: "formal/foo.md".to_string(),
            kind: EdgeKind::Cites,
            inverse: false,
            line: Some(9),
        }];

        let mut cascade = HashMap::new();
        cascade.insert(
            "formal/foo.md".to_string(),
            vec!["formal-model/foo.md".to_string()],
        );

        let diags = check_existence(&graph, &unresolved, 0, None, &cascade);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0]
                .message
                .contains("similar handle exists: formal-model/foo.md")
        );
    }

    #[test]
    fn run_checks_sorts_by_severity() {
        let mut graph = DiGraph::new();
        // Create a scenario producing all three severities:
        // E001 from unresolved edge
        let source = graph.add_node(Handle::test_file("doc.md", Some("draft")));
        // W001 from stale dependency (DependsOn triggers staleness check)
        let terminal = graph.add_node(Handle::test_file("old.md", Some("archived")));
        graph.add_edge(source, terminal, EdgeKind::DependsOn);

        let lattice = Lattice::test_with_ordering(&["draft"], &["archived"], &[]);
        let config = AnnealConfig::default();

        let unresolved = vec![PendingEdge {
            source,
            target_identity: "missing-ref".to_string(),
            kind: EdgeKind::Cites,
            inverse: false,
            line: None,
        }];

        let cascade = HashMap::new();
        let input = CheckInput {
            graph: &graph,
            lattice: &lattice,
            config: &config,
            unresolved_edges: &unresolved,
            section_ref_count: 5,
            section_ref_file: None,
            implausible_refs: &[],
            cascade_candidates: &cascade,
            previous_snapshot: None,
        };
        let diags = run_checks(&input);

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

    #[test]
    fn apply_suppressions_removes_global_code_matches() {
        let mut diagnostics = vec![
            Diagnostic {
                severity: Severity::Info,
                code: DiagnosticCode::I001,
                message: "section refs".to_string(),
                file: Some("doc.md".to_string()),
                line: None,
                evidence: None,
            },
            Diagnostic {
                severity: Severity::Error,
                code: DiagnosticCode::E001,
                message: "broken reference: target.md not found".to_string(),
                file: Some("doc.md".to_string()),
                line: Some(1),
                evidence: None,
            },
        ];

        apply_suppressions(
            &mut diagnostics,
            &SuppressConfig {
                codes: vec!["I001".to_string()],
                rules: Vec::new(),
            },
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::E001);
    }

    #[test]
    fn apply_suppressions_removes_targeted_rule_matches() {
        let mut diagnostics = vec![
            Diagnostic {
                severity: Severity::Error,
                code: DiagnosticCode::E001,
                message: "broken reference: synthesis/v17.md not found".to_string(),
                file: Some("anneal-spec.md".to_string()),
                line: Some(1),
                evidence: None,
            },
            Diagnostic {
                severity: Severity::Error,
                code: DiagnosticCode::E001,
                message: "broken reference: spec.md not found".to_string(),
                file: Some("anneal-spec.md".to_string()),
                line: Some(1),
                evidence: None,
            },
        ];

        apply_suppressions(
            &mut diagnostics,
            &SuppressConfig {
                codes: Vec::new(),
                rules: vec![SuppressRule {
                    code: "E001".to_string(),
                    target: "synthesis/v17.md".to_string(),
                }],
            },
        );

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("spec.md"));
    }

    #[test]
    fn apply_suppressions_keeps_non_matching_diagnostics() {
        let original = Diagnostic {
            severity: Severity::Warning,
            code: DiagnosticCode::W001,
            message: "stale dependency: doc.md depends on archived.md".to_string(),
            file: Some("doc.md".to_string()),
            line: None,
            evidence: None,
        };
        let mut diagnostics = vec![original.clone()];

        apply_suppressions(
            &mut diagnostics,
            &SuppressConfig {
                codes: vec!["E001".to_string()],
                rules: vec![SuppressRule {
                    code: "W001".to_string(),
                    target: "other.md".to_string(),
                }],
            },
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, original.code);
        assert_eq!(diagnostics[0].message, original.message);
    }
}
