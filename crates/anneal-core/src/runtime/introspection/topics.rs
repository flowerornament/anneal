//! Static runtime-topic, axis, and diagnostic-code teaching cards.

use std::collections::BTreeSet;

use super::render;
use super::render::{DescribeCard, describe_card};
use super::{DescribeEntry, DescribeKind, Tuple, describe_entry, string_value};

#[derive(Clone, Copy, Debug)]
/// Static fields required to render one diagnostic code's teaching card.
pub(super) struct DiagnosticCodeCard {
    pub(super) code: &'static str,
    pub(super) severity: &'static str,
    pub(super) summary: &'static str,
    pub(super) rule: &'static str,
    pub(super) evidence: &'static str,
    pub(super) common_joins: &'static [&'static str],
    pub(super) example: &'static str,
    pub(super) see_also: &'static [&'static str],
}

#[derive(Clone, Copy, Debug)]
struct AxisTopicCard {
    name: &'static str,
    summary: &'static str,
    question: &'static str,
    oracle: &'static str,
    disposition: &'static str,
    member_predicates: &'static str,
    common_joins: &'static [&'static str],
    examples: &'static [&'static str],
    see_also: &'static [&'static str],
}

const AXIS_TOPIC_CARDS: &[AxisTopicCard] = &[
    AxisTopicCard {
        name: "currency",
        summary: "Currency asks whether a file handle has been displaced by a marked Supersedes edge.",
        question: "displaced?",
        oracle: "old-to-new file Supersedes edges; status strings such as superseded stay on the lifecycle axis.",
        disposition: "REPORT: down-rank and annotate; never hide superseded material.",
        member_predicates: "currency_current, currency_current_head, currency_successor, currency_superseded, currency_disposition, hit_currency_disposition, orientation_replaced.",
        common_joins: &[
            "`currency_disposition(h, disposition), *handle{id: h, file: file, status: status}` to read displacement beside lifecycle status",
            "`currency_current_head(h), operative(h), *handle{id: h, file: file}` for boosted current heads",
            "`currency_superseded(h), *edge{from: h, to: newer, kind: \"Supersedes\"}` to inspect the replacement edge",
        ],
        examples: &[
            "? currency_disposition(h, disposition), *handle{id: h, file: file, status: status}.",
            "? currency_current_head(h), operative(h), *handle{id: h, file: file}.",
            "? axis_of(\"currency_current_head\", axis).",
        ],
        see_also: &["lifecycle", "structure", "ranked_anchor"],
    },
    AxisTopicCard {
        name: "lifecycle",
        summary: "Lifecycle asks where a handle sits in the corpus status band: draft, operative, retired, or project-specific equivalents.",
        question: "draft, operative, or retired?",
        oracle: "source status values interpreted through project convergence config and lifecycle helpers; project settled entries do not imply terminality.",
        disposition: "REPORT / PRE-FLIGHT: report observed status; declare missing config or missing status before relying on lifecycle-sensitive claims.",
        member_predicates: "status_of, operative, lifecycle_status_candidate, orientation_retired_status, asserts_code, aspirational_code_status, frontmatter_adoption_high.",
        common_joins: &[
            "`status_of(h, status), *handle{id: h, file: file}` to inspect source-provided status",
            "`operative(h), *handle{id: h, file: file}` for handles eligible for current-head ranking boosts",
            "`lifecycle_status_classification(status, classification, origin)` to inspect effective builtin and project lifecycle meanings",
            "`lifecycle_config_gap(status, count, variant), diagnostic(\"W005\", severity, status, file, line, evidence)` for config evidence",
        ],
        examples: &[
            "? status_of(h, status), *handle{id: h, file: file}.",
            "? operative(h), *handle{id: h, file: file}.",
            "? lifecycle_status_classification(status, classification, origin).",
            "? axis_of(\"operative\", axis).",
        ],
        see_also: &["currency", "convergence", "diagnostic"],
    },
    AxisTopicCard {
        name: "recency",
        summary: "Recency asks when a handle was authored, changed, or observed, while keeping the three clocks separate.",
        question: "authored, changed, or observed when?",
        oracle: "date-backed authored_age for authored age, git mtime for lower-authority change recency, snapshots for observed history.",
        disposition: "REPORT; flux and snapshot comparisons are TREND because they need a baseline.",
        member_predicates: "authored_age, changed_recently, snapshot_history_exists, snapshot_history_present; primitives include freshness, changed_within, git_mtime, flux.",
        common_joins: &[
            "`authored_age(h, days), *handle{id: h, file: file}` for date-backed age",
            "`changed_recently(h, band), *handle{id: h, file: file}` for coarse git-backed change recency",
            "`at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now` for observed status movement",
        ],
        examples: &[
            "? authored_age(h, days), *handle{id: h, file: file}.",
            "? changed_recently(h, band), *handle{id: h, file: file}.",
            "? axis_of(\"authored_age\", axis).",
        ],
        see_also: &["recent_frontier", "convergence", "runtime"],
    },
    AxisTopicCard {
        name: "relevance",
        summary: "Relevance asks whether a handle or span matches the current query.",
        question: "matches my query?",
        oracle: "text and query scored by the ranker/search provider.",
        disposition: "REPORT: relevance scores inform retrieval, not corpus validity.",
        member_predicates: "search and match are primitives; verb-local search/context rows project this axis into product surfaces.",
        common_joins: &[
            "`search(\"TERM\", h, span_id, score, reason, field, low_confidence), *handle{id: h, file: file}` for raw hit evidence",
            "`search{query: \"TERM\", handle: h, score: score}, *span{handle: h, id: span_id, summary: summary}` for span summaries",
            "`axis_of(p, \"composition\")` to inspect rankers that combine relevance with other axes",
        ],
        examples: &[
            "? search{query: \"convergence\", handle: h, score: score}.",
            "? axis(\"relevance\", question, oracle, disposition).",
        ],
        see_also: &["search", "context", "importance"],
    },
    AxisTopicCard {
        name: "importance",
        summary: "Importance asks how central a handle is in the graph.",
        question: "how central?",
        oracle: "degree, citations, impact, and neighborhood graph primitives over current edges.",
        disposition: "REPORT: centrality changes ranking and navigation, not validity.",
        member_predicates: "hub, incoming_edge, outgoing_edge, incident, orientation_inbound_count; primitives include cite_count, in_degree, out_degree, impact, neighborhood, upstream, downstream.",
        common_joins: &[
            "`incoming_edge(h, from, kind), *handle{id: h, file: file}` for inbound evidence",
            "`out_degree(h, degree), *handle{id: h, file: file}` for broad hubs",
            "`hub(h, degree), *handle{id: h, file: file}` to spot maps or over-broad index handles",
        ],
        examples: &[
            "? hub(h, degree), *handle{id: h, file: file}.",
            "? incoming_edge(h, from, kind), *handle{id: h, file: file}.",
            "? axis_of(\"hub\", axis).",
        ],
        see_also: &["structure", "relevance", "context"],
    },
    AxisTopicCard {
        name: "structure",
        summary: "Structure asks how corpus handles are organized and connected.",
        question: "organized or connected?",
        oracle: "stored edges plus adapter-provided areas, namespaces, sections, and pipeline structure.",
        disposition: "REPORT: structure orients navigation; diagnostics decide when a structural fact becomes a gate.",
        member_predicates: "area_of, namespace_of, handle_file, section_ref, area_health, area_frontier, parent_dir_* and namespace_* helpers, top_pair, orphan, stub.",
        common_joins: &[
            "`area_of(h, area), *handle{id: h, file: file}` to group handles by source area",
            "`namespace_of(h, namespace), *handle{id: h, kind: kind}` to inspect label families",
            "`section_ref_edge(edge_id), *edge{native_id: edge_id, from: src, to: dst, kind: kind}` for markdown section-reference evidence",
        ],
        examples: &[
            "? area_health(area, grade, files, errors, cross_edges).",
            "? namespace_of(h, namespace), *handle{id: h, kind: kind}.",
            "? axis_of(\"area_of\", axis).",
        ],
        see_also: &["importance", "diagnostic", "area_health"],
    },
    AxisTopicCard {
        name: "obligations",
        summary: "Obligations ask what has been promised and whether the corpus records a discharge.",
        question: "owed?",
        oracle: "obligation and discharge facts over handles.",
        disposition: "GATE-able through E002: undischarged obligations are allowed to block release or review gates.",
        member_predicates: "undischarged_obligation, multiple_discharge; primitives include obligation, discharged, undischarged, discharge_count.",
        common_joins: &[
            "`undischarged(h), obligation(h), *handle{id: h, file: file, status: status}` for owed work",
            "`multiple_discharge(h, file, count), diagnostic(\"I002\", severity, h, file, line, evidence)` for duplicate discharge evidence",
            "`axis_of(\"undischarged_obligation\", axis)` to inspect obligation-axis placement",
        ],
        examples: &[
            "? undischarged(h), obligation(h), *handle{id: h, file: file, status: status}.",
            "? multiple_discharge(h, file, count).",
            "? axis_of(\"undischarged_obligation\", axis).",
        ],
        see_also: &["convergence", "diagnostic", "check"],
    },
    AxisTopicCard {
        name: "dependency-validity",
        summary: "Dependency validity asks whether a terminal target is dead, remains valid to depend on, or is not yet classified.",
        question: "still valid to depend on?",
        oracle: "actual terminal target status against conservative builtin classifications plus per-status project overrides.",
        disposition: "PRE-FLIGHT for classified-dead targets through W001; unknown is suggestion-only through aggregate S006.",
        member_predicates: "dependency_dead_status, dependency_valid_status, dependency_status_classification; dependency_config_gap and stale_reference are diagnostic projections.",
        common_joins: &[
            "`dependency_status_classification(status, classification, origin)` to inspect the effective classification and its source",
            "`dependency_dead_status(status), *handle{id: target, status: status}, terminal(target)` to inspect terminal targets that can trigger W001",
            "`dependency_config_gap(status, count, variant), diagnostic{code: \"S006\", subject: status}` to inspect unknown terminal statuses",
        ],
        examples: &[
            "? dependency_status_classification(status, classification, origin).",
            "? dependency_config_gap(status, count, variant).",
            "? axis_of(\"dependency_dead_status\", axis).",
        ],
        see_also: &[
            "stale_reference",
            "W001",
            "dependency_config_gap",
            "S006",
            "lifecycle",
        ],
    },
    AxisTopicCard {
        name: "topic",
        summary: "Topic asks whether two files are likely on the same subject through shared discriminative citation targets.",
        question: "same subject?",
        oracle: "pairwise shared Cites targets after excluding curated inventory handles and mega-targets.",
        disposition: "REPORT: annotate possible topical relation; never assert an edge or hidden supersession.",
        member_predicates: "topic_citation_target, topic_target_citation_count, topic_mega_target_cap, topic_nondiscriminative_target, topic_shared_target, topic_pair, topic_sibling.",
        common_joins: &[
            "`topic_sibling(a, b, shared), *handle{id: a, file: left}, *handle{id: b, file: right}` to inspect same-subject file pairs",
            "`topic_nondiscriminative_target(t), topic_target_citation_count(t, n)` to see why broad targets are excluded",
            "`topic_pair(left, right, shared)` when you need canonical pair rows without symmetric duplicates",
        ],
        examples: &[
            "? topic_sibling(a, b, shared), shared >= 2.",
            "? topic_nondiscriminative_target(t), topic_target_citation_count(t, n).",
            "? axis_of(\"topic_sibling\", axis).",
        ],
        see_also: &["currency", "importance", "structure", "context"],
    },
];

/// Renders the named axis card from the canonical CR-D104 axis table.
pub(super) fn axis_topic_card(name: &str) -> Option<String> {
    let card = AXIS_TOPIC_CARDS.iter().find(|card| card.name == name)?;
    Some(describe_card(DescribeCard {
        summary: card.summary,
        kind: Some(DescribeKind::RuntimeTopic),
        relationship: Some(
            "Axis card from CR-D104: use `axis` for the machine question/oracle/disposition row and `axis_of` to place predicates.",
        ),
        common_joins: card.common_joins,
        examples: card.examples.to_vec(),
        see_also: card.see_also,
        extra_lines: vec![
            format!("Question: {}", card.question),
            format!("Oracle: {}", card.oracle),
            format!("Disposition: {}", card.disposition),
            format!("Member predicates: {}", card.member_predicates),
            "Placement categories outside axes: composition, diagnostic, infrastructure."
                .to_string(),
        ],
        ..DescribeCard::default()
    }))
}

/// Renders the convergence overview that binds the runtime vocabulary together.
pub(super) fn convergence_topic_card() -> String {
    describe_card(DescribeCard {
        summary: "Convergence is anneal's physics: corpus facts create energy, energy creates a frontier, agents do work, and snapshots show whether the landscape is flattening.",
        kind: Some(DescribeKind::RuntimeTopic),
        relationship: Some("This topic names the act as well as the vocabulary. Use `status` for a landing view, then compose `potential`, `frontier`, `blocker`, diagnostics, and `flow` in eval."),
        common_joins: &[
            "`potential(h, energy), primary_entropy(h, source)` to see why a handle has energy",
            "`frontier(h, energy), *handle{id: h, file: file, summary: summary}` for the global work frontier",
            "`blocker(h, energy, source), primary_entropy(h, source)` for one blocker reason per handle",
            "`flow(h, direction), *handle{id: h, status: status}` to inspect convergence flow",
        ],
        extra_lines: vec![
            "The Act: agents dissipate potential by editing corpus facts, then rerun status/check to verify energy moved.".to_string(),
            "Vocabulary: entropy is an unsettled signal; potential is weighted energy; frontier is the highest-energy projection; blocker is stalled energy; flow is advancing, holding, or drifting.".to_string(),
            "Flow: settled handles are outside flow by design; regressed(h) and re_opened(h) explain drifting(h) leaves.".to_string(),
            "Tuning: project rules can shadow `potential_weight(source, weight)` to retune convergence energy.".to_string(),
        ],
        requires: &["snapshot history for flow predicates that compare at(\"snapshot:last\") with the current graph."],
        see_also: &[
            "status",
            "potential",
            "frontier",
            "blocker",
            "flow",
            "potential_weight",
        ],
        examples: vec![
            "? frontier(h, energy), primary_entropy(h, source).",
            "? flow(h, direction), *handle{id: h, summary: summary}.",
            "? at(\"snapshot:last\") { *handle{id: h, status: old} }, *handle{id: h, status: now}, old != now.",
        ],
        ..DescribeCard::default()
    })
}

/// Canonical diagnostic-code vocabulary projected into `describe` and examples.
pub(super) const DIAGNOSTIC_CODE_CARDS: &[DiagnosticCodeCard] = &[
    DiagnosticCodeCard {
        code: "E001",
        severity: "error",
        summary: "Broken reference: a corpus edge points at a handle that does not exist.",
        rule: "broken_reference",
        evidence: r#"("broken_ref", target)"#,
        common_joins: &[
            "`diagnostic{code: \"E001\", subject: src}, broken_reference(src, target, file, line)` to inspect the missing target",
            "`broken_reference(src, target, file, line), read{handle: src, budget: 1200, text: text}` to read the source context",
        ],
        example: r#"? diagnostic{code: "E001", severity: severity, subject: src}."#,
        see_also: &["diagnostic", "broken_reference", "W004"],
    },
    DiagnosticCodeCard {
        code: "E002",
        severity: "error",
        summary: "Undischarged obligation: a live obligation handle has no Discharges edge.",
        rule: "undischarged_obligation",
        evidence: r#""undischarged""#,
        common_joins: &[
            "`diagnostic{code: \"E002\", subject: h}, undischarged_obligation(h, file)` to inspect open obligations",
            "`undischarged_obligation(h, file), area_of{h: h, area: area}` to group open obligations by area",
        ],
        example: r#"? diagnostic{code: "E002", subject: h, file: file}."#,
        see_also: &[
            "diagnostic",
            "undischarged_obligation",
            "undischarged",
            "I002",
        ],
    },
    DiagnosticCodeCard {
        code: "W001",
        severity: "warning",
        summary: "Stale reference: an active handle depends on a terminal target whose status is classified dead.",
        rule: "stale_reference",
        evidence: r#"("stale_ref", source_status, target_status)"#,
        common_joins: &[
            "`diagnostic{code: \"W001\", subject: src}, stale_reference(src, target, file, source_status, target_status)` to inspect the stale edge",
            "`stale_reference(src, target, file, source_status, target_status), *handle{id: target, summary: summary}` to add target context",
        ],
        example: r#"? diagnostic{code: "W001", subject: src, file: file}."#,
        see_also: &["diagnostic", "stale_reference", "W002"],
    },
    DiagnosticCodeCard {
        code: "W002",
        severity: "warning",
        summary: "Confidence gap: a dependency target is behind its source in the configured lifecycle order.",
        rule: "confidence_gap",
        evidence: r#"("confidence_gap", source_status, source_level, target_status, target_level)"#,
        common_joins: &[
            "`diagnostic{code: \"W002\", subject: src}, confidence_gap(src, target, file, source_status, source_level, target_status, target_level)` to inspect lifecycle levels",
            "`confidence_gap(src, target, file, source_status, source_level, target_status, target_level), area_of{h: src, area: area}` to group gaps by area",
        ],
        example: r#"? diagnostic{code: "W002", subject: src, evidence: evidence}."#,
        see_also: &[
            "diagnostic",
            "confidence_gap",
            "configured_pipeline_status",
            "W001",
        ],
    },
    DiagnosticCodeCard {
        code: "W003",
        severity: "warning",
        summary: "Missing frontmatter: a file lacks status frontmatter in a directory where frontmatter is otherwise established.",
        rule: "missing_frontmatter_file",
        evidence: "null",
        common_joins: &[
            "`diagnostic{code: \"W003\", subject: h}, missing_frontmatter_file(h, dir, file)` to inspect the missing metadata",
            "`missing_frontmatter_file(h, dir, file), area_of{h: h, area: area}` to group missing metadata by area",
        ],
        example: r#"? diagnostic{code: "W003", subject: h, file: file}."#,
        see_also: &["diagnostic", "missing_frontmatter_file", "W004"],
    },
    DiagnosticCodeCard {
        code: "W004",
        severity: "warning",
        summary: "Implausible reference: markdown extraction saw a reference-like token that was rejected as implausible.",
        rule: "implausible_ref",
        evidence: r#"("implausible_ref", value)"#,
        common_joins: &[
            "`diagnostic{code: \"W004\", subject: h}, implausible_ref(h, file, value)` to inspect rejected tokens",
            "`implausible_ref(h, file, value), read{handle: h, budget: 1200, text: text}` to read the local context",
        ],
        example: r#"? diagnostic{code: "W004", subject: h, evidence: evidence}."#,
        see_also: &["diagnostic", "implausible_ref", "E001", "W003"],
    },
    DiagnosticCodeCard {
        code: "W005",
        severity: "warning",
        summary: "Lifecycle config gap: a status appears in handles or ordering without an effective builtin or project classification, or the ordering cannot terminate.",
        rule: "lifecycle_config_gap",
        evidence: r#"("lifecycle_config_gap", status, count, variant)"#,
        common_joins: &[
            "`diagnostic{code: \"W005\", subject: status}, lifecycle_config_gap(status, count, variant)` to inspect lifecycle config drift",
            "`lifecycle_status_classification(status, classification, origin)` to inspect effective builtin and project classifications",
        ],
        example: r#"? diagnostic{code: "W005", subject: status, evidence: evidence}."#,
        see_also: &[
            "diagnostic",
            "lifecycle_config_gap",
            "lifecycle_status_classification",
            "configured_pipeline_status",
            "pipeline_stall",
        ],
    },
    DiagnosticCodeCard {
        code: "W006",
        severity: "warning",
        summary: "Spec-code drift: a spec that asserts current code cites a path that existed in HEAD history but is now missing on disk.",
        rule: "spec_code_drift",
        evidence: r#"("spec_code_drift", target_path, source_status)"#,
        common_joins: &[
            "`diagnostic{code: \"W006\", subject: src}, spec_code_drift(src, target_path, file, line, source_status)` to inspect the missing code target",
            "`spec_code_drift(src, target_path, file, line, source_status), read{handle: src, budget: 1200, text: text}` to read the live spec context",
            "`spec_code_drift(src, target_path, file, line, source_status), asserts_code(source_status)` to inspect the lifecycle gate",
            "`spec_code_drift(src, target_path, file, line, source_status), *edge{from: src, to: ref, kind: \"Cites\"}, *meta{handle: ref, key: \"target_history_status\", value: \"present\"}` to audit history evidence",
        ],
        example: r#"? diagnostic{code: "W006", subject: src, evidence: evidence}."#,
        see_also: &[
            "diagnostic",
            "spec_code_drift",
            "asserts_code",
            "target_exists",
            "target_history_status",
            "external_class",
        ],
    },
    DiagnosticCodeCard {
        code: "W007",
        severity: "warning",
        summary: "Frontmatter mapping gap: an exact reference-like markdown key appears on one or more handles but has no configured edge mapping.",
        rule: "frontmatter_mapping_gap",
        evidence: r#"("frontmatter_mapping_gap", key, distinct_handle_count, suggested_field, edge_kind, direction)"#,
        common_joins: &[
            "`diagnostic{code: \"W007\", subject: key}, frontmatter_mapping_gap(key, distinct_handle_count, suggested_field, edge_kind, direction)` to inspect the supported recovery mapping",
            "`frontmatter_mapping_gap(key, count, field, kind, direction), *meta{handle: h, key: key}, *meta{handle: h, key: \"md.parent_dir\"}` to list the markdown handles carrying that unmapped key",
        ],
        example: r#"? diagnostic{code: "W007", subject: key, evidence: evidence}."#,
        see_also: &[
            "diagnostic",
            "frontmatter_mapping_gap",
            "frontmatter_mapping_alias",
            "*meta",
            "*config",
            "W003",
            "W004",
        ],
    },
    DiagnosticCodeCard {
        code: "I001",
        severity: "info",
        summary: "Section references present: section-reference placeholders exist and are counted separately from broken handles.",
        rule: "section_ref_total",
        evidence: r#"("section_refs", count)"#,
        common_joins: &[
            "`diagnostic{code: \"I001\", evidence: evidence}` to see whether section references were counted",
            "`section_ref_total(count), diagnostic{code: \"I001\"}` to inspect the section-reference total",
        ],
        example: r#"? diagnostic{code: "I001", evidence: evidence}."#,
        see_also: &["diagnostic", "section_ref_total", "E001"],
    },
    DiagnosticCodeCard {
        code: "I002",
        severity: "info",
        summary: "Multiple discharges: a live obligation has more than one Discharges edge.",
        rule: "multiple_discharge",
        evidence: r#"("multiple_discharges", count)"#,
        common_joins: &[
            "`diagnostic{code: \"I002\", subject: h}, multiple_discharge(h, file, count)` to inspect redundant discharges",
            "`multiple_discharge(h, file, count), discharge_count(h, n)` to compare the reported count",
        ],
        example: r#"? diagnostic{code: "I002", subject: h, evidence: evidence}."#,
        see_also: &[
            "diagnostic",
            "multiple_discharge",
            "E002",
            "discharge_count",
        ],
    },
    DiagnosticCodeCard {
        code: "S001",
        severity: "suggestion",
        summary: "Orphaned handle: the subject is a label or version handle with no incoming references. The file is its declaring file, not a claim that the document is orphaned.",
        rule: "orphaned_handle",
        evidence: r#"("orphaned_handle", kind, h)"#,
        common_joins: &[
            "`diagnostic{code: \"S001\", subject: h}, *handle{id: h, kind: kind}` to inspect whether the orphaned subject is a label or version",
            "`orphaned_handle(h), *handle{id: h, namespace: namespace}` to group orphans by namespace",
        ],
        example: r#"? diagnostic{code: "S001", subject: h, file: file}."#,
        see_also: &["diagnostic", "orphaned_handle", "orphan", "S004"],
    },
    DiagnosticCodeCard {
        code: "S003",
        severity: "suggestion",
        summary: "Pipeline stall: snapshot history shows a lifecycle status accumulating without movement to the next configured status.",
        rule: "pipeline_stall",
        evidence: r#"("pipeline_stall", status, count, next_status, based_on_history)"#,
        common_joins: &[
            "`diagnostic{code: \"S003\", subject: status}, pipeline_stall(status, count, next_status, based_on_history)` to inspect the stalled status",
            "`snapshot_history_present(count), pipeline_stall(status, stalled, next_status, true)` to confirm automatic status snapshots have accrued",
        ],
        example: r#"? diagnostic{code: "S003", subject: status, evidence: evidence}."#,
        see_also: &[
            "diagnostic",
            "pipeline_stall",
            "advancing",
            "snapshot_history_present",
            "W005",
        ],
    },
    DiagnosticCodeCard {
        code: "S004",
        severity: "suggestion",
        summary: "Abandoned namespace: an active namespace's members are all terminal or stale.",
        rule: "abandoned_namespace",
        evidence: r#"("abandoned_namespace", namespace, total, terminal_count, stale_count)"#,
        common_joins: &[
            "`diagnostic{code: \"S004\", subject: namespace}, abandoned_namespace(namespace, total, terminal_count, stale_count)` to inspect the namespace",
            "`abandoned_namespace(namespace, total, terminal_count, stale_count), namespace_label(namespace, h)` to inspect members",
        ],
        example: r#"? diagnostic{code: "S004", subject: namespace, evidence: evidence}."#,
        see_also: &["diagnostic", "abandoned_namespace", "S001", "freshness"],
    },
    DiagnosticCodeCard {
        code: "S005",
        severity: "suggestion",
        summary: "Concern-group candidate: two label namespaces frequently co-occur and may deserve a configured concern group.",
        rule: "top_pair",
        evidence: r#"("concern_group_candidate", left_prefix, right_prefix, count)"#,
        common_joins: &[
            "`diagnostic{code: \"S005\", subject: left_prefix}, top_pair(left_prefix, right_prefix, count)` to inspect candidate concern groups",
            "`top_pair(left_prefix, right_prefix, count), same_concern_pair(left_prefix, right_prefix)` to test whether a concern already covers it",
        ],
        example: r#"? diagnostic{code: "S005", subject: left_prefix, evidence: evidence}."#,
        see_also: &["diagnostic", "top_pair", "*concern", "same_concern_pair"],
    },
    DiagnosticCodeCard {
        code: "S006",
        severity: "suggestion",
        summary: "Dependency config gap: a terminal status is not classified as dead or valid for dependency checks.",
        rule: "dependency_config_gap",
        evidence: r#"("dependency_config_gap", status, count, "terminal_status_unclassified")"#,
        common_joins: &[
            "`diagnostic{code: \"S006\", subject: status}, dependency_config_gap(status, count, variant)` to inspect each unclassified terminal status",
            "`dependency_status_classification(status, classification, origin)` to inspect effective builtin and project classifications",
        ],
        example: r#"? diagnostic{code: "S006", subject: status, evidence: evidence}."#,
        see_also: &[
            "diagnostic",
            "dependency_config_gap",
            "dependency_status_classification",
            "dependency-validity",
            "W001",
        ],
    },
];

/// The complete static runtime-topic projection consumed by the index builder.
pub(super) struct RuntimeTopicCatalog {
    pub(super) descriptions: BTreeSet<DescribeEntry>,
    pub(super) examples: BTreeSet<Tuple>,
}

/// Builds all hand-authored runtime and configuration topics from one authority.
pub(super) fn runtime_topic_catalog() -> RuntimeTopicCatalog {
    let mut descriptions = BTreeSet::new();
    let mut examples = BTreeSet::new();
    descriptions.insert(describe_entry(
            "runtime",
            DescribeKind::RuntimeTopic,
            &render::describe_card(render::DescribeCard {
                summary: "Query stored corpus facts, compose graph/lifecycle/content/search primitives, load Datalog rules, and discover the available model.",
                kind: Some(DescribeKind::RuntimeTopic),
                extra_lines: vec![
                    "Visible commands: status, context, search, read, handle, schema, describe, eval, init.".to_string(),
                    "Hidden support commands: check, prime.".to_string(),
                    "Agent briefing: anneal help agent (or hidden alias anneal prime).".to_string(),
                    "Use schema for the callable catalog, describe NAME for examples and joins, and eval/-e for composition.".to_string(),
                    "Dimensional map: axis(name, question, oracle, disposition) lists runtime axes, axis_of(predicate, axis) places vocabulary, and describe <axis> opens the teaching card for currency, lifecycle, dependency-validity, recency, relevance, importance, convergence, structure, obligations, or topic.".to_string(),
                    "Schema discovery is interactive: unknown predicate or field errors include nearby names and allowed fields.".to_string(),
                    "Observed vocabulary recipes: query *handle.status, *edge.kind, *handle.namespace, or *meta.key directly.".to_string(),
                    "Orientation predicates:".to_string(),
                    "  - recent_frontier(h, rank, recency) ranks goal-less reading candidates: date-backed authored age first, coarse git change-recency only for undated files.".to_string(),
                    "  - anchor(h, score, why) is the uncapped durable-spine relation.".to_string(),
                    "  - ranked_anchor(h, rank, score, why) is the rank projection used by status pointers.".to_string(),
                    "Cold-start ladder: status for aggregate vital signs, recent_frontier/ranked_anchor for goal-less reading, context GOAL for focused retrieval.".to_string(),
                    "Recent-change recipes: join *handle.file to git_mtime(file, instant), or use changed_within(h, days); these are git-backed change signals, not authored age.".to_string(),
                    "History concepts:".to_string(),
                    "  - snapshots capture graph state over time for at(\"snapshot:last\") queries.".to_string(),
                    "  - generations mark source refresh epochs for atomic fact replacement.".to_string(),
                    "  - trails record per-query provenance and surfaced/consumed references.".to_string(),
                ],
                examples: vec![
                    "? schema(name, kind, signature, determinism, provenance).",
                    "? describe(\"search\", doc).",
                    "? describe(\"convergence\", doc).",
                    "? axis(name, question, oracle, disposition).",
                    "? axis_of(\"currency_suspect\", axis).",
                    "? describe(\"topic\", doc).",
                    "? examples(\"search\", example).",
                    "? *handle{status: status}, status != null.",
                    "? *edge{kind: kind}.",
                    "? *handle{id: h, file: file}, git_mtime(file, instant).",
                    "? changed_within(h, 7), *handle{id: h, kind: \"file\", summary: summary}.",
                    "? recent_frontier(h, rank, recency), *handle{id: h, file: file} order by rank asc.",
                    "? ranked_anchor(h, rank, score, why), *handle{id: h, file: file} order by rank asc.",
                    "? flow(h, direction), *handle{id: h, summary: summary}.",
                ],
                ..render::DescribeCard::default()
            }),
        ));
    descriptions.insert(describe_entry(
            "check",
            DescribeKind::RuntimeTopic,
            &render::describe_card(render::DescribeCard {
                summary: "Hidden CI-gate alias for the error-only diagnostic view.",
                kind: Some(DescribeKind::RuntimeTopic),
                relationship: Some("Use eval for agent workflows; `anneal check` remains callable for CI and pre-commit gates, exits 1 when any error row exists, and is intentionally hidden from the default command surface."),
                common_joins: &[
                    "`diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}` mirrors the rows checked by the hidden CI gate",
                    "`diagnostic(code, severity, subject, file, line, evidence)` for the full diagnostic stream",
                ],
                extra_lines: vec![
                    "Canonical eval: anneal -e '? diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}.'".to_string(),
                    "Exit code: `anneal check` returns 1 when error-severity diagnostics exist, 0 otherwise.".to_string(),
                    "Deprecation: hidden alias retained for CI muscle memory; prefer eval composition in agent-facing workflows.".to_string(),
                ],
                see_also: &["diagnostic", "status", "help eval"],
                examples: vec![
                    "? diagnostic{code: code, severity: \"error\", subject: h, file: file, line: line}.",
                ],
                ..render::DescribeCard::default()
            }),
        ));
    examples.insert(Tuple(vec![
        string_value("runtime"),
        string_value(r#"? describe("runtime", doc)."#),
    ]));
    examples.insert(Tuple(vec![
        string_value("runtime"),
        string_value("? *handle{namespace: ns}, ns != \"\"."),
    ]));
    descriptions.insert(describe_entry(
            "code_path_root",
            DescribeKind::RuntimeTopic,
            &render::describe_card(render::DescribeCard {
                summary: "Project config section for extra in-repo code-reference roots scanned from markdown bodies.",
                kind: Some(DescribeKind::RuntimeTopic),
                signature: Some("config code_path_root { root([...]). }"),
                extra_lines: vec![
                    "Defaults already recognize crates/, lib/, src/, app/, test/, priv/, and native/.".to_string(),
                    "Build-output roots _build/, target/, and node_modules/ are always ignored.".to_string(),
                    "Recognized refs become external handles with external_class=\"code\" and ordinary Cites edges.".to_string(),
                ],
                common_joins: &[
                    "`*config{key: \"code_path_root.root\", value: root}` to inspect configured extra roots",
                    "`*meta{handle: h, key: \"external_class\", value: \"code\"}, *meta{handle: h, key: \"target_path\", value: path}` to inspect captured code refs",
                ],
                examples: vec![
                    "? *config{key: \"code_path_root.root\", value: root}.",
                    "? *meta{handle: h, key: \"external_class\", value: \"code\"}, *meta{handle: h, key: \"target_path\", value: path}.",
                ],
                ..render::DescribeCard::default()
            }),
        ));
    examples.insert(Tuple(vec![
        string_value("code_path_root"),
        string_value(r#"? *config{key: "code_path_root.root", value: root}."#),
    ]));
    examples.insert(Tuple(vec![
        string_value("asserts_code"),
        string_value("? asserts_code(status)."),
    ]));
    descriptions.insert(describe_entry(
            "search_boost",
            DescribeKind::RuntimeTopic,
            &render::describe_card(render::DescribeCard {
                summary: "Project config section for tuning search ranking boosts by lifecycle status and hub degree.",
                kind: Some(DescribeKind::RuntimeTopic),
                signature: Some("config search_boost { status(\"status\", boost). hub(boost). }"),
                extra_lines: vec![
                    "Defaults boost authoritative/current/stable handles above active/review handles, and active/review handles above draft/raw handles.".to_string(),
                    "The hub boost is a bounded per-incoming-edge score bump; set hub(0) to disable it for one corpus.".to_string(),
                    "Boosts are additive score calibration, not filters: low-confidence filtering still happens after ranking.".to_string(),
                ],
                common_joins: &[
                    "`*config{key: \"search_boost.status.authoritative\", value: boost}` to inspect a status override",
                    "`*config{key: \"search_boost.hub\", value: boost}` to inspect the hub-edge override",
                    "`search{query: \"text\", handle: h, score: score}, *handle{id: h, status: status}` to see boosted statuses in ranked rows",
                ],
                examples: vec![
                    "? *config{key: \"search_boost.status.authoritative\", value: boost}.",
                    "? search{query: \"conformance\", handle: h, score: score}, *handle{id: h, status: status}.",
                ],
                see_also: &["search", "context", "schema"],
                ..render::DescribeCard::default()
            }),
        ));
    examples.insert(Tuple(vec![
        string_value("search_boost"),
        string_value(r#"? *config{key: "search_boost.status.authoritative", value: boost}."#),
    ]));
    descriptions.insert(describe_entry(
            "external",
            DescribeKind::RuntimeTopic,
            &render::describe_card(render::DescribeCard {
                summary: "External handles mark references outside the markdown corpus boundary, including URLs, other repos, and in-repo code paths.",
                kind: Some(DescribeKind::RuntimeTopic),
                signature: Some(r#"*handle{kind: "external"} plus optional external_class metadata"#),
                extra_lines: vec![
                    "Document-like external refs use the same substrate kind as code refs so the graph stays small and composable.".to_string(),
                    "In-repo code refs carry standard metadata: external_class=\"code\", target_path, target_start_line, and target_end_line.".to_string(),
                    "Future code adapters can promote code refs into first-class code handles without changing today's Cites edges.".to_string(),
                ],
                common_joins: &[
                    "`*handle{id: h, kind: \"external\"}, *edge{to: h, from: src, kind: \"Cites\"}` to find who cites an external target",
                    "`*meta{handle: h, key: \"external_class\", value: \"code\"}, *meta{handle: h, key: \"target_path\", value: path}` to keep code refs only",
                ],
                examples: vec![
                    "? *handle{id: h, kind: \"external\"}.",
                    "? *meta{handle: h, key: \"external_class\", value: \"code\"}, *meta{handle: h, key: \"target_path\", value: path}.",
                ],
                see_also: &["external_class", "target_path", "handle", "code_path_root", "*handle", "*meta"],
                ..render::DescribeCard::default()
            }),
        ));
    examples.insert(Tuple(vec![
        string_value("external"),
        string_value(r#"? *handle{id: h, kind: "external"}."#),
    ]));
    descriptions.insert(describe_entry(
            "external_class",
            DescribeKind::RuntimeTopic,
            &render::describe_card(render::DescribeCard {
                summary: r#"Discriminator for *handle{kind: "external"} sub-classes."#,
                kind: Some(DescribeKind::RuntimeTopic),
                signature: Some(r#"*meta{handle: h, key: "external_class", value: class}"#),
                extra_lines: vec![
                    "Known values (standard, adapter-neutral):".to_string(),
                    r#"- "code": target_path, target_start_line, target_end_line, target_exists, and target_history_status describe source-code locations."#.to_string(),
                    r#"- Future "url": target_url."#.to_string(),
                    r#"- Future "issue": target_repo and target_number."#.to_string(),
                    "A new external_class value is an anneal standard-key decision.".to_string(),
                    "Sources may emit additional source-specific discriminators in their own namespace, such as md.link_type.".to_string(),
                ],
                common_joins: &[
                    r#"`*handle{id: h, kind: "external"}, *meta{handle: h, key: "external_class", value: "code"}` to find all code-target external handles"#,
                    r#"`*meta{handle: h, key: "external_class", value: "code"}, *meta{handle: h, key: "target_path", value: path}` to add the code location"#,
                ],
                examples: vec![
                    r#"? *handle{id: h, kind: "external"}, *meta{handle: h, key: "external_class", value: "code"}."#,
                    r#"? *meta{handle: h, key: "external_class", value: "code"}, *meta{handle: h, key: "target_path", value: path}."#,
                ],
                see_also: &["*meta", "*handle", "target_path"],
                ..render::DescribeCard::default()
            }),
        ));
    examples.insert(Tuple(vec![
        string_value("external_class"),
        string_value(r#"? *meta{handle: h, key: "external_class", value: "code"}."#),
    ]));
    for (name, summary, detail) in [
        (
            "target_path",
            r"Standard metadata key for the path an external handle points at.",
            r#"For external_class="code", this is the in-repo source path without a line range."#,
        ),
        (
            "target_start_line",
            r"Standard metadata key for the first target line an external handle points at.",
            r#"For external_class="code", this is the first line in the code location when a range was present."#,
        ),
        (
            "target_end_line",
            r"Standard metadata key for the last target line an external handle points at.",
            r#"For external_class="code", this is the inclusive end line when a range was present."#,
        ),
        (
            "target_exists",
            r"Standard metadata key for whether an external handle's target exists or confidently drifted.",
            r#"For external_class="code", this is true when present on disk, false when absent but present in HEAD history, and unknown when history cannot prove drift."#,
        ),
        (
            "target_history_status",
            r"Standard metadata key for whether a code target appears in HEAD history.",
            r#"For external_class="code", values are present, absent, or unavailable. target_exists=false is evidence-backed drift only when history status is present."#,
        ),
        (
            "target_probe_base",
            r"Standard metadata key for the base directory used to probe target existence.",
            r#"For external_class="code", this records the repository, workspace, or corpus root used to resolve target_path."#,
        ),
        (
            "target_resolved_path",
            r"Standard metadata key for the resolved on-disk target when one was found.",
            r#"For external_class="code", this records the path that made target_exists true."#,
        ),
    ] {
        let signature = format!(r#"*meta{{handle: h, key: "{name}", value: value}}"#);
        let example = format!(r#"? *meta{{handle: h, key: "{name}", value: value}}."#);
        let common_join = format!(
            r#"`*meta{{handle: h, key: "external_class", value: "code"}}, *meta{{handle: h, key: "{name}", value: value}}` to inspect code target metadata"#
        );
        descriptions.insert(describe_entry(
                name,
                DescribeKind::RuntimeTopic,
                &render::describe_card(render::DescribeCard {
                    summary,
                    kind: Some(DescribeKind::RuntimeTopic),
                    signature: Some(signature.as_str()),
                    extra_lines: vec![
                        detail.to_string(),
                        "The key is standard: anneal defines it and it has the same meaning on any corpus.".to_string(),
                    ],
                    common_joins: &[common_join.as_str()],
                    examples: vec![example.as_str()],
                    see_also: &["external_class", "*meta", "external"],
                    ..render::DescribeCard::default()
                }),
            ));
        examples.insert(Tuple(vec![string_value(name), string_value(&example)]));
    }
    RuntimeTopicCatalog {
        descriptions,
        examples,
    }
}
