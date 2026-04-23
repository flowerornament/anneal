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
    Frontier,
    Foundation,
    Upstream,
    Downstream,
}

impl OrientTier {
    /// Heading + dim caption for each tier. The caption carries the
    /// ranking rationale without competing with the tier title.
    fn section(self) -> (&'static str, &'static str) {
        match self {
            Self::Pinned => ("Pinned", "always-included context"),
            Self::Frontier => ("Frontier", "where work is now"),
            Self::Foundation => ("Foundation", "stable hubs the frontier still cites"),
            Self::Upstream => ("Upstream", "dependencies outside this area"),
            Self::Downstream => ("Downstream", "consumers outside this area"),
        }
    }
}

impl std::fmt::Display for OrientTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Pinned => "pinned",
            Self::Frontier => "frontier",
            Self::Foundation => "foundation",
            Self::Upstream => "upstream",
            Self::Downstream => "downstream",
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
    /// True when this entry was too large for the remaining budget.
    /// Overflow entries render as `path  Nk` only (no snippet). Agents
    /// see them as hints to re-run with a wider `--budget`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) overflow: bool,
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
            OrientTier::Frontier,
            OrientTier::Foundation,
            OrientTier::Upstream,
            OrientTier::Downstream,
        ] {
            let in_tier: Vec<&OrientEntry> =
                self.entries.iter().filter(|e| e.tier == tier).collect();
            if in_tier.is_empty() {
                continue;
            }
            let (fits, overflow): (Vec<&OrientEntry>, Vec<&OrientEntry>) =
                in_tier.iter().partition(|e| !e.overflow);
            if !first_section {
                p.blank()?;
            }
            first_section = false;
            let (title, caption) = tier.section();
            p.heading(title, Some(fits.len()))?;
            p.caption(caption)?;
            for e in fits {
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
            if !overflow.is_empty() {
                p.blank()?;
                p.line_at(
                    4,
                    &Line::new()
                        .dim("Overflow ")
                        .dim(format!("({})", overflow.len()))
                        .dim("  too large for budget; re-run with wider --budget"),
                )?;
                for e in overflow {
                    let tokens_str = format_tokens(e.tokens);
                    let path_width = console::measure_text_width(&e.path);
                    let pad = path_col.saturating_sub(path_width) + 2;
                    let row = Line::new()
                        .dim(e.path.clone())
                        .pad(pad)
                        .toned(Tone::Number, tokens_str);
                    p.line_at(4, &row)?;
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
    pub(crate) lattice: &'a crate::lattice::Lattice,
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

    let file_entries = collect_file_entries(graph, &exclude, opts.lattice, opts.config);

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
    let all_scored = score_files(graph, &file_entries, opts.lattice, opts.config);
    let tier_scope: HashMap<NodeId, &ScoredFile> = all_scored
        .iter()
        .filter(|(node, _)| candidate_set.contains(node))
        .map(|(node, score)| (*node, score))
        .collect();

    let pinned_entries = collect_pinned(graph, opts.node_index, opts.config, &exclude);
    let pinned_ids: HashSet<NodeId> = pinned_entries.iter().map(|e| e.node).collect();

    // Frontier candidates: files with active-like status, partitioned
    // by area (top-level directory or `--area`). In global mode we take
    // the newest active file per area; in area-scoped mode we take
    // top-K by date within the scope.
    let frontier_ids = collect_frontier(graph, &file_entries, &candidate_set, &pinned_ids, opts);
    // Preserve the date-descending order from collect_frontier — for
    // Frontier, "newest first" reads better than "score-first." A
    // resuming agent wants yesterday's landing at the top.
    let frontier_candidates: Vec<&ScoredFile> = frontier_ids
        .iter()
        .filter_map(|nid| tier_scope.get(nid).copied())
        .collect();
    let frontier_set: HashSet<NodeId> = frontier_ids.iter().copied().collect();

    // Foundation candidates: everything in tier_scope that isn't pinned
    // or in the frontier. Ranked by score (curated-hub bonus + recency-
    // weighted in-degree). Curated hubs from the top-level surface in
    // area mode too, so a newcomer touching compiler/ still sees the
    // project README.
    let mut foundation_candidates: Vec<&ScoredFile> = tier_scope
        .values()
        .copied()
        .filter(|s| !pinned_ids.contains(&s.node))
        .filter(|s| !frontier_set.contains(&s.node))
        .collect();
    if opts.area.is_some() {
        // In area mode, also pull top-level curated hubs even if they
        // live outside the area's scope.
        for (nid, s) in &all_scored {
            if candidate_set.contains(nid) {
                continue; // already in tier_scope
            }
            if pinned_ids.contains(nid) || frontier_set.contains(nid) {
                continue;
            }
            if is_curated_hub(graph.node(*nid)) {
                foundation_candidates.push(s);
            }
        }
    }
    foundation_candidates.sort_by(|a, b| {
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
        OrientTier::Frontier,
        frontier_candidates,
        opts,
        graph,
    );
    add_tier(
        &mut entries,
        &mut used_tokens,
        &mut dropped_tiers,
        OrientTier::Foundation,
        foundation_candidates,
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
    /// Pre-computed curated-hub flag. Called three times per file
    /// during scoring + tier assignment; compute once here and reuse.
    is_curated: bool,
}

fn collect_file_entries(
    graph: &DiGraph,
    exclude: &ExcludeMatcher<'_>,
    lattice: &crate::lattice::Lattice,
    config: &OrientConfig,
) -> Vec<FileEntry> {
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
            if !passes_hard_filters(handle, lattice, config) {
                return None;
            }
            Some(FileEntry {
                node,
                path,
                date_ord: handle
                    .date
                    .map(|d| d.signed_duration_since(EPOCH).num_days()),
                is_curated: is_curated_hub(handle),
            })
        })
        .collect()
}

/// Basenames (case-insensitive, stripped of extension) that carry an
/// "orient me" contract by maintainer convention. Files matching these
/// names are curated hubs regardless of status or frontmatter.
///
/// Kept conservative on purpose. `MANIFEST` and `TOC` are intentionally
/// omitted: directory manifests tend to be redirect stubs more often
/// than orientation material. Corpora that use those names as real
/// entry points can promote the file explicitly with `status: living`
/// or a `purpose:` frontmatter line.
pub(super) const CURATED_HUB_BASENAMES: &[&str] = &[
    "readme",
    "changelog",
    "design-goals",
    "open-questions",
    "labels",
    "index",
    "roadmap",
    "overview",
    "glossary",
];

/// Frontmatter `purpose:` substrings (case-insensitive) that flag a file
/// as a curated hub. The maintainer declared orientation intent in
/// words — honor it.
pub(super) const CURATED_HUB_PURPOSE_CUES: &[&str] = &[
    "entry point",
    "read first",
    "read this first",
    "orientation",
    "overview",
    "map",
    "starting point",
    "guide",
];

/// True if a file is a curated orientation hub — via basename,
/// `status: living`, or `purpose:` frontmatter.
///
/// Basename is the primary signal because agents forget to annotate.
/// Zero-annotation corpora still get README/CHANGELOG/DESIGN-GOALS
/// detection; corpora that bother with frontmatter get finer control.
pub(super) fn is_curated_hub(handle: &Handle) -> bool {
    if handle.status.as_deref() == Some("living") {
        return true;
    }
    if let Some(file_path) = handle.file_path.as_deref()
        && let Some(stem) = file_path.file_stem()
    {
        let lower = stem.to_ascii_lowercase();
        if CURATED_HUB_BASENAMES.iter().any(|b| *b == lower) {
            return true;
        }
    }
    if let Some(purpose) = &handle.metadata.purpose {
        let lower = purpose.to_ascii_lowercase();
        if CURATED_HUB_PURPOSE_CUES
            .iter()
            .any(|cue| lower.contains(cue))
        {
            return true;
        }
    }
    false
}

/// True if the file should be in orient output at all (before any
/// tier/scoring logic). Replaces the 0.9.3-unreleased `content_factor`
/// soft penalty: filters beat demotions because demoted stubs still
/// consume budget at the tail end.
///
/// Excludes: terminal status (per the corpus lattice), `superseded-by:`
/// pointer, undersized non-curated files, files living under an
/// archive-style directory.
fn passes_hard_filters(
    handle: &Handle,
    lattice: &crate::lattice::Lattice,
    config: &OrientConfig,
) -> bool {
    if handle.is_terminal(lattice) {
        return false;
    }
    if handle.metadata.superseded_by.is_some() {
        return false;
    }
    if let Some(file_path) = handle.file_path.as_deref()
        && is_archive_area(file_path.as_str())
    {
        return false;
    }
    let is_curated = is_curated_hub(handle);
    if !is_curated
        && handle
            .size_bytes
            .is_some_and(|size| size < config.stub_bytes)
    {
        return false;
    }
    true
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
    lattice: &crate::lattice::Lattice,
    config: &OrientConfig,
) -> HashMap<NodeId, ScoredFile> {
    score_files_at(
        graph,
        all_files,
        lattice,
        config,
        chrono::Local::now().date_naive(),
    )
}

/// Score files relative to an anchor date. Separated from `score_files` so
/// tests can pin a reproducible "today" without clock flake.
fn score_files_at(
    graph: &DiGraph,
    all_files: &[FileEntry],
    lattice: &crate::lattice::Lattice,
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
            // Recency-weighted in-degree: each incoming citation counted
            // by the *citer's* recency, not the cited file's. An old hub
            // cited only by pre-frontier material decays; an old hub
            // still cited by this month's work keeps weight. This is the
            // core fix for stale-hub leakage — previously a March file
            // with 50 March citers outranked a March file with 10 April
            // citers, because raw in-degree ignored when the citer was
            // written.
            let edge_score = recency_weighted_in_degree(graph, fe.node, today_ord, half_life)
                * config.edge_weight;

            let label_count = label_counts.get(fe.path.as_str()).copied().unwrap_or(0);
            #[allow(clippy::cast_precision_loss)]
            let label_score = (label_count as f64 + 1.0).ln() * config.label_weight;

            let recency = fe.date_ord.map_or(0.0, |d| {
                // Files dated in the future pin to bonus=1.0 rather than
                // overshooting; ancient files decay toward zero.
                let bonus = recency_decay(today_ord, d, half_life);
                bonus * config.recency_weight
            });
            let status_bonus = status_bonus(handle, lattice);
            let curated_bonus = if fe.is_curated {
                config.curated_hub_weight
            } else {
                0.0
            };
            let score = edge_score + label_score + recency + status_bonus + curated_bonus;
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

/// Additive score bump for handles with a status the corpus declares
/// active. Uses the lattice (tool-wide canon) rather than a hardcoded
/// token list — corpora that declare `wip` as active score it the same
/// as `draft`, same as every other surface in anneal.
fn status_bonus(handle: &Handle, lattice: &crate::lattice::Lattice) -> f64 {
    match handle.status.as_deref() {
        Some(s) if lattice.active.contains(s) => 2.0,
        Some(_) => 0.3,
        None => 0.5,
    }
}

/// Exponential recency decay: 1.0 at today, halves every `half_life`
/// days, asymptotic to 0 for ancient files. Future-dated files pin at
/// 1.0 rather than overshooting.
fn recency_decay(today_ord: i64, date_ord: i64, half_life: f64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let age_days = (today_ord - date_ord).max(0) as f64;
    0.5_f64.powf(age_days / half_life)
}

/// Sum of incoming citations, each weighted by the citer's recency.
/// A file cited by many old papers scores less than a file cited by
/// fewer recent papers — the foundation we want is "what the current
/// frontier builds on," not "what everyone ever built on."
fn recency_weighted_in_degree(
    graph: &DiGraph,
    node: NodeId,
    today_ord: i64,
    half_life: f64,
) -> f64 {
    graph
        .incoming(node)
        .iter()
        .map(|edge| {
            let source = graph.node(edge.source);
            source.date.map_or(0.5, |d| {
                let date_ord = (d - EPOCH).num_days();
                recency_decay(today_ord, date_ord, half_life)
            })
        })
        .sum()
}

/// Top-level directory names that indicate historical storage — files
/// under these are never Frontier candidates regardless of status.
/// Protects against `status: active` accidentally surviving a file's
/// move into archive.
const ARCHIVE_AREAS: &[&str] = &["archive", "archives", "archived", "old", "legacy"];

fn is_archive_area(path: &str) -> bool {
    let first = path.split('/').next().unwrap_or("");
    ARCHIVE_AREAS.contains(&first.to_ascii_lowercase().as_str())
}

/// Per-area frontier picks: where is current work happening?
///
/// Rules:
/// - `--area=X` set: files in the area with active-like status, ordered
///   newest first. Return all of them; budget cap does the rest.
/// - `--area` not set, subdirectories present: partition candidate
///   files by top-level directory, take the single newest active file
///   per area.
/// - `--file=X` set (upstream walk): no frontier concept — an upstream
///   walk is already scoped and doesn't benefit from per-area slicing.
///   Return empty so Foundation owns the whole walk.
/// - Flat corpus: partition collapses to one area; top-1 picks the
///   globally-newest active file.
fn collect_frontier(
    graph: &DiGraph,
    file_entries: &[FileEntry],
    candidate_set: &HashSet<NodeId>,
    pinned: &HashSet<NodeId>,
    opts: &OrientOptions<'_>,
) -> Vec<NodeId> {
    if opts.file.is_some() {
        return Vec::new();
    }

    // Frontier-eligible = "under active authorship," which is finer than
    // the lattice's "not terminal." We drive this from the corpus's own
    // pipeline declaration when one exists: if `lattice.ordering` lists the
    // stages a handle flows through, a Frontier-eligible file has a status
    // IN that pipeline (so it's mid-flight, not off-pipeline reference
    // material like `status: stable` or `status: reference`). Corpora
    // without an ordering fall back to "any non-terminal status" — the
    // lattice has no finer signal to offer.
    let is_active = |nid: NodeId| -> bool {
        let handle = graph.node(nid);
        let Some(status) = handle.status.as_deref() else {
            return false;
        };
        if handle.is_terminal(opts.lattice) {
            return false;
        }
        if opts.lattice.ordering.is_empty() {
            return true;
        }
        opts.lattice.ordering.iter().any(|s| s == status)
    };

    // Frontier excludes curated hubs (they belong in Foundation as
    // reference material) and archive-style paths (historical storage
    // regardless of individual file status).
    let active_candidates: Vec<&FileEntry> = file_entries
        .iter()
        .filter(|fe| candidate_set.contains(&fe.node))
        .filter(|fe| !pinned.contains(&fe.node))
        .filter(|fe| is_active(fe.node))
        .filter(|fe| !fe.is_curated)
        .filter(|fe| !is_archive_area(&fe.path))
        .collect();

    if opts.area.is_some() {
        // Area-scoped: return all active files in the area, newest
        // first. Budget cap controls how many fit.
        let mut picks: Vec<&FileEntry> = active_candidates;
        picks.sort_by(|a, b| b.date_ord.cmp(&a.date_ord));
        return picks.into_iter().map(|fe| fe.node).collect();
    }

    // Global mode: partition by top-level directory; pick newest active
    // per area. Flat corpora (no subdirectories) fall back to top-K
    // globally since there's nothing meaningful to partition.
    fn area_of(path: &str) -> &str {
        if path.contains('/') {
            path.split('/').next().unwrap_or("")
        } else {
            "" // flat: everyone shares one "no-subdir" area
        }
    }

    let mut by_area: HashMap<&str, &FileEntry> = HashMap::new();
    for fe in &active_candidates {
        let area = area_of(&fe.path);
        match by_area.get(area) {
            Some(current) if current.date_ord >= fe.date_ord => {}
            _ => {
                by_area.insert(area, fe);
            }
        }
    }

    if by_area.len() == 1 && by_area.contains_key("") {
        // Flat corpus: no meaningful per-area partition. Return top-K
        // by date globally so a newcomer sees the current frontier
        // instead of one lonely pick.
        const FLAT_FRONTIER_LIMIT: usize = 5;
        let mut picks: Vec<&FileEntry> = active_candidates;
        picks.sort_by(|a, b| b.date_ord.cmp(&a.date_ord));
        picks.truncate(FLAT_FRONTIER_LIMIT);
        return picks.into_iter().map(|fe| fe.node).collect();
    }

    let mut picks: Vec<&FileEntry> = by_area.into_values().collect();
    picks.sort_by(|a, b| b.date_ord.cmp(&a.date_ord));
    picks.into_iter().map(|fe| fe.node).collect()
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

/// Maximum overflow rows to emit per tier. Overflow surfaces the
/// highest-ranked candidates that didn't fit the budget; beyond this
/// count the list is mostly noise.
const OVERFLOW_DISPLAY_LIMIT: usize = 5;

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
    let mut overflow_shown = 0usize;
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
        let overflow = used.saturating_add(tokens) > opts.budget_tokens;
        if overflow && overflow_shown >= OVERFLOW_DISPLAY_LIMIT {
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
            overflow,
        });
        if overflow {
            overflow_shown += 1;
        } else {
            // Full entries consume budget; overflow entries are metadata-
            // only (path + size) and don't.
            *used = used.saturating_add(tokens);
            included += 1;
        }
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

    /// Test lattice covering the statuses orient tests use. Mirrors the
    /// common heuristic set so terminal/active classification matches what
    /// the real lattice would produce.
    fn test_lattice() -> crate::lattice::Lattice {
        // Terminal set drawn directly from the tool-wide heuristic so
        // the two lists can't drift — adding a new terminal token to
        // lattice.rs automatically covers orient's tests.
        crate::lattice::Lattice::test_new(
            &[
                "active",
                "draft",
                "current",
                "in-progress",
                "plan",
                "complete",
                "open",
                "proposed",
            ],
            crate::lattice::TERMINAL_STATUS_HEURISTICS,
        )
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
                is_curated: false,
            },
            FileEntry {
                node: old_id,
                path: "old.md".to_string(),
                date_ord: Some((date(2024, 4, 17) - EPOCH).num_days()),
                is_curated: false,
            },
        ];

        let lattice = test_lattice();
        let scores = score_files_at(&graph, &files, &lattice, &config, date(2026, 4, 17));
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
                is_curated: false,
            },
            FileEntry {
                node: year_old,
                path: "year-old.md".to_string(),
                date_ord: Some((date(2025, 4, 17) - EPOCH).num_days()),
                is_curated: false,
            },
        ];

        // Short half-life: a year-old file is almost fully decayed.
        let short = OrientConfig {
            recency_half_life_days: 30,
            ..OrientConfig::default()
        };
        let lattice = test_lattice();
        let short_scores = score_files_at(&graph, &files, &lattice, &short, date(2026, 4, 17));
        let short_gap =
            short_scores.get(&day_old).unwrap().score - short_scores.get(&year_old).unwrap().score;

        // Long half-life: a year-old file is still half-ish there, gap narrows.
        let long = OrientConfig {
            recency_half_life_days: 730,
            ..OrientConfig::default()
        };
        let long_scores = score_files_at(&graph, &files, &lattice, &long, date(2026, 4, 17));
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
            is_curated: false,
        }];
        let config = OrientConfig::default();
        let lattice = test_lattice();
        let scores = score_files_at(&graph, &files, &lattice, &config, date(2026, 4, 17));
        // Status bonus only: 2.0 (draft), no edges, no labels, no recency.
        assert!((scores.get(&undated).unwrap().score - 2.0).abs() < 0.001);
    }

    // --- Hard filters + curated-hub detection ---------------------------

    #[test]
    fn curated_hub_detects_readme_basename_case_insensitive() {
        let readme = Handle::file(
            camino::Utf8PathBuf::from("docs/README.md"),
            Some("draft".to_string()),
            None,
            Some(2000),
            HandleMetadata::default(),
        );
        assert!(is_curated_hub(&readme));

        let readme_lower = Handle::file(
            camino::Utf8PathBuf::from("docs/readme.md"),
            None,
            None,
            Some(2000),
            HandleMetadata::default(),
        );
        assert!(is_curated_hub(&readme_lower));
    }

    #[test]
    fn curated_hub_detects_status_living() {
        let living = Handle::file(
            camino::Utf8PathBuf::from("docs/principles.md"),
            Some("living".to_string()),
            None,
            Some(2000),
            HandleMetadata::default(),
        );
        assert!(is_curated_hub(&living));
    }

    #[test]
    fn curated_hub_detects_purpose_frontmatter() {
        let meta = HandleMetadata {
            purpose: Some("Entry point. Read this first.".to_string()),
            ..HandleMetadata::default()
        };
        let hub = Handle::file(
            camino::Utf8PathBuf::from("synthesis/corpus-map.md"),
            Some("draft".to_string()),
            None,
            Some(2000),
            meta,
        );
        assert!(is_curated_hub(&hub));
    }

    #[test]
    fn curated_hub_rejects_non_hub_names() {
        let ordinary = Handle::file(
            camino::Utf8PathBuf::from("spec/readme-migration.md"),
            None,
            None,
            Some(2000),
            HandleMetadata::default(),
        );
        assert!(!is_curated_hub(&ordinary));
    }

    #[test]
    fn hard_filter_excludes_terminal_status() {
        let config = OrientConfig::default();
        // Any status the lattice marks terminal is filtered — tool-wide
        // vocabulary, not an orient-private list. Sample the heuristic
        // tokens the lattice recognizes out of the box.
        let terminal_samples = [
            "superseded",
            "archived",
            "historical",
            "prior",
            "incorporated",
            "digested",
            "deprecated",
            "obsolete",
        ];
        let lattice = crate::lattice::Lattice::test_new(&["active", "draft"], &terminal_samples);
        for status in terminal_samples {
            let handle = Handle::file(
                camino::Utf8PathBuf::from("compiler/old.md"),
                Some(status.to_string()),
                None,
                Some(8000),
                HandleMetadata::default(),
            );
            assert!(
                !passes_hard_filters(&handle, &lattice, &config),
                "status {status} should be filtered out"
            );
        }
    }

    #[test]
    fn hard_filter_excludes_superseded_by_pointer() {
        let meta = HandleMetadata {
            superseded_by: Some("compiler/new.md".to_string()),
            ..HandleMetadata::default()
        };
        let handle = Handle::file(
            camino::Utf8PathBuf::from("compiler/old.md"),
            Some("active".to_string()),
            None,
            Some(8000),
            meta,
        );
        let config = OrientConfig::default();
        let lattice = crate::lattice::Lattice::test_new(&["active"], &["superseded"]);
        assert!(!passes_hard_filters(&handle, &lattice, &config));
    }

    #[test]
    fn hard_filter_excludes_stub_sized_files_unless_curated() {
        let config = OrientConfig::default();
        let lattice = crate::lattice::Lattice::test_new(&["active"], &["superseded"]);
        let stub = Handle::file(
            camino::Utf8PathBuf::from("spec/redirect.md"),
            Some("active".to_string()),
            None,
            Some(500),
            HandleMetadata::default(),
        );
        assert!(!passes_hard_filters(&stub, &lattice, &config));

        let small_hub = Handle::file(
            camino::Utf8PathBuf::from("README.md"),
            None,
            None,
            Some(500),
            HandleMetadata::default(),
        );
        assert!(passes_hard_filters(&small_hub, &lattice, &config));
    }

    #[test]
    fn frontier_picks_newest_active_per_area() {
        let mut graph = DiGraph::new();
        // compiler/ area: two active files, different dates.
        graph.add_node(file_with_date(
            "compiler/a.md",
            Some("active"),
            date(2026, 3, 1),
        ));
        graph.add_node(file_with_date(
            "compiler/b.md",
            Some("active"),
            date(2026, 4, 20),
        ));
        // formal-model/ area: one active, one archived.
        graph.add_node(file_with_date(
            "formal-model/model.md",
            Some("active"),
            date(2026, 4, 10),
        ));
        graph.add_node(file_with_date(
            "formal-model/old.md",
            Some("superseded"),
            date(2026, 3, 15),
        ));

        let node_index = test_node_index(&graph);
        let config = OrientConfig::default();
        let (files, labels) = empty_snippets();
        let snippets = SnippetIndex {
            files: &files,
            labels: &labels,
        };
        let lattice = test_lattice();

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            lattice: &lattice,
            area: None,
            file: None,
            budget_tokens: 50_000,
            snippets,
            area_health: None,
        })
        .expect("orient output");

        let frontier_paths: HashSet<String> = output
            .entries
            .iter()
            .filter(|e| e.tier == OrientTier::Frontier)
            .map(|e| e.path.clone())
            .collect();

        assert!(frontier_paths.contains("compiler/b.md"));
        assert!(!frontier_paths.contains("compiler/a.md"));
        assert!(frontier_paths.contains("formal-model/model.md"));
        assert!(!frontier_paths.contains("formal-model/old.md"));
    }

    #[test]
    fn frontier_excludes_curated_hubs_and_archive_areas() {
        let mut graph = DiGraph::new();
        graph.add_node(file_with_date(
            "README.md",
            Some("active"),
            date(2026, 4, 20),
        ));
        graph.add_node(file_with_date(
            "archive/recent.md",
            Some("active"),
            date(2026, 4, 21),
        ));
        graph.add_node(file_with_date(
            "compiler/active.md",
            Some("active"),
            date(2026, 4, 15),
        ));

        let node_index = test_node_index(&graph);
        let config = OrientConfig::default();
        let (files, labels) = empty_snippets();
        let snippets = SnippetIndex {
            files: &files,
            labels: &labels,
        };
        let lattice = test_lattice();

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            lattice: &lattice,
            area: None,
            file: None,
            budget_tokens: 50_000,
            snippets,
            area_health: None,
        })
        .expect("orient output");

        let frontier_paths: HashSet<String> = output
            .entries
            .iter()
            .filter(|e| e.tier == OrientTier::Frontier)
            .map(|e| e.path.clone())
            .collect();
        assert!(
            !frontier_paths.contains("README.md"),
            "README.md is curated — belongs in Foundation, not Frontier"
        );
        assert!(
            !frontier_paths.contains("archive/recent.md"),
            "archive/* is historical storage — never Frontier"
        );
        assert!(frontier_paths.contains("compiler/active.md"));
    }

    #[test]
    fn frontier_pipeline_ordering_excludes_off_pipeline_status() {
        // Simulates murail: `stable` and `reference` are declared active
        // (not terminal) but are NOT in the pipeline ordering. Under the
        // pipeline-driven Frontier rule, only statuses inside the ordering
        // count as "under active authorship."
        let mut graph = DiGraph::new();
        graph.add_node(file_with_date(
            "compiler/in-pipeline.md",
            Some("draft"),
            date(2026, 4, 20),
        ));
        // `stable` is active in the lattice but outside the pipeline; in a
        // corpus like murail this should not surface as Frontier.
        graph.add_node(file_with_date(
            "runtime/off-pipeline.md",
            Some("stable"),
            date(2026, 4, 21),
        ));

        let node_index = test_node_index(&graph);
        let config = OrientConfig::default();
        let (files, labels) = empty_snippets();
        let snippets = SnippetIndex {
            files: &files,
            labels: &labels,
        };
        // Ordering lists only pipeline stages. `stable` is in `active`
        // (non-terminal) but absent from `ordering`.
        let lattice = crate::lattice::Lattice {
            observed_statuses: ["draft", "stable"]
                .iter()
                .copied()
                .map(String::from)
                .collect(),
            active: ["draft", "stable"]
                .iter()
                .copied()
                .map(String::from)
                .collect(),
            terminal: std::collections::HashSet::new(),
            ordering: ["raw", "draft", "active", "current"]
                .iter()
                .copied()
                .map(String::from)
                .collect(),
            kind: crate::lattice::LatticeKind::Confidence,
        };

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            lattice: &lattice,
            area: None,
            file: None,
            budget_tokens: 50_000,
            snippets,
            area_health: None,
        })
        .expect("orient output");

        let frontier_paths: std::collections::HashSet<String> = output
            .entries
            .iter()
            .filter(|e| e.tier == OrientTier::Frontier)
            .map(|e| e.path.clone())
            .collect();
        assert!(
            frontier_paths.contains("compiler/in-pipeline.md"),
            "in-pipeline draft should be Frontier"
        );
        assert!(
            !frontier_paths.contains("runtime/off-pipeline.md"),
            "off-pipeline `stable` should NOT be Frontier (corpus declared a pipeline)"
        );
    }

    #[test]
    fn recency_weighted_in_degree_demotes_stale_hub() {
        // Hub A: 3 citers, all ancient. Hub B: 1 citer, recent. Under
        // raw in-degree A wins 3-1; under recency-weighted in-degree
        // B wins because its single citation carries full weight and
        // A's three carry decayed weight.
        let mut graph = DiGraph::new();
        let today = date(2026, 4, 22);
        let ancient = date(2025, 10, 22); // 6 months ago, ~2 half-lives
        let recent = date(2026, 4, 15);

        let hub_a = graph.add_node(file_with_size("hub-a.md", Some("draft"), 10_000));
        let hub_b = graph.add_node(file_with_size("hub-b.md", Some("draft"), 10_000));

        for i in 1..=3 {
            let citer = graph.add_node(file_with_date(
                &format!("old-{i}.md"),
                Some("active"),
                ancient,
            ));
            graph.add_edge(citer, hub_a, EdgeKind::DependsOn);
        }
        let fresh_citer = graph.add_node(file_with_date("fresh.md", Some("active"), recent));
        graph.add_edge(fresh_citer, hub_b, EdgeKind::DependsOn);

        let files = vec![
            FileEntry {
                node: hub_a,
                path: "hub-a.md".to_string(),
                date_ord: None,
                is_curated: false,
            },
            FileEntry {
                node: hub_b,
                path: "hub-b.md".to_string(),
                date_ord: None,
                is_curated: false,
            },
        ];
        let config = OrientConfig::default();
        let lattice = test_lattice();
        let scores = score_files_at(&graph, &files, &lattice, &config, today);
        let a_score = scores.get(&hub_a).expect("a").score;
        let b_score = scores.get(&hub_b).expect("b").score;
        assert!(
            b_score > a_score,
            "fresh-citation hub (b={b_score:.3}) should outrank ancient-citation hub (a={a_score:.3})"
        );
    }

    #[test]
    fn hard_filter_keeps_missing_size_files() {
        // Handles without size_bytes (non-file handles would never reach
        // here, but safety). We can't penalize what we can't measure.
        let handle = Handle::file(
            camino::Utf8PathBuf::from("spec/a.md"),
            Some("active".to_string()),
            None,
            None,
            HandleMetadata::default(),
        );
        let config = OrientConfig::default();
        let lattice = crate::lattice::Lattice::test_new(&["active"], &["superseded"]);
        assert!(passes_hard_filters(&handle, &lattice, &config));
    }

    #[test]
    fn orient_respects_budget() {
        let mut graph = DiGraph::new();
        graph.add_node(file_with_size("compiler/big.md", Some("active"), 40_000));
        graph.add_node(file_with_size("compiler/med.md", Some("active"), 8_000));

        let node_index = test_node_index(&graph);
        let config = OrientConfig::default();
        let (files, labels) = empty_snippets();
        let snippets = SnippetIndex {
            files: &files,
            labels: &labels,
        };
        let lattice = test_lattice();

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            lattice: &lattice,
            area: None,
            file: None,
            budget_tokens: 5_000,
            snippets,
            area_health: None,
        })
        .expect("orient output");

        assert!(output.budget.used <= 5_000);
        for e in &output.entries {
            if e.overflow {
                continue; // overflow entries carry path + size only; budget doesn't apply
            }
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
        let lattice = test_lattice();

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            lattice: &lattice,
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
        let lattice = test_lattice();

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            lattice: &lattice,
            area: Some(&area),
            file: None,
            budget_tokens: 50_000,
            snippets,
            area_health: None,
        })
        .expect("orient output");

        let area_scoped: Vec<&str> = output
            .entries
            .iter()
            .filter(|e| matches!(e.tier, OrientTier::Frontier | OrientTier::Foundation))
            .map(|e| e.path.as_str())
            .collect();
        assert!(area_scoped.contains(&"compiler/a.md"));
        assert!(!area_scoped.contains(&"synthesis/b.md"));
    }

    #[test]
    fn orient_file_mode_walks_upstream() {
        let mut graph = DiGraph::new();
        let target = graph.add_node(file_with_size("x.md", Some("active"), 2_000));
        let upstream = graph.add_node(file_with_size("y.md", Some("active"), 2_000));
        // An unrelated node that shouldn't appear in the upstream walk.
        graph.add_node(file_with_size("z.md", Some("active"), 2_000));
        graph.add_edge(target, upstream, EdgeKind::DependsOn);

        let node_index = test_node_index(&graph);
        let config = OrientConfig::default();
        let (files, labels) = empty_snippets();
        let snippets = SnippetIndex {
            files: &files,
            labels: &labels,
        };
        let lattice = test_lattice();

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            lattice: &lattice,
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
        let lattice = test_lattice();

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            lattice: &lattice,
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
        let lattice = test_lattice();

        let output = cmd_orient(&OrientOptions {
            graph: &graph,
            node_index: &node_index,
            config: &config,
            lattice: &lattice,
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
