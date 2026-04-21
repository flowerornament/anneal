use std::collections::{HashMap, HashSet};
use std::io::Write;

use globset::GlobSet;
use serde::Serialize;

use crate::area::{AreaFilter, AreaHealth};
use crate::config::OrientConfig;
use crate::graph::DiGraph;
use crate::handle::{Handle, HandleKind, NodeId};
use crate::output::{Line, Printer, Render, Tone, Toned};

use super::map::{TraversalDirection, around_subgraph};
use super::{DetailLevel, OutputMeta, SnippetIndex, lookup_handle, truncate_snippet};

// ---------------------------------------------------------------------------
// Orient command
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OrientTier {
    Pinned,
    EntryPoint,
    Upstream,
    Downstream,
}

impl OrientTier {
    /// Heading + dim caption for each tier. The caption carries the
    /// ranking rationale without competing with the tier title.
    fn section(self) -> (&'static str, &'static str) {
        match self {
            Self::Pinned => ("Read first", "pinned files"),
            Self::EntryPoint => (
                "Read next",
                "area entry points, ranked by centrality × recency",
            ),
            Self::Upstream => ("Upstream context", "files these read"),
            Self::Downstream => ("Downstream consumers", "files that read this area"),
        }
    }
}

impl std::fmt::Display for OrientTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Pinned => "pinned",
            Self::EntryPoint => "entry points",
            Self::Upstream => "upstream context",
            Self::Downstream => "downstream consumers",
        })
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct OrientEntry {
    pub(crate) path: String,
    pub(crate) tier: OrientTier,
    pub(crate) score: f64,
    pub(crate) tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) purpose: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) date: Option<chrono::NaiveDate>,
}

#[derive(Serialize)]
pub(crate) struct OrientBudget {
    pub(crate) limit: u32,
    pub(crate) used: u32,
    pub(crate) dropped_tiers: Vec<OrientTier>,
}

#[derive(Serialize)]
pub(crate) struct AreaSummary {
    pub(crate) name: String,
    pub(crate) grade: crate::area::AreaGrade,
    pub(crate) files: usize,
    pub(crate) handles: usize,
    pub(crate) connectivity: f64,
    pub(crate) signal: String,
    pub(crate) errors: usize,
    pub(crate) orphans: usize,
}

#[derive(Serialize)]
pub(crate) struct OrientOutput {
    #[serde(rename = "_meta")]
    pub(crate) meta: OutputMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) area: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) scope_file: Option<String>,
    pub(crate) entries: Vec<OrientEntry>,
    pub(crate) budget: OrientBudget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) area_summary: Option<AreaSummary>,
}

impl Render for OrientOutput {
    fn render<W: Write>(&self, p: &mut Printer<W>) -> std::io::Result<()> {
        if let Some(sum) = &self.area_summary {
            p.line(
                &Line::new()
                    .heading(format!("{}/", sum.name))
                    .text("  ")
                    .toned(sum.grade.tone(), format!("[{}]", sum.grade))
                    .text("  ")
                    .count(sum.files)
                    .text(" files, ")
                    .count(sum.handles)
                    .text(" handles, conn ")
                    .float(sum.connectivity, 1),
            )?;
            p.blank()?;
        } else if let Some(path) = &self.scope_file {
            p.line(&Line::new().heading("Reading list for ").path(path.clone()))?;
            p.blank()?;
        }

        // Column width for the path column in each tier's rows.
        let path_col = self.path_column_width();

        let mut first_section = true;
        for tier in [
            OrientTier::Pinned,
            OrientTier::EntryPoint,
            OrientTier::Upstream,
            OrientTier::Downstream,
        ] {
            let in_tier: Vec<&OrientEntry> =
                self.entries.iter().filter(|e| e.tier == tier).collect();
            if in_tier.is_empty() {
                continue;
            }
            if !first_section {
                p.blank()?;
            }
            first_section = false;
            let (title, caption) = tier.section();
            p.heading(title, Some(in_tier.len()))?;
            p.caption(caption)?;
            for e in in_tier {
                let tokens_str = format_tokens(e.tokens);
                let path_width = console::measure_text_width(&e.path);
                let pad = path_col.saturating_sub(path_width) + 2;
                let row = Line::new()
                    .path(e.path.clone())
                    .pad(pad)
                    .toned(Tone::Number, tokens_str);
                p.line(&row)?;
                if let Some(purpose) = e.purpose.as_deref()
                    && !purpose.is_empty()
                {
                    p.line_at(6, &Line::new().dim(truncate_snippet(purpose).into_owned()))?;
                }
            }
        }

        p.blank()?;
        // Budget line. `N / M tokens used` with dropped tiers callout if any.
        let budget_line = Line::new()
            .heading("Budget ")
            .toned(Tone::Number, format_tokens(self.budget.used))
            .dim(" / ")
            .toned(Tone::Number, format_tokens(self.budget.limit))
            .dim(" tokens used");
        p.line(&budget_line)?;
        if !self.budget.dropped_tiers.is_empty() {
            let dropped: Vec<String> = self
                .budget
                .dropped_tiers
                .iter()
                .map(ToString::to_string)
                .collect();
            p.line_at(4, &Line::new().dim("dropped: ").warning(dropped.join(", ")))?;
        }

        if let Some(sum) = &self.area_summary
            && (sum.errors > 0 || sum.orphans > 0)
        {
            p.blank()?;
            p.heading("Active issues", None)?;
            if sum.errors > 0 {
                p.line_at(
                    4,
                    &Line::new()
                        .count(sum.errors)
                        .text(" errors in ")
                        .path(format!("{}/", sum.name)),
                )?;
            }
            if sum.orphans > 0 {
                p.line_at(4, &Line::new().count(sum.orphans).text(" orphaned labels"))?;
            }
        }

        Ok(())
    }
}

impl OrientOutput {
    /// Widest file path across all entries (for column alignment). Capped at
    /// 64 to keep the token column visible on narrow terminals.
    fn path_column_width(&self) -> usize {
        let max = self
            .entries
            .iter()
            .map(|e| console::measure_text_width(&e.path))
            .max()
            .unwrap_or(0);
        max.min(64)
    }

    pub(crate) fn print_paths_only(&self, w: &mut dyn Write) -> std::io::Result<()> {
        for e in &self.entries {
            writeln!(w, "{}", e.path)?;
        }
        Ok(())
    }
}

fn format_tokens(tokens: u32) -> String {
    if tokens >= 1000 {
        format!("{}k", tokens / 1000)
    } else {
        tokens.to_string()
    }
}

/// Parse a budget string like `"50k"`, `"100k"`, or `"5000"` into a token count.
pub(crate) fn parse_budget(s: &str) -> anyhow::Result<u32> {
    let s = s.trim().to_lowercase();
    if let Some(stripped) = s.strip_suffix('k') {
        stripped
            .trim()
            .parse::<u32>()
            .map(|n| n.saturating_mul(1000))
            .map_err(|_| anyhow::anyhow!("invalid budget '{s}': expected format like '50k'"))
    } else if let Some(stripped) = s.strip_suffix('m') {
        stripped
            .trim()
            .parse::<u32>()
            .map(|n| n.saturating_mul(1_000_000))
            .map_err(|_| anyhow::anyhow!("invalid budget '{s}': expected format like '1m'"))
    } else {
        s.parse::<u32>()
            .map_err(|_| anyhow::anyhow!("invalid budget '{s}': expected '50k' or '5000'"))
    }
}

pub(crate) struct OrientOptions<'a> {
    pub(crate) graph: &'a DiGraph,
    pub(crate) node_index: &'a HashMap<String, NodeId>,
    pub(crate) config: &'a OrientConfig,
    pub(crate) area: Option<&'a AreaFilter>,
    pub(crate) file: Option<&'a str>,
    pub(crate) budget_tokens: u32,
    pub(crate) snippets: SnippetIndex<'a>,
    pub(crate) area_health: Option<&'a AreaHealth>,
}

/// Compute the orient reading list.
pub(crate) fn cmd_orient(opts: &OrientOptions<'_>) -> anyhow::Result<OrientOutput> {
    let graph = opts.graph;
    let exclude = ExcludeMatcher::new(&opts.config.exclude);

    let file_entries = collect_file_entries(graph, &exclude);

    let candidate_set: HashSet<NodeId> = match opts.file {
        Some(path) => {
            let start = lookup_handle(opts.node_index, path)
                .ok_or_else(|| anyhow::anyhow!("handle not found: {path}"))?;
            around_subgraph(
                graph,
                start,
                opts.config.depth.max(1),
                TraversalDirection::Upstream,
                None,
            )
        }
        None => match opts.area {
            Some(af) => file_entries
                .iter()
                .filter(|fe| af.matches_handle(graph.node(fe.node)))
                .map(|fe| fe.node)
                .collect(),
            None => file_entries.iter().map(|fe| fe.node).collect(),
        },
    };

    // Score every file once. `tier_scope` is the subset in the candidate set;
    // `all_scored` is the full map used for boundary (upstream/downstream) tiers
    // so out-of-scope files carry their full-graph rank.
    let all_scored = score_files(graph, &file_entries, opts.config);
    let tier_scope: HashMap<NodeId, &ScoredFile> = all_scored
        .iter()
        .filter(|(node, _)| candidate_set.contains(node))
        .map(|(node, score)| (*node, score))
        .collect();

    let pinned_entries = collect_pinned(graph, opts.node_index, opts.config, &exclude);
    let pinned_ids: HashSet<NodeId> = pinned_entries.iter().map(|e| e.node).collect();

    let mut entry_candidates: Vec<&ScoredFile> = tier_scope
        .values()
        .copied()
        .filter(|s| !pinned_ids.contains(&s.node))
        .collect();
    entry_candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let (upstream_candidates, downstream_candidates) = if opts.file.is_some() {
        (Vec::new(), Vec::new())
    } else if let Some(af) = opts.area {
        let upstream = boundary_files(
            graph,
            &all_scored,
            &candidate_set,
            &pinned_ids,
            af,
            &BoundaryDirection::Upstream,
        );
        let downstream = boundary_files(
            graph,
            &all_scored,
            &candidate_set,
            &pinned_ids,
            af,
            &BoundaryDirection::Downstream,
        );
        (upstream, downstream)
    } else {
        (Vec::new(), Vec::new())
    };

    let mut entries: Vec<OrientEntry> = Vec::new();
    let mut used_tokens: u32 = 0;
    let mut dropped_tiers: Vec<OrientTier> = Vec::new();

    add_tier(
        &mut entries,
        &mut used_tokens,
        &mut dropped_tiers,
        OrientTier::Pinned,
        pinned_entries.iter().collect::<Vec<_>>(),
        opts,
        graph,
    );
    add_tier(
        &mut entries,
        &mut used_tokens,
        &mut dropped_tiers,
        OrientTier::EntryPoint,
        entry_candidates,
        opts,
        graph,
    );
    add_tier(
        &mut entries,
        &mut used_tokens,
        &mut dropped_tiers,
        OrientTier::Upstream,
        upstream_candidates.iter().collect::<Vec<_>>(),
        opts,
        graph,
    );
    add_tier(
        &mut entries,
        &mut used_tokens,
        &mut dropped_tiers,
        OrientTier::Downstream,
        downstream_candidates.iter().collect::<Vec<_>>(),
        opts,
        graph,
    );

    let area_summary = opts.area_health.map(|h| AreaSummary {
        name: h.name.clone(),
        grade: h.grade,
        files: h.files,
        handles: h.handles,
        connectivity: h.connectivity,
        signal: h.signal.clone(),
        errors: h.errors,
        orphans: h.orphans,
    });

    Ok(OrientOutput {
        meta: OutputMeta::new(DetailLevel::Full, false, None, None, Vec::new()),
        area: opts.area.map(|a| a.name().to_string()),
        scope_file: opts.file.map(str::to_string),
        entries,
        budget: OrientBudget {
            limit: opts.budget_tokens,
            used: used_tokens,
            dropped_tiers,
        },
        area_summary,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Matcher honoring the shared `exclude` grammar from `anneal.toml`.
/// Plain names exclude whole top-level areas; glob patterns match full paths.
/// Mirrors the semantics of the parser's graph-walker so users get one rule
/// that means the same thing everywhere.
struct ExcludeMatcher<'a> {
    dir_names: Vec<&'a str>,
    glob_set: Option<GlobSet>,
}

impl<'a> ExcludeMatcher<'a> {
    fn new(patterns: &'a [String]) -> Self {
        let (dir_names, glob_set) = crate::parse::build_exclude_sets(patterns);
        Self {
            dir_names,
            glob_set,
        }
    }

    fn is_excluded(&self, path: &str) -> bool {
        let first = path.split('/').next().unwrap_or(path);
        if self.dir_names.contains(&first) {
            return true;
        }
        self.glob_set.as_ref().is_some_and(|gs| gs.is_match(path))
    }
}

struct FileEntry {
    node: NodeId,
    path: String,
    date_ord: Option<i64>,
}

fn collect_file_entries(graph: &DiGraph, exclude: &ExcludeMatcher<'_>) -> Vec<FileEntry> {
    graph
        .nodes()
        .filter_map(|(node, handle)| {
            let path = match &handle.kind {
                HandleKind::File(p) => p.as_str().to_string(),
                _ => return None,
            };
            if exclude.is_excluded(&path) {
                return None;
            }
            Some(FileEntry {
                node,
                path,
                date_ord: handle
                    .date
                    .map(|d| d.signed_duration_since(EPOCH).num_days()),
            })
        })
        .collect()
}

struct ScoredFile {
    node: NodeId,
    score: f64,
}

const EPOCH: chrono::NaiveDate = match chrono::NaiveDate::from_ymd_opt(1970, 1, 1) {
    Some(d) => d,
    None => unreachable!(),
};

fn score_files(
    graph: &DiGraph,
    all_files: &[FileEntry],
    config: &OrientConfig,
) -> HashMap<NodeId, ScoredFile> {
    score_files_at(graph, all_files, config, chrono::Local::now().date_naive())
}

/// Score files relative to an anchor date. Separated from `score_files` so
/// tests can pin a reproducible "today" without clock flake.
fn score_files_at(
    graph: &DiGraph,
    all_files: &[FileEntry],
    config: &OrientConfig,
    today: chrono::NaiveDate,
) -> HashMap<NodeId, ScoredFile> {
    let today_ord = (today - EPOCH).num_days();
    // Guard against a zero/unset half-life: treat as one day so the decay
    // is sharp but defined, rather than dividing by zero.
    let half_life = f64::from(config.recency_half_life_days.max(1));

    let mut label_counts: HashMap<&str, usize> = HashMap::new();
    for (_, h) in graph.nodes() {
        if let (HandleKind::Label { .. }, Some(fp)) = (&h.kind, h.file_path.as_deref()) {
            *label_counts.entry(fp.as_str()).or_insert(0) += 1;
        }
    }

    all_files
        .iter()
        .map(|fe| {
            let handle = graph.node(fe.node);
            // Incoming edges (others → this file) are the real centrality
            // signal for an orientation target: "people cite this." Outgoing
            // edges say "this file consumes a lot", which is weaker evidence
            // that it's a good reading entry point (e.g. long reference
            // tables). Weight incoming twice as heavily.
            //
            // Log-scaling keeps hubs from monopolizing the top. A file with
            // 100 citations isn't 10× more useful to read first than one
            // with 10 — diminishing returns kick in fast. This lets
            // recency, status, and other signals actually influence order
            // in mature corpora where a few files have accumulated
            // hundreds of back-references.
            #[allow(clippy::cast_precision_loss)]
            let incoming_log = (graph.incoming(fe.node).len() as f64 + 1.0).ln();
            #[allow(clippy::cast_precision_loss)]
            let outgoing_log = (graph.outgoing(fe.node).len() as f64 + 1.0).ln();
            let edge_score = (incoming_log * 2.0 + outgoing_log) * config.edge_weight;

            let label_count = label_counts.get(fe.path.as_str()).copied().unwrap_or(0);
            #[allow(clippy::cast_precision_loss)]
            let label_score = (label_count as f64 + 1.0).ln() * config.label_weight;

            let recency = fe.date_ord.map_or(0.0, |d| {
                // Files dated in the future pin to bonus=1.0 rather than
                // overshooting; ancient files decay toward zero.
                #[allow(clippy::cast_precision_loss)]
                let age_days = (today_ord - d).max(0) as f64;
                let bonus = 0.5_f64.powf(age_days / half_life);
                bonus * config.recency_weight
            });
            let status_bonus = status_bonus(handle);
            let score = edge_score + label_score + recency + status_bonus;
            (
                fe.node,
                ScoredFile {
                    node: fe.node,
                    score,
                },
            )
        })
        .collect()
}

fn status_bonus(handle: &Handle) -> f64 {
    match handle.status.as_deref() {
        Some("active" | "draft" | "stable" | "current" | "open" | "proposed") => 2.0,
        Some(_) => 0.3,
        None => 0.5,
    }
}

enum BoundaryDirection {
    Upstream,
    Downstream,
}

fn boundary_files(
    graph: &DiGraph,
    all_scored: &HashMap<NodeId, ScoredFile>,
    in_area: &HashSet<NodeId>,
    pinned: &HashSet<NodeId>,
    area: &AreaFilter,
    direction: &BoundaryDirection,
) -> Vec<ScoredFile> {
    let mut outside: HashSet<NodeId> = HashSet::new();
    for &anchor in in_area {
        match direction {
            BoundaryDirection::Upstream => {
                for edge in graph.outgoing(anchor) {
                    if !area.matches_handle(graph.node(edge.target))
                        && matches!(graph.node(edge.target).kind, HandleKind::File(_))
                    {
                        outside.insert(edge.target);
                    }
                }
            }
            BoundaryDirection::Downstream => {
                for edge in graph.incoming(anchor) {
                    if !area.matches_handle(graph.node(edge.source))
                        && matches!(graph.node(edge.source).kind, HandleKind::File(_))
                    {
                        outside.insert(edge.source);
                    }
                }
            }
        }
    }

    let mut scored: Vec<ScoredFile> = outside
        .into_iter()
        .filter(|nid| !pinned.contains(nid))
        .filter_map(|nid| {
            all_scored.get(&nid).map(|s| ScoredFile {
                node: s.node,
                score: s.score,
            })
        })
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored
}

fn collect_pinned(
    graph: &DiGraph,
    node_index: &HashMap<String, NodeId>,
    config: &OrientConfig,
    exclude: &ExcludeMatcher<'_>,
) -> Vec<ScoredFile> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for pin in &config.pin {
        let Some(node) = lookup_handle(node_index, pin) else {
            continue;
        };
        if !seen.insert(node) {
            continue;
        }
        let handle = graph.node(node);
        let path = match handle.file_path.as_deref() {
            Some(p) => p.as_str(),
            None => continue,
        };
        if exclude.is_excluded(path) {
            continue;
        }
        out.push(ScoredFile {
            node,
            score: f64::INFINITY,
        });
    }
    out
}

fn add_tier(
    entries: &mut Vec<OrientEntry>,
    used: &mut u32,
    dropped: &mut Vec<OrientTier>,
    tier: OrientTier,
    candidates: Vec<&ScoredFile>,
    opts: &OrientOptions<'_>,
    graph: &DiGraph,
) {
    if candidates.is_empty() {
        return;
    }
    let mut included = 0usize;
    let mut seen: HashSet<NodeId> = entries
        .iter()
        .filter_map(|e| opts.node_index.get(&e.path).copied())
        .collect();

    for c in candidates {
        if !seen.insert(c.node) {
            continue;
        }
        let handle = graph.node(c.node);
        let tokens = tokens_for(handle);
        if used.saturating_add(tokens) > opts.budget_tokens {
            continue;
        }
        let path = handle
            .file_path
            .as_deref()
            .map_or_else(|| handle.id.clone(), |p| p.as_str().to_string());
        entries.push(OrientEntry {
            path,
            tier,
            score: c.score,
            tokens,
            status: handle.status.clone(),
            purpose: opts.snippets.summary_for(handle).map(str::to_string),
            date: handle.date,
        });
        *used = used.saturating_add(tokens);
        included += 1;
    }

    if included == 0 {
        dropped.push(tier);
    }
}

fn tokens_for(handle: &Handle) -> u32 {
    handle.size_bytes.unwrap_or(4000) / 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::test_helpers::test_node_index;
    use crate::graph::EdgeKind;
    use crate::handle::HandleMetadata;

    fn file_with_size(path: &str, status: Option<&str>, size: u32) -> Handle {
        let mut h = Handle::file(
            camino::Utf8PathBuf::from(path),
            status.map(String::from),
            None,
            Some(size),
            HandleMetadata::default(),
        );
        h.size_bytes = Some(size);
        h
    }

    fn empty_snippets() -> (HashMap<String, String>, HashMap<String, String>) {
        (HashMap::new(), HashMap::new())
    }

    #[test]
    fn parse_budget_accepts_k_suffix() {
        assert_eq!(parse_budget("50k").unwrap(), 50_000);
        assert_eq!(parse_budget("100K").unwrap(), 100_000);
    }

    #[test]
    fn parse_budget_accepts_bare_number() {
        assert_eq!(parse_budget("5000").unwrap(), 5000);
    }

    #[test]
    fn parse_budget_rejects_garbage() {
        assert!(parse_budget("abc").is_err());
        assert!(parse_budget("xyzk").is_err());
    }

    fn file_with_date(path: &str, status: Option<&str>, date: chrono::NaiveDate) -> Handle {
        Handle::file(
            camino::Utf8PathBuf::from(path),
            status.map(String::from),
            Some(date),
            Some(4000),
            HandleMetadata::default(),
        )
    }

    fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn recency_decays_exponentially_with_age() {
        // Two otherwise identical files differing only in date.
        let mut graph = DiGraph::new();
        let fresh_id = graph.add_node(file_with_date("fresh.md", Some("draft"), date(2026, 4, 17)));
        let old_id = graph.add_node(file_with_date("old.md", Some("draft"), date(2024, 4, 17)));

        let config = OrientConfig::default();
        let files = vec![
            FileEntry {
                node: fresh_id,
                path: "fresh.md".to_string(),
                date_ord: Some((date(2026, 4, 17) - EPOCH).num_days()),
            },
            FileEntry {
                node: old_id,
                path: "old.md".to_string(),
                date_ord: Some((date(2024, 4, 17) - EPOCH).num_days()),
            },
        ];

        let scores = score_files_at(&graph, &files, &config, date(2026, 4, 17));
        let fresh_score = scores.get(&fresh_id).unwrap().score;
        let old_score = scores.get(&old_id).unwrap().score;

        // Fresh = today: full recency weight (5.0) added.
        // Old = ~730 days = 8.1 half-lives out at half_life=90, so ~5.0 * 0.0035.
        // With default status_bonus 2.0 for both and no edges/labels,
        // fresh ≈ 2.0 + 5.0 = 7.0, old ≈ 2.0 + 0.018 ≈ 2.02.
        assert!(
            (fresh_score - old_score) > 4.0,
            "fresh ({fresh_score}) should outscore old ({old_score}) by ≈5 under default config",
        );
    }

    #[test]
    fn recency_respects_half_life_config() {
        let mut graph = DiGraph::new();
        let day_old = graph.add_node(file_with_date(
            "day-old.md",
            Some("draft"),
            date(2026, 4, 16),
        ));
        let year_old = graph.add_node(file_with_date(
            "year-old.md",
            Some("draft"),
            date(2025, 4, 17),
        ));

        let files = vec![
            FileEntry {
                node: day_old,
                path: "day-old.md".to_string(),
                date_ord: Some((date(2026, 4, 16) - EPOCH).num_days()),
            },
            FileEntry {
                node: year_old,
                path: "year-old.md".to_string(),
                date_ord: Some((date(2025, 4, 17) - EPOCH).num_days()),
            },
        ];

        // Short half-life: a year-old file is almost fully decayed.
        let short = OrientConfig {
            recency_half_life_days: 30,
            ..OrientConfig::default()
        };
        let short_scores = score_files_at(&graph, &files, &short, date(2026, 4, 17));
        let short_gap =
            short_scores.get(&day_old).unwrap().score - short_scores.get(&year_old).unwrap().score;

        // Long half-life: a year-old file is still half-ish there, gap narrows.
        let long = OrientConfig {
            recency_half_life_days: 730,
            ..OrientConfig::default()
        };
        let long_scores = score_files_at(&graph, &files, &long, date(2026, 4, 17));
        let long_gap =
            long_scores.get(&day_old).unwrap().score - long_scores.get(&year_old).unwrap().score;

        assert!(
            short_gap > long_gap,
            "shorter half-life should widen the recent-vs-stale gap \
             (short={short_gap:.3}, long={long_gap:.3})",
        );
    }

    #[test]
    fn recency_zero_for_undated_files() {
        let mut graph = DiGraph::new();
        let undated = graph.add_node(Handle::file(
            camino::Utf8PathBuf::from("undated.md"),
            Some("draft".to_string()),
            None,
            Some(4000),
            HandleMetadata::default(),
        ));

        let files = vec![FileEntry {
            node: undated,
            path: "undated.md".to_string(),
            date_ord: None,
        }];
        let config = OrientConfig::default();
        let scores = score_files_at(&graph, &files, &config, date(2026, 4, 17));
        // Status bonus only: 2.0 (draft), no edges, no labels, no recency.
        assert!((scores.get(&undated).unwrap().score - 2.0).abs() < 0.001);
    }

    #[test]
    fn orient_respects_budget() {
        let mut graph = DiGraph::new();
        let big = graph.add_node(file_with_size("compiler/big.md", Some("active"), 40_000));
        let med = graph.add_node(file_with_size("compiler/med.md", Some("active"), 8_000));
        let _ = big;
        let _ = med;

        let node_index = test_node_index(&graph);
        let config = OrientConfig::default();
        let (files, labels) = empty_snippets();
        let snippets = SnippetIndex {
            files: &files,
            labels: &labels,
        };

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            area: None,
            file: None,
            budget_tokens: 5_000,
            snippets,
            area_health: None,
        })
        .expect("orient output");

        assert!(output.budget.used <= 5_000);
        for e in &output.entries {
            assert!(e.tokens <= 5_000);
        }
    }

    #[test]
    fn orient_pinned_appears_first() {
        let mut graph = DiGraph::new();
        graph.add_node(file_with_size("OPEN-QUESTIONS.md", Some("active"), 2_000));
        graph.add_node(file_with_size("compiler/a.md", Some("active"), 2_000));
        graph.add_node(file_with_size("compiler/b.md", Some("active"), 2_000));

        let node_index = test_node_index(&graph);
        let config = OrientConfig {
            pin: vec!["OPEN-QUESTIONS.md".to_string()],
            ..OrientConfig::default()
        };
        let (files, labels) = empty_snippets();
        let snippets = SnippetIndex {
            files: &files,
            labels: &labels,
        };

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            area: None,
            file: None,
            budget_tokens: 50_000,
            snippets,
            area_health: None,
        })
        .expect("orient output");

        assert_eq!(output.entries[0].tier, OrientTier::Pinned);
        assert_eq!(output.entries[0].path, "OPEN-QUESTIONS.md");
    }

    #[test]
    fn orient_area_scope_excludes_other_areas() {
        let mut graph = DiGraph::new();
        graph.add_node(file_with_size("compiler/a.md", Some("active"), 2_000));
        graph.add_node(file_with_size("synthesis/b.md", Some("active"), 2_000));

        let node_index = test_node_index(&graph);
        let config = OrientConfig::default();
        let (files, labels) = empty_snippets();
        let snippets = SnippetIndex {
            files: &files,
            labels: &labels,
        };
        let area = AreaFilter::new("compiler");

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            area: Some(&area),
            file: None,
            budget_tokens: 50_000,
            snippets,
            area_health: None,
        })
        .expect("orient output");

        let entry_paths: Vec<&str> = output
            .entries
            .iter()
            .filter(|e| e.tier == OrientTier::EntryPoint)
            .map(|e| e.path.as_str())
            .collect();
        assert!(entry_paths.contains(&"compiler/a.md"));
        assert!(!entry_paths.contains(&"synthesis/b.md"));
    }

    #[test]
    fn orient_file_mode_walks_upstream() {
        let mut graph = DiGraph::new();
        let target = graph.add_node(file_with_size("x.md", Some("active"), 2_000));
        let upstream = graph.add_node(file_with_size("y.md", Some("active"), 2_000));
        let unrelated = graph.add_node(file_with_size("z.md", Some("active"), 2_000));
        graph.add_edge(target, upstream, EdgeKind::DependsOn);
        let _ = unrelated;

        let node_index = test_node_index(&graph);
        let config = OrientConfig::default();
        let (files, labels) = empty_snippets();
        let snippets = SnippetIndex {
            files: &files,
            labels: &labels,
        };

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            area: None,
            file: Some("x.md"),
            budget_tokens: 50_000,
            snippets,
            area_health: None,
        })
        .expect("orient output");

        let paths: Vec<&str> = output.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"x.md"));
        assert!(paths.contains(&"y.md"));
        assert!(!paths.contains(&"z.md"));
    }

    #[test]
    fn orient_paths_only_writer() {
        let mut graph = DiGraph::new();
        graph.add_node(file_with_size("a.md", Some("active"), 2_000));

        let node_index = test_node_index(&graph);
        let config = OrientConfig::default();
        let (files, labels) = empty_snippets();
        let snippets = SnippetIndex {
            files: &files,
            labels: &labels,
        };

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            area: None,
            file: None,
            budget_tokens: 50_000,
            snippets,
            area_health: None,
        })
        .expect("orient output");

        let mut buf = Vec::new();
        output.print_paths_only(&mut buf).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("a.md"));
    }

    #[test]
    fn orient_exclude_treats_plain_names_as_dir_exclusions() {
        let mut graph = DiGraph::new();
        graph.add_node(file_with_size("compiler/a.md", Some("active"), 2_000));
        graph.add_node(file_with_size("archive/old.md", Some("active"), 2_000));
        graph.add_node(file_with_size("CHANGELOG.md", Some("active"), 2_000));

        let node_index = test_node_index(&graph);
        // `archive` is a plain directory name; `**/CHANGELOG.md` is a glob.
        // Before the shared-exclude fix, plain names were treated as full-path
        // globs and silently matched nothing.
        let config = OrientConfig {
            exclude: vec!["archive".to_string(), "**/CHANGELOG.md".to_string()],
            ..OrientConfig::default()
        };
        let (files, labels) = empty_snippets();
        let snippets = SnippetIndex {
            files: &files,
            labels: &labels,
        };

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            area: None,
            file: None,
            budget_tokens: 50_000,
            snippets,
            area_health: None,
        })
        .expect("orient output");

        let paths: Vec<String> = output.entries.iter().map(|e| e.path.clone()).collect();
        assert!(paths.iter().any(|p| p == "compiler/a.md"));
        assert!(
            !paths.iter().any(|p| p == "archive/old.md"),
            "archive/ directory should be excluded by plain name"
        );
        assert!(
            !paths.iter().any(|p| p == "CHANGELOG.md"),
            "CHANGELOG.md should be excluded by glob"
        );
    }
}
