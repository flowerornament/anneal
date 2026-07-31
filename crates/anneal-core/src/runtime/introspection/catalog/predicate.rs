//! Derived-predicate relationships, joins, requirements, and examples.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PredicateFamily {
    ConvergenceEnergy,
    CorpusArea,
    Blocking,
    Flow,
    DependencyValidity,
    FrontmatterMappingAlias,
    PipelineStall,
    AbandonedNamespace,
    ConcernPair,
    RetiredObligation,
    GitRecency,
    FrontmatterVocabulary,
}

impl PredicateFamily {
    fn for_name(name: &str) -> Option<Self> {
        match name {
            "entropy" | "entropy_priority" | "primary_entropy" | "potential_subject"
            | "potential" | "potential_weight" | "frontier" | "ranked_work" => {
                Some(Self::ConvergenceEnergy)
            }
            "area"
            | "area_of"
            | "area_file_count"
            | "area_error_location_count"
            | "area_error_count"
            | "area_cross_edges"
            | "area_health"
            | "area_frontier" => Some(Self::CorpusArea),
            "blocked" | "blocker" => Some(Self::Blocking),
            "advancing"
            | "recently_advanced"
            | "holding"
            | "regressed"
            | "re_opened"
            | "drifting"
            | "flow"
            | "snapshot_history_present" => Some(Self::Flow),
            "dependency_dead_status"
            | "dependency_valid_status"
            | "dependency_status_classification" => Some(Self::DependencyValidity),
            "frontmatter_mapping_alias" | "configured_frontmatter_alias" => {
                Some(Self::FrontmatterMappingAlias)
            }
            "frontmatter_reference_name_fragment"
            | "frontmatter_reference_name_signal"
            | "unmodeled_frontmatter_key" => Some(Self::FrontmatterVocabulary),
            "pipeline_stall" | "s003_pipeline_stall" => Some(Self::PipelineStall),
            "abandoned_namespace" | "s004_abandoned_namespace" => Some(Self::AbandonedNamespace),
            "top_pair" | "s005_top_pair" => Some(Self::ConcernPair),
            "obligation" | "undischarged" => Some(Self::RetiredObligation),
            "git_mtime" | "changed_within" => Some(Self::GitRecency),
            _ => None,
        }
    }
}

/// Complete shared teaching policy for one runtime name.
pub(in crate::runtime::introspection) struct RuntimeTeaching {
    pub(in crate::runtime::introspection) relationship: Option<&'static str>,
    pub(in crate::runtime::introspection) common_joins: &'static [&'static str],
    pub(in crate::runtime::introspection) requires: &'static [&'static str],
    pub(in crate::runtime::introspection) see_also: &'static [&'static str],
    pub(in crate::runtime::introspection) example: Option<&'static str>,
    pub(in crate::runtime::introspection) extra_lines: Vec<String>,
}

/// Assembles every shared teaching facet from one family classification.
pub(in crate::runtime::introspection) fn runtime_teaching(name: &str) -> RuntimeTeaching {
    let family = PredicateFamily::for_name(name);
    RuntimeTeaching {
        relationship: predicate_relationship(name, family),
        common_joins: common_joins_for(name, family),
        requires: predicate_requires(name, family),
        see_also: predicate_see_also(name, family),
        example: predicate_example(name, family),
        extra_lines: predicate_extra_lines(name, family),
    }
}

/// Names evidence or configuration required by a derived predicate.
fn predicate_requires(name: &str, family: Option<PredicateFamily>) -> &'static [&'static str] {
    match name {
        _ if family == Some(PredicateFamily::ConvergenceEnergy)
            && !matches!(name, "entropy_priority" | "potential_weight") =>
        {
            &[
                "stored handles plus the relevant diagnostic, obligation, lifecycle, freshness, or graph facts that create unsettled-work signals.",
            ]
        }
        "entropy_priority" => {
            &["`potential_weight` rows for the same source; lower priority values win ties."]
        }
        _ if family == Some(PredicateFamily::CorpusArea) && name != "area_of" => &[
            "`area_of` rows from source facts. Area health also uses diagnostics, edges, and potential convergence signals.",
        ],
        _ if family == Some(PredicateFamily::Blocking) => {
            &["active lifecycle config, at least one potential signal, and no recent status flux."]
        }
        "broken_reference" => {
            &["stored edges and handles; section-reference placeholders are excluded."]
        }
        "stale_reference" => &[
            "DependsOn edges, active/terminal lifecycle facts, and the effective dependency-validity classification of the target status.",
        ],
        "confidence_gap" => {
            &["DependsOn edges plus lifecycle status facts for both source and target handles."]
        }
        "undischarged_obligation" | "multiple_discharge" => {
            &["linear namespace policy in anneal.dl plus Discharges edge counts."]
        }
        "implausible_ref" => {
            &["markdown extraction metadata for references rejected by the plausibility filter."]
        }
        "lifecycle_config_gap" => {
            &["handle statuses plus `config convergence` active, terminal, and ordering entries."]
        }
        _ if family == Some(PredicateFamily::DependencyValidity) => &[
            "conservative builtin status classifications plus per-status `config dependency` overrides.",
        ],
        "dependency_config_gap" => &[
            "actual terminal handles whose statuses are absent from the effective dependency-validity classification.",
        ],
        "frontmatter_mapping_alias" => &[
            "the standard prelude's finite exact alias table; it does not inspect corpus values or perform fuzzy matching.",
        ],
        "configured_frontmatter_alias" => &[
            "explicit `config frontmatter` edge-kind entries for keys in the finite W007 alias vocabulary.",
        ],
        "frontmatter_mapping_gap" => &[
            "raw markdown frontmatter metadata, markdown parent-directory metadata, and the absence of a project mapping for an exact built-in alias.",
        ],
        _ if family == Some(PredicateFamily::FrontmatterVocabulary) => &[
            "markdown file metadata classified as authored_unmodeled, excluding every key covered by the finite W007 alias vocabulary.",
        ],
        "missing_frontmatter_file" => &[
            "parent-directory metadata and enough neighboring frontmatter adoption to make the omission suspicious.",
        ],
        "orphaned_handle" => &["label or version handles plus graph in-degree counts."],
        _ if family == Some(PredicateFamily::PipelineStall) => &[
            "configured lifecycle ordering, current status population, and automatic snapshot history.",
        ],
        _ if family == Some(PredicateFamily::AbandonedNamespace) => {
            &["active namespace membership, lifecycle status, and freshness."]
        }
        _ if family == Some(PredicateFamily::ConcernPair) => {
            &["namespace co-occurrence in file references plus configured concern groups."]
        }
        _ if family == Some(PredicateFamily::Flow) => &[
            "snapshot history and configured lifecycle ordering. On a corpus with no snapshots, these predicates return no rows.",
        ],
        "recent_frontier" => &[
            "date-backed authored_age is the dominant clock, with git-backed changed_recently only as a coarse lower-authority no-date fallback. Terminal and superseded files are excluded; statusless files remain eligible.",
        ],
        "anchor" => &[
            "file handles plus authority, curated-name, incoming-edge, and weak recency signals. Terminal files need an explicit authoritative-style status to remain eligible.",
        ],
        "ranked_anchor" => &[
            "the uncapped anchor(h, score, why) relation plus anchor_signal(h, score, priority, why) for per-signal provenance. Explicit CLI JSON adds signals:[{why, score}] when the query directly projects ranked_anchor.",
        ],
        "configured_pipeline_status"
        | "next_pipeline_status"
        | "status_population"
        | "previous_status_population" => {
            &["`config convergence { ordering([...]). }` in anneal.dl."]
        }
        _ => &[],
    }
}

/// Places a derived predicate relative to adjacent runtime concepts.
fn predicate_relationship(name: &str, family: Option<PredicateFamily>) -> Option<&'static str> {
    match name {
        "diagnostic" => Some(
            "Shared diagnostic stream used by `status`, `check`, and eval diagnostics; individual rules contribute rows by diagnostic code.",
        ),
        "potential" => Some(
            "Canonical raw-energy predicate for handles an agent could improve; use `frontier` for the capped top projection.",
        ),
        "potential_weight" => Some(
            "Default calibration table. Project rules may shadow this predicate by name and arity to retune convergence energy.",
        ),
        "frontier" => Some(
            "Canonical global convergence frontier; paired with `area_frontier` for area-scoped work.",
        ),
        "recent_frontier" => Some(
            "Goal-less orientation frontier: date-backed authored-recent files a cold agent should inspect first, with only coarse lower-authority git change bands for undated files. Unlike `frontier`, this is about reading orientation, not potential work energy.",
        ),
        "anchor" => Some(
            "Goal-less orientation anchors: durable read-first files such as authoritative models, living READMEs, curated indexes, and high-inbound references.",
        ),
        "ranked_anchor" => Some(
            "Dense-ranked projection of `anchor`; useful when you want the top few durable read-first files. The `why` column is the dominant summary; inspect `anchor_signal` for the contribution set.",
        ),
        "blocked" => Some("Used by `blocker` and the blocked section of `status`."),
        "blocker" => Some(
            "Canonical focused blocker view: blocked handle, total energy, and each signal explaining it. Join `primary_entropy` when you want one row per handle.",
        ),
        "holding" => Some(
            "A flow leaf for active handles with remaining potential whose status did not change since the latest snapshot.",
        ),
        "regressed" => Some(
            "A drifting leaf for active handles that moved backward in the configured lifecycle since the latest snapshot.",
        ),
        "re_opened" => Some(
            "A drifting leaf for handles that were terminal at the latest snapshot and active now.",
        ),
        "drifting" => Some(
            "A flow leaf that unifies `regressed(h)` and `re_opened(h)` as movement away from settledness.",
        ),
        "flow" => Some(
            "Coarse convergence direction: advancing, holding, or drifting. Settled handles are intentionally outside flow.",
        ),
        "broken_reference" => {
            Some("Diagnostic-rule predicate behind E001 broken-reference errors.")
        }
        "undischarged_obligation" => {
            Some("Diagnostic-rule predicate behind E002 undischarged-obligation errors.")
        }
        "stale_reference" => Some(
            "Diagnostic-rule predicate behind W001 stale-reference warnings. Dependency deadness narrows the terminal target gate; it never widens it.",
        ),
        "spec_code_drift" => Some(
            "Diagnostic-rule predicate behind W006 spec-code-drift warnings. It uses asserts_code(status), not bare active(h), and target history, not bare absence, to avoid warning on examples, forward plans, or external-code studies.",
        ),
        "confidence_gap" => Some("Diagnostic-rule predicate behind W002 confidence-gap warnings."),
        "missing_frontmatter_file" => {
            Some("Diagnostic-rule predicate behind W003 missing-frontmatter warnings.")
        }
        "implausible_ref" => {
            Some("Diagnostic-rule predicate behind W004 implausible-reference warnings.")
        }
        "lifecycle_config_gap" => Some(
            "Diagnostic-rule predicate behind W005 warnings for lifecycle vocabulary that neither project config nor builtin policy models.",
        ),
        "dependency_config_gap" => {
            Some("Diagnostic-rule predicate behind S006 dependency-config-gap suggestions.")
        }
        "frontmatter_mapping_gap" => Some(
            "Diagnostic-rule predicate behind W007 frontmatter-mapping-gap warnings. It recognizes a finite exact alias vocabulary and never infers edges from fuzzy key similarity.",
        ),
        "unmodeled_frontmatter_key" => Some(
            "Inverse-discovery inventory of authored markdown vocabulary that no typed projection consumes and W007 does not already report. Usage count dominates; the lexical signal only breaks ties.",
        ),
        "frontmatter_reference_name_signal" => Some(
            "Finite lowercase substring signal used only to rank equal-count unmodeled keys. It does not infer a relationship or recommend an edge mapping.",
        ),
        "frontmatter_reference_name_fragment" => Some(
            "Queryable finite lowercase fragment vocabulary behind the reference-name ranking signal.",
        ),
        "dependency_status_classification" => Some(
            "Effective dependency-validity classification with origin: project entries override conservative builtins one status at a time.",
        ),
        _ if family == Some(PredicateFamily::DependencyValidity)
            && name != "dependency_status_classification" =>
        {
            Some(
                "Effective unary projections of dependency_status_classification; use the three-column relation when classification provenance matters.",
            )
        }
        "orphaned_handle" => {
            Some("Diagnostic-rule predicate behind S001 orphaned-handle suggestions.")
        }
        "pipeline_stall" => Some(
            "Diagnostic-rule predicate behind S003 pipeline-stall suggestions. It only emits after automatic snapshot history exists.",
        ),
        "abandoned_namespace" => {
            Some("Diagnostic-rule predicate behind S004 abandoned-namespace suggestions.")
        }
        "top_pair" => {
            Some("Diagnostic-rule predicate behind S005 concern-group-candidate suggestions.")
        }
        "area_of" => Some(
            "Source-neutral area lens over `*handle.area`; use it to group queries by corpus area.",
        ),
        "area_health" => Some(
            "Use directly in eval to grade each corpus area by local errors and cross-area connectivity.",
        ),
        "area_frontier" => Some(
            "Use directly in eval to pick the strongest unsettled-work handles inside each area.",
        ),
        _ => None,
    }
}

/// Returns predicate-specific teaching details beyond the declared doc string.
fn predicate_extra_lines(name: &str, family: Option<PredicateFamily>) -> Vec<String> {
    match name {
        "potential_weight" => vec![
            "Default weights in v0.15: undischarged=5, broken_ref=4, stale_dep=3, spec_code_drift=3, confidence_gap=3, freshness_decay=1, missing_meta=1, orphan_label=1.".to_string(),
            "Retune by declaring a project predicate with the same name and arity, for example `potential_weight(\"freshness_decay\", 0).`.".to_string(),
        ],
        "diagnostic" => vec![
            "Hidden CI gate: `anneal check` is retained for pre-commit/release gates and exits 1 when error rows exist.".to_string(),
            "Canonical error query: `anneal -e '? diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}.'`.".to_string(),
        ],
        "flow" => vec![
            "Directions are exactly \"advancing\", \"holding\", and \"drifting\". Leaf predicates explain why a handle entered a direction.".to_string(),
            "`regressed(h)` and `re_opened(h)` are drifting leaves, not extra flow directions.".to_string(),
            "Settled handles are excluded so flow stays about active movement, not completed state.".to_string(),
        ],
        "holding" => vec![
            "Holding means stuck with work remaining: the handle is active, has potential, and has the same status at snapshot:last and now.".to_string(),
            "This intentionally excludes settled or inactive handles that simply did not change.".to_string(),
        ],
        "drifting" => vec![
            "Drifting means moving away from settledness. Inspect `regressed(h)` and `re_opened(h)` to see which leaf fired.".to_string(),
            "Use `flow(h, \"drifting\")` when you only need the coarse direction.".to_string(),
        ],
        "regressed" => vec![
            "Regression compares configured pipeline positions at snapshot:last and now.".to_string(),
            "If a status has no configured position, it cannot produce a regression row.".to_string(),
        ],
        "re_opened" => vec![
            "Re-opened handles were terminal at snapshot:last and active now.".to_string(),
            "This is tracked separately from generic regression because terminal-to-active movement often means a settled claim was reopened.".to_string(),
        ],
        "blocker" => vec![
            "A blocked handle can emit multiple rows when several entropy sources explain it.".to_string(),
            "Join `primary_entropy(h, source)` with the same source variable for one row per blocked handle.".to_string(),
        ],
        "recent_frontier" => vec![
            "Ranking shape: lower recency means newer; authored dates dominate, active status is a boost, statusless files remain eligible, and curated hubs are de-prioritized.".to_string(),
            "Use `--limit` on the eval command for a reading-list budget; then join to `read` or `context` when you have a goal.".to_string(),
        ],
        "anchor" => vec![
            "Ranking shape: explicit authoritative/living/current status outranks curated names; incoming degree and recency are bounded supporting signals.".to_string(),
            "The predicate is intentionally uncapped; use `? ranked_anchor(h, rank, score, why) order by rank asc.` with eval `--limit N` for a bounded read-first list.".to_string(),
            "The `why` column names the strongest signal: authoritative_status, curated_name, inbound_degree, or recent.".to_string(),
        ],
        "ranked_anchor" => vec![
            "Add `order by rank asc --limit N` when you need a budgeted top-N anchor list.".to_string(),
            "Explicit JSON keeps h/rank/score/why and additively emits `signals: [{why, score}]` from `anchor_signal(h, score, priority, why)`.".to_string(),
            "Text stays compact; use `anneal -e '? anchor_signal(h, s, prio, why).'` to drill into the contribution set.".to_string(),
        ],
        "status_item" => vec![
            "The `why` column is currently a lossless prioritized status reason on the self-corpus; no JSON signal set is emitted.".to_string(),
            "Inspect leaf predicates such as blocker, entropy, primary_entropy, regressed, and re_opened when a status row needs explanation.".to_string(),
        ],
        "asserts_code" => vec![
            "Config syntax: config convergence { asserts_code([stable, current, authoritative, active, draft]). }".to_string(),
            "Default when unconfigured: active status-bearing handles minus the aspirational study tier: plan, research, reference, exploratory.".to_string(),
            "W006 spec_code_drift uses this gate instead of bare active(h), so forward plans and external-code studies do not look like rot.".to_string(),
        ],
        _ if family == Some(PredicateFamily::RetiredObligation) => vec![
            "Retired obligations equivalent: `anneal -e '? undischarged(h), obligation(h), *handle{id: h, file: file, status: status}.'`.".to_string(),
        ],
        "lifecycle_config_gap" => lifecycle_config_gap_variant_lines(),
        _ if family == Some(PredicateFamily::DependencyValidity) => vec![
            "Builtins: dead = superseded, retired, archived, historical, deprecated; valid = authoritative, complete, decided, stable, ratified.".to_string(),
            "Override one status with `config dependency { dead([\"custom-retired\"]). }` or `config dependency { valid([\"custom-current\"]). }`; a project entry replaces that status's builtin meaning.".to_string(),
            "Query `dependency_status_classification(status, classification, origin)` to see the effective set and whether each row is builtin or project.".to_string(),
        ],
        "dependency_config_gap" => dependency_config_gap_lines(),
        "frontmatter_mapping_gap" => frontmatter_mapping_gap_lines(),
        _ => Vec::new(),
    }
}

/// Returns evidence-shape details for diagnostic code cards.
pub(in crate::runtime::introspection) fn diagnostic_code_extra_lines(code: &str) -> Vec<String> {
    match code {
        "W005" => lifecycle_config_gap_variant_lines(),
        "W007" => frontmatter_mapping_gap_lines(),
        "S006" => dependency_config_gap_lines(),
        _ => Vec::new(),
    }
}

fn lifecycle_config_gap_variant_lines() -> Vec<String> {
    vec![
        "Variants: used_status_unpartitioned = a handle uses a status with no effective lifecycle classification.".to_string(),
        "Variants: ordering_status_unpartitioned = convergence.ordering names a status with no effective lifecycle classification.".to_string(),
        "Variants: ordering_not_terminal = the final ordered status is not terminal, so the lattice cannot settle.".to_string(),
        "Builtin pipeline: raw, draft, research, plan, current, active, stable, authoritative. Builtin settled: authoritative, current, active, stable, living.".to_string(),
        "Builtin terminal stems: superseded, archived, historical, prior, retired, deprecated, obsolete, withdrawn, cancelled/canceled, closed, resolved, done, completed, incorporated, digested.".to_string(),
        "Query `lifecycle_status_classification(status, classification, origin)` to inspect effective builtin and project classifications; unknown statuses intentionally produce no row.".to_string(),
    ]
}

fn dependency_config_gap_lines() -> Vec<String> {
    vec![
        "Variant: terminal_status_unclassified = actual terminal handles use a status whose dependency validity is unknown.".to_string(),
        "Classify a dead target with `config dependency { dead([\"custom-retired\"]). }`, or a still-valid target with `config dependency { valid([\"custom-current\"]). }`.".to_string(),
        "W001 remains silent until the status is classified dead; the aggregate suggestion preserves the unknown instead of guessing.".to_string(),
    ]
}

fn frontmatter_mapping_gap_lines() -> Vec<String> {
    vec![
        "The count is distinct markdown file handles, not scalar values.".to_string(),
        "Drill down with `? frontmatter_mapping_gap(key, count, field, kind, direction), *meta{handle: h, key: key}, *meta{handle: h, key: \"md.parent_dir\"}.`.".to_string(),
        "Configure the reported mapping with `config frontmatter { field(\"KEY\", \"EDGE_KIND\", \"DIRECTION\"). }`; a project mapping suppresses W007 for that key.".to_string(),
        "The finite alias vocabulary is active even when the corpus has no `config frontmatter` block. Generic source/sources remain unclassified pending a deliberate built-in mapping policy.".to_string(),
    ]
}

fn common_joins_for(name: &str, family: Option<PredicateFamily>) -> &'static [&'static str] {
    match name {
        "diagnostic" => &[
            "`diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}` mirrors `anneal check` rows",
            "`diagnostic{subject: h}, area_of{h: h, area: \"X\"}` for area filtering",
            "`diagnostic{subject: h}, *handle{id: h, kind: \"file\"}` for file-handle diagnostics",
        ],
        "snapshot" => &[
            "`at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now` mirrors retired diff",
            "`*snapshot{snapshot: snapshot, id: h, key: \"status\", value: status}` to inspect raw status history rows",
        ],
        "search" => &[
            "`search{query: \"text\", handle: h, span_id: span_id, score: score}, *span{handle: h, id: span_id, summary: summary}` to add span summary",
            "`search{query: \"text\", handle: h, score: score}, *handle{id: h, status: status}` to inspect status-aware ranking",
            "`search{query: \"text\", handle: h, span_id: span_id}, read(h, 4000, span_id, text, start, end, tokens)` to read matched spans",
        ],
        "meta" => &[
            "`*meta{handle: h, key: \"external_class\", value: class}` to inspect standard external sub-classes",
            "`*meta{handle: h, key: \"target_path\", value: path}` to inspect standard external targets",
            "`*meta{handle: h, key: k}` to discover corpus frontmatter keys",
        ],
        "read" => &[
            "`search{query: \"text\", handle: h, span_id: span_id}, read(h, 4000, span_id, text, start, end, tokens)` to read the matched heading span",
            "`*span{handle: h, id: span_id, summary: summary}, read(h, 4000, span_id, text, start, end, tokens)` to read by heading hierarchy",
        ],
        "context" => &[
            "`context` composes ranked section search, compact span metadata, and graph neighborhood rows for cold-agent orientation",
            "`search{query: \"text\", handle: h, span_id: span_id}, read(h, 4000, span_id, text, start, end, tokens)` when you need the same retrieval pieces manually",
        ],
        "handle" => &[
            "`*edge{to: h, from: src}, *handle{id: src, kind: kind}` mirrors `anneal handle H --impact` direct reverse dependencies",
            "`impact(h, affected, depth), *handle{id: affected, file: file}` for composable downstream traversal",
        ],
        "upstream" => &[
            "`upstream{h: h, anc: anc}, diagnostic{subject: anc}` to find broken upstream context",
            "`upstream{h: h, anc: anc}, *handle{id: anc, kind: \"file\"}` to keep upstream files only",
        ],
        "downstream" => &[
            "`downstream{h: h, desc: desc}, diagnostic{subject: desc}` to find affected diagnostics",
            "`downstream{h: h, desc: desc}, area_of{h: desc, area: area}` to group dependents by area",
        ],
        "potential" => &[
            "`potential(h, energy), entropy(h, source)` to explain raw energy",
            "`potential(h, energy), primary_entropy(h, source)` to keep one strongest reason per handle",
            "`potential(h, energy), frontier(h, energy)` to keep only the global frontier",
        ],
        "frontier" => &[
            "`frontier(h, energy), diagnostic{subject: h}` to see what blocks the frontier",
            "`frontier(h, energy), area_of{h: h, area: \"X\"}` for area-scoped frontier work",
        ],
        "recent_frontier" => &[
            "`recent_frontier(h, rank, recency), *handle{id: h, file: file, status: status} order by rank asc` for a goal-less reading frontier",
            "`recent_frontier(h, rank, recency), area_of{h: h, area: \"X\"}` to scope orientation to one area",
            "`recent_frontier(h, rank, recency), read(h, 1200, null, text, start, end, tokens)` to sample each file body",
        ],
        "anchor" => &[
            "`anchor(h, score, why), *handle{id: h, file: file, status: status}` for durable read-first files",
            "`anchor(h, score, why), incoming_edge(h, from, kind)` to inspect why a graph hub matters",
            "`anchor(h, score, why), area_of{h: h, area: area}` to group anchors by corpus area",
        ],
        "ranked_anchor" => &[
            "`ranked_anchor(h, rank, score, why), *handle{id: h, file: file} order by rank asc` with eval `--limit 12` for a budgeted anchor list",
            "`ranked_anchor(h, rank, score, why), h = \"HANDLE\"` to inspect one anchor's rank",
            "`anchor_signal(h, score, priority, why), h = \"HANDLE\" order by priority asc` to inspect the contribution set",
        ],
        "status_item" => &[
            "`status_item(section, h, score, why), blocker(h, score, why)` to inspect blocked rows",
            "`status_item(\"drifting\", h, score, why), regressed(h)` or `re_opened(h)` to inspect drifting leaves",
            "`status_item(section, h, score, why), primary_entropy(h, why)` to compare prioritized convergence reasons",
        ],
        _ if family == Some(PredicateFamily::Blocking) => &[
            "`blocked(h), entropy(h, source)` to see the unsettled signal",
            "`blocker(h, energy, source), primary_entropy(h, source)` to keep one strongest blocker row per handle",
            "`blocker(h, energy, source), *handle{id: h, file: file}` to add location metadata",
            "`blocked(h), area_of{h: h, area: \"X\"}` for area-scoped blockers",
        ],
        "broken_reference" => &[
            "`broken_reference(src, target, file, line), diagnostic{code: \"E001\", subject: src}` to inspect broken-reference diagnostics",
            "`broken_reference(src, target, file, line), *handle{id: src, summary: summary}` to add source context",
        ],
        "undischarged_obligation" => &[
            "`undischarged_obligation(h, file), diagnostic{code: \"E002\", subject: h}` to inspect undischarged-obligation errors",
            "`undischarged_obligation(h, file), area_of{h: h, area: area}` to group open obligations by area",
        ],
        "stale_reference" => &[
            "`stale_reference(src, target, file, source_status, target_status), diagnostic{code: \"W001\", subject: src}` to inspect stale-reference warnings",
            "`stale_reference(src, target, file, source_status, target_status), *handle{id: target, summary: summary}` to add target context",
        ],
        "spec_code_drift" => &[
            "`spec_code_drift(src, target_path, file, line, source_status), diagnostic{code: \"W006\", subject: src}` to inspect spec-code drift warnings",
            "`spec_code_drift(src, target_path, file, line, source_status), asserts_code(source_status)` to inspect the lifecycle gate",
            "`spec_code_drift(src, target_path, file, line, source_status), read{handle: src, budget: 1200, text: text}` to read the live spec context",
            "`spec_code_drift(src, target_path, file, line, source_status), *edge{from: src, to: ref, kind: \"Cites\"}, *meta{handle: ref, key: \"target_probe_base\", value: base}` to audit path resolution",
        ],
        "asserts_code" => &[
            "`asserts_code(status)` to inspect the effective status set",
            "`*config{key: \"convergence.asserts_code\", value: status}` to inspect explicit project config",
            "`spec_code_drift(src, target_path, file, line, status), asserts_code(status)` to audit W006 gating",
        ],
        "confidence_gap" => &[
            "`confidence_gap(src, target, file, source_status, source_level, target_status, target_level), diagnostic{code: \"W002\", subject: src}` to inspect confidence-gap warnings",
            "`confidence_gap(src, target, file, source_status, source_level, target_status, target_level), area_of{h: src, area: area}` to group gaps by area",
        ],
        "missing_frontmatter_file" => &[
            "`missing_frontmatter_file(h, dir, file), diagnostic{code: \"W003\", subject: h}` to inspect missing-frontmatter warnings",
            "`missing_frontmatter_file(h, dir, file), area_of{h: h, area: area}` to group missing metadata by area",
        ],
        "implausible_ref" => &[
            "`implausible_ref(h, file, value), diagnostic{code: \"W004\", subject: h}` to inspect implausible-reference warnings",
            "`implausible_ref(h, file, value), read{handle: h, budget: 1200, text: text}` to read nearby evidence",
        ],
        "lifecycle_config_gap" => &[
            "`lifecycle_config_gap(status, count, variant), diagnostic{code: \"W005\", subject: status}` to inspect lifecycle config warnings",
            "`lifecycle_status_classification(status, classification, origin)` to inspect the effective lifecycle vocabulary and provenance",
        ],
        "dependency_config_gap" => &[
            "`dependency_config_gap(status, count, variant), diagnostic{code: \"S006\", subject: status}` to inspect unclassified terminal statuses",
            "`dependency_config_gap(status, count, variant), *handle{id: h, status: status}, terminal(h)` to inspect affected terminal handles",
        ],
        "frontmatter_mapping_gap" => &[
            "`frontmatter_mapping_gap(key, count, field, kind, direction), diagnostic{code: \"W007\", subject: key}` to inspect the supported recovery mapping",
            "`frontmatter_mapping_gap(key, count, field, kind, direction), *meta{handle: h, key: key}, *meta{handle: h, key: \"md.parent_dir\"}` to list the markdown handles carrying that unmapped key",
        ],
        "unmodeled_frontmatter_key" => &[
            "`unmodeled_frontmatter_key(key, count, signal, rank)` to inspect the ranked key inventory",
            "`unmodeled_frontmatter_key(key, count, signal, rank), *meta{handle: h, key: key, role: \"authored_unmodeled\"}` to list carrying handles",
        ],
        _ if family == Some(PredicateFamily::DependencyValidity) => &[
            "`dependency_status_classification(status, classification, origin)` to inspect the effective set and provenance",
            "`dependency_dead_status(status), *handle{id: target, status: status}, terminal(target)` to inspect possible W001 targets",
        ],
        "orphaned_handle" => &[
            "`orphaned_handle(h), diagnostic{code: \"S001\", subject: h}` to inspect orphaned-handle suggestions",
            "`orphaned_handle(h), *handle{id: h, namespace: namespace}` to group orphans by namespace",
        ],
        _ if family == Some(PredicateFamily::PipelineStall) => &[
            "`pipeline_stall(status, count, next_status, based_on_history), diagnostic{code: \"S003\", subject: status}` to inspect stalled lifecycle statuses",
            "`snapshot_history_present(count), pipeline_stall(status, stalled, next_status, true)` to confirm automatic status snapshots have accrued",
        ],
        _ if family == Some(PredicateFamily::AbandonedNamespace) => &[
            "`abandoned_namespace(namespace, total, terminal_count, stale_count), diagnostic{code: \"S004\", subject: namespace}` to inspect abandoned namespace suggestions",
            "`abandoned_namespace(namespace, total, terminal_count, stale_count), namespace_label(namespace, h)` to inspect members",
        ],
        _ if family == Some(PredicateFamily::ConcernPair) => &[
            "`top_pair(left_prefix, right_prefix, count), diagnostic{code: \"S005\", subject: left_prefix}` to inspect concern-group candidates",
            "`top_pair(left_prefix, right_prefix, count), same_concern_pair(left_prefix, right_prefix)` to test whether a configured concern already covers it",
        ],
        "entropy" => &[
            "`entropy(h, source), potential(h, energy)` to see weighted convergence reasons",
            "`entropy(h, source), diagnostic{subject: h}` to connect signals to diagnostics",
        ],
        _ if family == Some(PredicateFamily::RetiredObligation) => &[
            "`undischarged(h), obligation(h), *handle{id: h, file: file, status: status}` mirrors retired obligations",
            "`undischarged(h), *handle{id: h, namespace: \"OQ\"}` for namespace-scoped obligations",
            "`undischarged(h), area_of{h: h, area: area}` to group open obligations by area",
        ],
        _ if family == Some(PredicateFamily::GitRecency) => &[
            "`*handle{id: h, file: file}, git_mtime(file, instant)` to inspect raw git-backed change time",
            "`changed_within(h, 7), *handle{id: h, kind: \"file\", summary: summary}` to keep the result at file granularity",
            "`changed_within(h, 7), search{query: \"text\", handle: h}` for lower-authority recently-edited search hits",
        ],
        "potential_weight" => &[
            "`potential_weight(source, weight), entropy(h, source)` to see which handles use each weight",
            "`potential(h, energy), primary_entropy(h, source)` to inspect the weighted result",
        ],
        _ if family == Some(PredicateFamily::Flow)
            && !matches!(name, "recently_advanced" | "snapshot_history_present") =>
        {
            &[
                "`flow(h, direction), *handle{id: h, status: status}` to add current lifecycle state",
                "`drifting(h), re_opened(h)` to separate reopened handles from ordinary regressions",
                "`holding(h), potential(h, energy)` to prioritize stuck handles with work remaining",
            ]
        }
        _ if family == Some(PredicateFamily::CorpusArea)
            && matches!(name, "area_of" | "area_health" | "area_frontier") =>
        {
            &[
                "`area_of{h: h, area: \"X\"}, frontier(h, energy)` for area-scoped work",
                "`area_of{h: h, area: \"X\"}, diagnostic{subject: h}` for area-scoped diagnostics",
            ]
        }
        _ => &[],
    }
}

/// Returns adjacent vocabulary for predicate drill-down.
fn predicate_see_also(name: &str, family: Option<PredicateFamily>) -> &'static [&'static str] {
    match name {
        "diagnostic" => &[
            "status",
            "E001",
            "E002",
            "broken_reference",
            "undischarged_obligation",
            "pipeline_stall",
            "abandoned_namespace",
            "top_pair",
        ],
        _ if family == Some(PredicateFamily::ConvergenceEnergy) && name != "entropy_priority" => &[
            "diagnostic",
            "obligation",
            "freshness",
            "hub",
            "orphan",
            "entropy_priority",
        ],
        "recent_frontier" => &[
            "anchor",
            "authored_age",
            "changed_recently",
            "freshness",
            "changed_within",
            "*handle",
            "status",
        ],
        "anchor" => &[
            "recent_frontier",
            "ranked_anchor",
            "incoming_edge",
            "hub",
            "freshness",
            "*handle",
        ],
        "ranked_anchor" => &[
            "anchor",
            "anchor_signal",
            "recent_frontier",
            "incoming_edge",
            "hub",
            "freshness",
            "*handle",
        ],
        _ if family == Some(PredicateFamily::Blocking) => {
            &["potential", "primary_entropy", "entropy", "flux", "status"]
        }
        "broken_reference" => &["E001", "diagnostic", "*edge", "*handle"],
        "undischarged_obligation" => &["E002", "diagnostic", "obligation", "discharge_count"],
        "stale_reference" => &[
            "W001",
            "diagnostic",
            "dependency_dead_status",
            "active",
            "terminal",
        ],
        "spec_code_drift" => &[
            "W006",
            "diagnostic",
            "asserts_code",
            "external_class",
            "target_path",
        ],
        "asserts_code" => &["W006", "spec_code_drift", "convergence", "*config"],
        "confidence_gap" => &[
            "W002",
            "diagnostic",
            "configured_pipeline_status",
            "pipeline_position_for",
        ],
        "missing_frontmatter_file" => &["W003", "diagnostic", "*handle", "*meta"],
        "implausible_ref" => &["W004", "diagnostic", "*meta"],
        "lifecycle_config_gap" => &[
            "W005",
            "diagnostic",
            "lifecycle_status_classification",
            "configured_pipeline_status",
            "pipeline_stall",
        ],
        "dependency_config_gap" => &[
            "S006",
            "diagnostic",
            "dependency_status_classification",
            "dependency-validity",
            "W001",
        ],
        _ if family == Some(PredicateFamily::FrontmatterMappingAlias) => {
            &["frontmatter_mapping_gap", "W007", "*config"]
        }
        "frontmatter_mapping_gap" => &[
            "W007",
            "diagnostic",
            "frontmatter_mapping_alias",
            "*meta",
            "*config",
        ],
        _ if family == Some(PredicateFamily::FrontmatterVocabulary) => &[
            "frontmatter_mapping_alias",
            "frontmatter_mapping_gap",
            "W007",
            "*meta",
        ],
        _ if family == Some(PredicateFamily::DependencyValidity) => &[
            "dependency-validity",
            "dependency_config_gap",
            "stale_reference",
            "W001",
            "*config",
        ],
        "orphaned_handle" => &["S001", "diagnostic", "in_degree"],
        _ if family == Some(PredicateFamily::PipelineStall) => &[
            "S003",
            "diagnostic",
            "status_population",
            "snapshot_history_present",
            "W005",
        ],
        _ if family == Some(PredicateFamily::AbandonedNamespace) => {
            &["S004", "diagnostic", "namespace_label", "freshness"]
        }
        _ if family == Some(PredicateFamily::ConcernPair) => {
            &["S005", "diagnostic", "*concern", "same_concern_pair"]
        }
        "area_of" => &["area", "area_health", "area_frontier", "*handle", "schema"],
        _ if family == Some(PredicateFamily::CorpusArea) => &[
            "area_of",
            "diagnostic",
            "potential",
            "primary_entropy",
            "area_health",
        ],
        _ if family == Some(PredicateFamily::Flow)
            && !matches!(name, "recently_advanced" | "snapshot_history_present") =>
        {
            &[
                "convergence",
                "snapshot_history_present",
                "potential",
                "settled",
            ]
        }
        _ if family == Some(PredicateFamily::RetiredObligation) => {
            &["*config", "discharged", "discharge_count"]
        }
        _ => &[],
    }
}

/// Returns the canonical executable example for a derived predicate.
fn predicate_example(name: &str, family: Option<PredicateFamily>) -> Option<&'static str> {
    match name {
        "entropy" => Some(r#"? entropy("docs/runtime-overview.md", source)."#),
        "entropy_priority" => Some(r#"? entropy_priority("stale_dep", priority)."#),
        "potential_weight" => Some(r#"? potential_weight("freshness_decay", weight)."#),
        "primary_entropy" => Some(r#"? primary_entropy("docs/runtime-overview.md", source)."#),
        "area" => Some("? area(area)."),
        "area_file_count" => Some("? area_file_count(area, files)."),
        "area_error_location_count" => {
            Some("? area_error_location_count(area, code, subject, file, line, count).")
        }
        "area_error_count" => Some("? area_error_count(area, errors)."),
        "area_cross_edges" => Some("? area_cross_edges(area, cross_edges)."),
        "area_health" => Some("? area_health(area, grade, files, errors, cross_edges)."),
        "area_frontier" => Some("? area_frontier{area: area, h: h, score: score}."),
        "potential" => Some(r#"? potential("docs/runtime-overview.md", energy)."#),
        "blocked" => Some(r#"? blocked("docs/runtime-overview.md")."#),
        "blocker" => Some(r#"? blocker("docs/runtime-overview.md", energy, source)."#),
        "advancing" => Some(r#"? advancing("docs/runtime-overview.md")."#),
        "holding" => Some(r#"? holding("docs/runtime-overview.md")."#),
        "regressed" => Some(r#"? regressed("docs/runtime-overview.md")."#),
        "re_opened" => Some(r#"? re_opened("docs/runtime-overview.md")."#),
        "drifting" => Some(r#"? drifting("docs/runtime-overview.md")."#),
        "flow" => Some("? flow(h, direction)."),
        "frontier" => Some("? frontier(h, energy)."),
        "recent_frontier" => Some("? recent_frontier(h, rank, recency)."),
        "authored_age" => Some("? authored_age(h, days)."),
        "changed_recently" => Some("? changed_recently(h, band)."),
        "anchor" => Some("? anchor(h, score, why)."),
        "ranked_anchor" => Some("? ranked_anchor(h, rank, score, why)."),
        "ranked_work" => Some("? ranked_work(h, energy, rank)."),
        "broken_reference" => Some("? broken_reference(src, target, file, line)."),
        "undischarged_obligation" => Some("? undischarged_obligation(h, file)."),
        "stale_reference" => {
            Some("? stale_reference(src, target, file, source_status, target_status).")
        }
        "spec_code_drift" => {
            Some("? spec_code_drift(src, target_path, file, line, source_status).")
        }
        "asserts_code" => Some("? asserts_code(status)."),
        "confidence_gap" => Some(
            "? confidence_gap(src, target, file, source_status, source_level, target_status, target_level).",
        ),
        "missing_frontmatter_file" => Some("? missing_frontmatter_file(h, dir, file)."),
        "implausible_ref" => Some("? implausible_ref(h, file, value)."),
        "lifecycle_config_gap" => Some("? lifecycle_config_gap(status, count, variant)."),
        "dependency_config_gap" => Some("? dependency_config_gap(status, count, variant)."),
        "frontmatter_mapping_gap" => Some(
            "? frontmatter_mapping_gap(key, distinct_handle_count, suggested_field, edge_kind, direction).",
        ),
        "frontmatter_mapping_alias" => {
            Some("? frontmatter_mapping_alias(key, suggested_field, edge_kind, direction).")
        }
        "configured_frontmatter_alias" => Some("? configured_frontmatter_alias(key)."),
        "frontmatter_reference_name_signal" => Some("? frontmatter_reference_name_signal(key)."),
        "frontmatter_reference_name_fragment" => {
            Some("? frontmatter_reference_name_fragment(fragment).")
        }
        "unmodeled_frontmatter_key" => Some(
            "? unmodeled_frontmatter_key(key, distinct_file_handles, reference_name_signal, rank).",
        ),
        "dependency_dead_status" => Some("? dependency_dead_status(status)."),
        "dependency_valid_status" => Some("? dependency_valid_status(status)."),
        "dependency_status_classification" => {
            Some("? dependency_status_classification(status, classification, origin).")
        }
        "orphaned_handle" => Some("? orphaned_handle(h)."),
        _ if family == Some(PredicateFamily::PipelineStall) => {
            Some("? pipeline_stall(status, count, next_status, based_on_history).")
        }
        _ if family == Some(PredicateFamily::AbandonedNamespace) => {
            Some("? abandoned_namespace(namespace, total, terminal_count, stale_count).")
        }
        _ if family == Some(PredicateFamily::ConcernPair) => {
            Some("? top_pair(left_prefix, right_prefix, count).")
        }
        "incoming_edge" => Some(r#"? incoming_edge("REQ-1", from, kind)."#),
        "outgoing_edge" => Some(r#"? outgoing_edge("plan.md", to, kind)."#),
        "area_of" => Some(r#"? area_of{h: "docs/runtime-overview.md", area: area}."#),
        "namespace_of" => Some(r#"? namespace_of("OQ-1", namespace)."#),
        "status_of" => Some(r#"? status_of("docs/runtime-overview.md", status)."#),
        "hub" => Some("? hub(h, degree)."),
        "orphan" => Some("? orphan(h)."),
        "stub" => Some("? stub(h)."),
        "diagnostic" => {
            Some(r#"? diagnostic{code: "E001", severity: severity, subject: subject}."#)
        }
        _ if family == Some(PredicateFamily::RetiredObligation) => {
            Some("? undischarged(h), obligation(h), *handle{id: h, file: file, status: status}.")
        }
        _ => None,
    }
}
