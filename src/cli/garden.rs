use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;

use serde::Serialize;

use crate::area::{AreaFilter, AreaHealth, area_of};
use crate::checks::{Diagnostic, DiagnosticCode, Severity};
use crate::graph::DiGraph;
use crate::handle::HandleKind;

// ---------------------------------------------------------------------------
// Garden command — ranked maintenance task list
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GardenCategory {
    /// Correctness blockers: E001 broken refs, E002 undischarged obligations.
    Fix,
    /// Orphaned labels (S001) grouped by area.
    Tidy,
    /// Areas with zero cross-links (structural islands).
    Link,
    /// Old files with no connection to recent work.
    Stale,
    /// Missing frontmatter / status metadata (W003).
    Meta,
    /// Namespaces leaking across area boundaries.
    Drift,
}

impl GardenCategory {
    fn short(self) -> &'static str {
        match self {
            Self::Fix => "fix",
            Self::Tidy => "tidy",
            Self::Link => "link",
            Self::Stale => "stale",
            Self::Meta => "meta",
            Self::Drift => "drift",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GardenBlast {
    High,
    Med,
    Low,
}

impl GardenBlast {
    fn short(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Med => "med",
            Self::Low => "low",
        }
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct GardenHints {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) verify: Option<String>,
}

#[derive(Clone, Serialize)]
pub(crate) struct GardenTask {
    pub(crate) category: GardenCategory,
    pub(crate) title: String,
    pub(crate) detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) area: Option<String>,
    pub(crate) blast: GardenBlast,
    pub(crate) blast_score: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) handles: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) files: Vec<String>,
    pub(crate) hints: GardenHints,
}

#[derive(Serialize)]
pub(crate) struct GardenOutput {
    #[serde(rename = "_meta")]
    pub(crate) meta: super::OutputMeta,
    pub(crate) tasks: Vec<GardenTask>,
    pub(crate) total: usize,
    pub(crate) returned: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) area: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) category_filter: Option<GardenCategory>,
}

impl GardenOutput {
    pub(crate) fn print_human(&self, w: &mut dyn Write) -> std::io::Result<()> {
        if self.tasks.is_empty() {
            writeln!(w, "No maintenance tasks — corpus is tidy.")?;
            return Ok(());
        }
        for (idx, task) in self.tasks.iter().enumerate() {
            let area = task
                .area
                .as_deref()
                .map_or(String::new(), |a| format!(" in {a}/"));
            writeln!(
                w,
                "{:>2}. [{:<5}] {}{area}   blast={}",
                idx + 1,
                task.category.short(),
                task.title,
                task.blast.short(),
            )?;
            if !task.detail.is_empty() {
                writeln!(w, "             {}", task.detail)?;
            }
            if let Some(fix) = &task.hints.fix {
                writeln!(w, "             fix:     {fix}")?;
            }
            if let Some(ctx) = &task.hints.context {
                writeln!(w, "             context: {ctx}")?;
            }
            if let Some(verify) = &task.hints.verify {
                writeln!(w, "             verify:  {verify}")?;
            }
        }
        if self.returned < self.total {
            writeln!(w)?;
            writeln!(
                w,
                "Showing {} of {} tasks. Use --limit={} for more.",
                self.returned,
                self.total,
                self.total.min(self.returned * 2),
            )?;
        }
        Ok(())
    }
}

pub(crate) struct GardenOptions<'a> {
    pub(crate) graph: &'a DiGraph,
    pub(crate) diagnostics: &'a [Diagnostic],
    pub(crate) areas: &'a [AreaHealth],
    pub(crate) area_filter: Option<&'a AreaFilter>,
    pub(crate) category: Option<GardenCategory>,
    pub(crate) limit: usize,
    pub(crate) config: &'a crate::config::AnnealConfig,
}

pub(crate) fn cmd_garden(opts: &GardenOptions<'_>) -> GardenOutput {
    let mut tasks: Vec<GardenTask> = Vec::new();
    collect_fix_tasks(opts, &mut tasks);
    collect_tidy_tasks(opts, &mut tasks);
    collect_link_tasks(opts, &mut tasks);
    collect_meta_tasks(opts, &mut tasks);
    collect_stale_tasks(opts, &mut tasks);
    collect_drift_tasks(opts, &mut tasks);

    if let Some(cat) = opts.category {
        tasks.retain(|t| t.category == cat);
    }
    if let Some(af) = opts.area_filter {
        tasks.retain(|t| match t.area.as_deref() {
            Some(a) => a == af.name(),
            // Corpus-wide tasks (e.g. drift) are kept only if they reference
            // the filtered area in their detail line.
            None => t.detail.contains(af.name()),
        });
    }

    tasks.sort_by(|a, b| b.blast_score.cmp(&a.blast_score));

    let total = tasks.len();
    let returned = tasks.len().min(opts.limit);
    let truncated = returned < total;
    tasks.truncate(opts.limit);

    GardenOutput {
        meta: super::OutputMeta::new(
            if truncated {
                super::DetailLevel::Sample
            } else {
                super::DetailLevel::Full
            },
            truncated,
            Some(returned),
            Some(total),
            if truncated {
                vec![format!("--limit={}", opts.limit.saturating_mul(2))]
            } else {
                Vec::new()
            },
        ),
        tasks,
        total,
        returned,
        area: opts.area_filter.map(|a| a.name().to_string()),
        category_filter: opts.category,
    }
}

// ---------------------------------------------------------------------------
// Category collectors
// ---------------------------------------------------------------------------

fn collect_fix_tasks(opts: &GardenOptions<'_>, out: &mut Vec<GardenTask>) {
    let mut by_area_e001: HashMap<String, Vec<&Diagnostic>> = HashMap::new();
    let mut by_area_e002: HashMap<String, Vec<&Diagnostic>> = HashMap::new();

    for diag in opts.diagnostics {
        if diag.severity != Severity::Error {
            continue;
        }
        let area_name = diag.file.as_deref().map_or("(root)", area_of).to_string();
        match diag.code {
            DiagnosticCode::E001 => by_area_e001.entry(area_name).or_default().push(diag),
            DiagnosticCode::E002 => by_area_e002.entry(area_name).or_default().push(diag),
            _ => {}
        }
    }

    for (area, diags) in by_area_e001 {
        let count = diags.len();
        let files: Vec<String> = diags
            .iter()
            .filter_map(|d| d.file.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let first = diags
            .iter()
            .find_map(|d| d.message.lines().next().map(String::from))
            .unwrap_or_default();
        out.push(GardenTask {
            category: GardenCategory::Fix,
            title: format!("{count} broken ref{}", plural(count)),
            detail: first,
            area: Some(area.clone()),
            blast: GardenBlast::High,
            blast_score: 1_000_000 + (count as u64).saturating_mul(10),
            handles: Vec::new(),
            files,
            hints: GardenHints {
                fix: Some("resolve or remove the broken references listed".to_string()),
                context: Some(format!("anneal orient --area={area} --budget=20k")),
                verify: Some(format!("anneal check --area={area} --errors-only")),
            },
        });
    }

    for (area, diags) in by_area_e002 {
        let count = diags.len();
        let handles: Vec<String> = diags
            .iter()
            .filter_map(|d| extract_obligation_handle(&d.message))
            .collect();
        let first = handles.first().cloned().unwrap_or_default();
        out.push(GardenTask {
            category: GardenCategory::Fix,
            title: format!("{count} undischarged obligation{}", plural(count),),
            detail: if handles.is_empty() {
                "E002 obligations without a Discharges edge".to_string()
            } else {
                format!("e.g., {first} has no Discharges edge")
            },
            area: Some(area.clone()),
            blast: GardenBlast::High,
            blast_score: 1_000_000 + (count as u64).saturating_mul(10),
            files: Vec::new(),
            handles: handles.clone(),
            hints: GardenHints {
                fix: if let Some(h) = handles.first() {
                    Some(format!(
                        "add `discharges: [{h}]` to the resolving document frontmatter",
                    ))
                } else {
                    Some(
                        "add `discharges: [HANDLE]` to a resolving document's frontmatter"
                            .to_string(),
                    )
                },
                context: Some(format!("anneal orient --area={area} --budget=20k")),
                verify: Some(format!("anneal check --area={area} --obligations")),
            },
        });
    }
}

fn collect_tidy_tasks(opts: &GardenOptions<'_>, out: &mut Vec<GardenTask>) {
    let mut by_area: HashMap<String, Vec<String>> = HashMap::new();
    for diag in opts.diagnostics {
        if diag.code != DiagnosticCode::S001 {
            continue;
        }
        let area = diag.file.as_deref().map_or("(root)", area_of).to_string();
        if let Some(handle) = extract_orphan_handle(&diag.message) {
            by_area.entry(area).or_default().push(handle);
        }
    }

    for (area, mut handles) in by_area {
        handles.sort();
        handles.dedup();
        let count = handles.len();
        if count == 0 {
            continue;
        }
        let sample = handles
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let detail = if handles.len() > 5 {
            format!("{sample}, ... ({} more)", handles.len() - 5)
        } else {
            sample
        };
        out.push(GardenTask {
            category: GardenCategory::Tidy,
            title: format!("{count} orphaned label{}", plural(count)),
            detail,
            area: Some(area.clone()),
            blast: GardenBlast::Med,
            blast_score: 100_000 + count as u64,
            handles,
            files: Vec::new(),
            hints: GardenHints {
                fix: Some(
                    "reference these labels from relevant documents, or retire them".to_string(),
                ),
                context: Some(format!("anneal orient --area={area} --budget=20k")),
                verify: Some(format!("anneal check --area={area} --suggest")),
            },
        });
    }
}

fn collect_link_tasks(opts: &GardenOptions<'_>, out: &mut Vec<GardenTask>) {
    for area in opts.areas {
        if area.cross_links > 0 || area.files < 3 {
            continue;
        }
        out.push(GardenTask {
            category: GardenCategory::Link,
            title: format!("island: {} files, 0 cross-links", area.files),
            detail: "nothing inside this area references work elsewhere, and nothing elsewhere references it".to_string(),
            area: Some(area.name.clone()),
            blast: GardenBlast::Low,
            blast_score: 10_000 + area.files as u64,
            handles: Vec::new(),
            files: Vec::new(),
            hints: GardenHints {
                fix: Some("add `depends-on:` frontmatter or body references that connect this area to adjacent work".to_string()),
                context: Some(format!("anneal orient --area={} --budget=20k", area.name)),
                verify: Some(format!("anneal areas --area={}", area.name)),
            },
        });
    }
}

fn collect_meta_tasks(opts: &GardenOptions<'_>, out: &mut Vec<GardenTask>) {
    let mut by_area: HashMap<String, usize> = HashMap::new();
    for diag in opts.diagnostics {
        if diag.code != DiagnosticCode::W003 {
            continue;
        }
        let area = diag.file.as_deref().map_or("(root)", area_of).to_string();
        *by_area.entry(area).or_insert(0) += 1;
    }
    for (area, count) in by_area {
        out.push(GardenTask {
            category: GardenCategory::Meta,
            title: format!("{count} file{} missing frontmatter", plural(count)),
            detail: "files have no `status:` field and no inferred lifecycle".to_string(),
            area: Some(area.clone()),
            blast: GardenBlast::Low,
            blast_score: 1_000 + count as u64,
            handles: Vec::new(),
            files: Vec::new(),
            hints: GardenHints {
                fix: Some("add `status:` frontmatter to key files in this area".to_string()),
                context: Some(format!("anneal orient --area={area} --budget=20k")),
                verify: Some(format!("anneal check --area={area} --include-terminal")),
            },
        });
    }
}

fn collect_stale_tasks(opts: &GardenOptions<'_>, out: &mut Vec<GardenTask>) {
    let today = chrono::Local::now().date_naive();
    let warn_days = i64::from(opts.config.freshness.warn.max(30));

    let mut handles_by_file: HashMap<String, usize> = HashMap::new();
    for (_, h) in opts.graph.nodes() {
        if let Some(fp) = h.file_path.as_deref() {
            *handles_by_file.entry(fp.as_str().to_string()).or_insert(0) += 1;
        }
    }

    let mut stale_by_area: HashMap<String, StaleAccumulator> = HashMap::new();
    for (node, handle) in opts.graph.nodes() {
        if !matches!(handle.kind, HandleKind::File(_)) {
            continue;
        }
        let Some(date) = handle.date else { continue };
        let age_days = today.signed_duration_since(date).num_days();
        if age_days < warn_days {
            continue;
        }
        if has_recent_neighbor(opts.graph, node, today, warn_days) {
            continue;
        }
        let path = handle
            .file_path
            .as_deref()
            .map_or(String::new(), |p| p.as_str().to_string());
        if path.is_empty() {
            continue;
        }
        let area = area_of(path.as_str()).to_string();
        let handle_count = handles_by_file.get(&path).copied().unwrap_or(1);
        #[allow(clippy::cast_sign_loss)]
        let age = age_days.max(0) as u64;
        let entry = stale_by_area.entry(area).or_default();
        entry.files.push(path);
        entry.score = entry
            .score
            .saturating_add(age.saturating_mul(handle_count as u64));
        entry.max_age = entry.max_age.max(age);
        entry.count += 1;
    }

    for (area, mut acc) in stale_by_area {
        acc.files.sort();
        let sample = acc
            .files
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let detail = if acc.files.len() > 3 {
            format!("{sample}, ... (+{} more)", acc.files.len() - 3)
        } else {
            sample
        };
        out.push(GardenTask {
            category: GardenCategory::Stale,
            title: format!(
                "{} stale file{} (oldest {}d)",
                acc.count,
                plural(acc.count),
                acc.max_age,
            ),
            detail,
            area: Some(area.clone()),
            blast: GardenBlast::Low,
            blast_score: acc.score,
            handles: Vec::new(),
            files: acc.files,
            hints: GardenHints {
                fix: Some(
                    "link these to recent work, retire them, or move them to an archive/ directory"
                        .to_string(),
                ),
                context: Some(format!("anneal orient --area={area} --budget=20k")),
                verify: Some(format!("anneal areas --area={area}")),
            },
        });
    }
}

#[derive(Default)]
struct StaleAccumulator {
    files: Vec<String>,
    score: u64,
    max_age: u64,
    count: usize,
}

fn has_recent_neighbor(
    graph: &DiGraph,
    node: crate::handle::NodeId,
    today: chrono::NaiveDate,
    warn_days: i64,
) -> bool {
    let is_recent = |n| -> bool {
        graph
            .node(n)
            .date
            .is_some_and(|d| today.signed_duration_since(d).num_days() < warn_days)
    };
    graph.outgoing(node).iter().any(|e| is_recent(e.target))
        || graph.incoming(node).iter().any(|e| is_recent(e.source))
}

fn collect_drift_tasks(opts: &GardenOptions<'_>, out: &mut Vec<GardenTask>) {
    let confirmed: HashSet<&str> = opts
        .config
        .handles
        .confirmed
        .iter()
        .map(String::as_str)
        .collect();

    let mut prefix_areas: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for (_, handle) in opts.graph.nodes() {
        if let HandleKind::Label { prefix, .. } = &handle.kind
            && confirmed.contains(prefix.as_str())
            && let Some(fp) = handle.file_path.as_deref()
        {
            prefix_areas
                .entry(prefix.clone())
                .or_default()
                .insert(area_of(fp.as_str()).to_string());
        }
    }

    for (prefix, areas) in prefix_areas {
        if areas.len() < 2 {
            continue;
        }
        let mut area_list: Vec<String> = areas.into_iter().collect();
        area_list.sort();
        let span = area_list.len();
        out.push(GardenTask {
            category: GardenCategory::Drift,
            title: format!("{prefix} namespace spans {span} areas"),
            detail: format!("defined across {}", area_list.join(", ")),
            area: None,
            blast: GardenBlast::Low,
            blast_score: 500 + span as u64,
            handles: Vec::new(),
            files: Vec::new(),
            hints: GardenHints {
                fix: Some(format!(
                    "consolidate {prefix}-* labels to one area, or add a concern group"
                )),
                context: Some(format!("anneal find {prefix} --kind=label")),
                verify: None,
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Message parsing helpers (best-effort — we avoid coupling to exact formats)
// ---------------------------------------------------------------------------

fn extract_orphan_handle(msg: &str) -> Option<String> {
    // S001 messages look like: "orphaned handle OQ-64 ..."
    msg.split_whitespace()
        .find(|tok| tok.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && tok.contains('-'))
        .map(|s| {
            s.trim_end_matches(|c: char| !c.is_alphanumeric())
                .to_string()
        })
}

fn extract_obligation_handle(msg: &str) -> Option<String> {
    // E002 messages look like: "undischarged obligation COMP-OQ-1 ..."
    msg.split_whitespace()
        .find(|tok| tok.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && tok.contains('-'))
        .map(|s| {
            s.trim_end_matches(|c: char| !c.is_alphanumeric())
                .to_string()
        })
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::area::compute_areas;
    use crate::config::{AnnealConfig, AreasConfig};
    use crate::graph::{DiGraph, EdgeKind};
    use crate::handle::Handle;
    use crate::lattice::Lattice;

    fn diag(code: DiagnosticCode, file: &str, msg: &str) -> Diagnostic {
        Diagnostic {
            severity: match code {
                DiagnosticCode::E001 | DiagnosticCode::E002 => Severity::Error,
                DiagnosticCode::W001 | DiagnosticCode::W002 | DiagnosticCode::W003 => {
                    Severity::Warning
                }
                DiagnosticCode::S001
                | DiagnosticCode::S002
                | DiagnosticCode::S003
                | DiagnosticCode::S004
                | DiagnosticCode::S005 => Severity::Suggestion,
                _ => Severity::Info,
            },
            code,
            message: msg.to_string(),
            file: Some(file.to_string()),
            line: None,
            evidence: None,
        }
    }

    fn base_opts<'a>(
        graph: &'a DiGraph,
        diagnostics: &'a [Diagnostic],
        areas: &'a [AreaHealth],
        config: &'a AnnealConfig,
    ) -> GardenOptions<'a> {
        GardenOptions {
            graph,
            diagnostics,
            areas,
            area_filter: None,
            category: None,
            limit: 50,
            config,
        }
    }

    #[test]
    fn garden_ranks_errors_first() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("compiler/a.md", Some("draft")));
        graph.add_node(Handle::test_file("notes/b.md", Some("draft")));
        let diagnostics = vec![
            diag(DiagnosticCode::S001, "notes/b.md", "orphaned handle OQ-1"),
            diag(DiagnosticCode::E001, "compiler/a.md", "broken ref spec.md"),
        ];
        let lattice = Lattice::test_empty();
        let areas = compute_areas(&graph, &lattice, &diagnostics, &AreasConfig::default());
        let config = AnnealConfig::default();

        let output = cmd_garden(&base_opts(&graph, &diagnostics, &areas, &config));
        assert!(!output.tasks.is_empty());
        assert_eq!(output.tasks[0].category, GardenCategory::Fix);
    }

    #[test]
    fn garden_filters_by_category() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("compiler/a.md", Some("draft")));
        let diagnostics = vec![
            diag(DiagnosticCode::E001, "compiler/a.md", "broken ref x"),
            diag(
                DiagnosticCode::S001,
                "compiler/a.md",
                "orphaned handle OQ-1",
            ),
        ];
        let lattice = Lattice::test_empty();
        let areas = compute_areas(&graph, &lattice, &diagnostics, &AreasConfig::default());
        let config = AnnealConfig::default();

        let mut opts = base_opts(&graph, &diagnostics, &areas, &config);
        opts.category = Some(GardenCategory::Tidy);
        let output = cmd_garden(&opts);
        assert!(
            output
                .tasks
                .iter()
                .all(|t| t.category == GardenCategory::Tidy)
        );
    }

    #[test]
    fn garden_fix_includes_frontmatter_hint_for_obligation() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("compiler/a.md", Some("draft")));
        let diagnostics = vec![diag(
            DiagnosticCode::E002,
            "compiler/a.md",
            "undischarged obligation COMP-OQ-1",
        )];
        let lattice = Lattice::test_empty();
        let areas = compute_areas(&graph, &lattice, &diagnostics, &AreasConfig::default());
        let config = AnnealConfig::default();

        let output = cmd_garden(&base_opts(&graph, &diagnostics, &areas, &config));
        let fix = output.tasks[0].hints.fix.as_deref().unwrap_or("");
        assert!(
            fix.contains("discharges:"),
            "expected frontmatter hint: {fix}"
        );
        assert!(fix.contains("COMP-OQ-1"), "expected handle in fix: {fix}");
    }

    #[test]
    fn garden_detects_island() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("archive/a.md", Some("draft")));
        graph.add_node(Handle::test_file("archive/b.md", Some("draft")));
        graph.add_node(Handle::test_file("archive/c.md", Some("draft")));
        graph.add_node(Handle::test_file("archive/d.md", Some("draft")));
        // connected area to make archive stand out
        let x = graph.add_node(Handle::test_file("compiler/x.md", Some("draft")));
        let y = graph.add_node(Handle::test_file("compiler/y.md", Some("draft")));
        graph.add_edge(x, y, EdgeKind::DependsOn);

        let lattice = Lattice::test_empty();
        let areas = compute_areas(&graph, &lattice, &[], &AreasConfig::default());
        let config = AnnealConfig::default();
        let output = cmd_garden(&base_opts(&graph, &[], &areas, &config));
        assert!(output
            .tasks
            .iter()
            .any(|t| t.category == GardenCategory::Link && t.area.as_deref() == Some("archive")));
    }

    #[test]
    fn garden_limit_truncates() {
        let mut graph = DiGraph::new();
        graph.add_node(Handle::test_file("a/x.md", Some("draft")));
        graph.add_node(Handle::test_file("b/x.md", Some("draft")));
        let diagnostics = vec![
            diag(DiagnosticCode::S001, "a/x.md", "orphaned OQ-1"),
            diag(DiagnosticCode::S001, "b/x.md", "orphaned FM-1"),
        ];
        let lattice = Lattice::test_empty();
        let areas = compute_areas(&graph, &lattice, &diagnostics, &AreasConfig::default());
        let config = AnnealConfig::default();

        let mut opts = base_opts(&graph, &diagnostics, &areas, &config);
        opts.limit = 1;
        let output = cmd_garden(&opts);
        assert_eq!(output.returned, 1);
        assert_eq!(output.total, 2);
        assert!(output.meta.truncated);
    }
}
