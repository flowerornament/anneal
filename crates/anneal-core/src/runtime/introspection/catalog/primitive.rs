//! Engine-primitive signatures, relationships, requirements, and examples.

use super::super::super::primitives::PrimitivePredicate;

/// Returns the determinism claim exposed in the primitive's schema row.
pub(in crate::runtime::introspection) fn primitive_determinism(
    primitive: PrimitivePredicate,
) -> &'static str {
    match primitive {
        PrimitivePredicate::Search => "ranker-dependent deterministic",
        _ => "deterministic",
    }
}

/// Returns the canonical one-line purpose for an engine primitive.
pub(in crate::runtime::introspection) fn primitive_doc(
    primitive: PrimitivePredicate,
) -> &'static str {
    match primitive {
        PrimitivePredicate::Upstream => {
            "Find handles that the starting handle depends on, following incoming dependency-style edges through the graph."
        }
        PrimitivePredicate::Downstream => {
            "Find handles that depend on the starting handle, following outgoing dependency-style edges through the graph."
        }
        PrimitivePredicate::Impact => {
            "Find graph nodes that could be affected if a handle changes, with the number of hops from the starting handle."
        }
        PrimitivePredicate::Neighborhood => {
            "Find handles near a starting handle within a bounded number of graph hops."
        }
        PrimitivePredicate::Terminal => {
            "Return handles whose status means the work is done, archived, rejected, or otherwise no longer active."
        }
        PrimitivePredicate::Active => {
            "Return handles whose status means an agent may still need to read, change, or resolve them."
        }
        PrimitivePredicate::Settled => {
            "Return handles that the corpus considers resolved enough to use as stable context."
        }
        PrimitivePredicate::LifecycleStatusClassification => {
            "Return each effectively modeled lifecycle status classification and whether project config or a builtin supplied it."
        }
        PrimitivePredicate::PipelinePosition => {
            "Return the numeric order for a handle's status, so queries can compare whether one status is ahead of another."
        }
        PrimitivePredicate::PipelinePositionFor => {
            "Return the numeric order for a status value, so queries can compare lifecycle progress."
        }
        PrimitivePredicate::Obligation => {
            "Return labels in namespaces that the project treats as obligations: promises, questions, requirements, or tasks that must be discharged."
        }
        PrimitivePredicate::Discharged => {
            "Return handles that already have at least one incoming Discharges edge."
        }
        PrimitivePredicate::Undischarged => {
            "Return obligations that still need a Discharges edge and are not terminal."
        }
        PrimitivePredicate::CiteCount => "Count incoming Cites edges for each handle.",
        PrimitivePredicate::InDegree => "Count all incoming graph edges for each handle.",
        PrimitivePredicate::OutDegree => "Count all outgoing graph edges for each handle.",
        PrimitivePredicate::DischargeCount => "Count incoming Discharges edges for each handle.",
        PrimitivePredicate::Freshness => {
            "Return how many days have passed since a handle's dated observation at the active time reference."
        }
        PrimitivePredicate::Flux => {
            "Count status changes for a handle over a recent day window, using snapshot history."
        }
        PrimitivePredicate::GitMtime => {
            "Return the latest git commit timestamp observed for a tracked corpus file."
        }
        PrimitivePredicate::ChangedWithin => {
            "Return handles whose backing file changed within a bound number of days according to git history."
        }
        PrimitivePredicate::TokenEstimate => {
            "Return the estimated number of stored content tokens for a handle."
        }
        PrimitivePredicate::Search => {
            "Search handle identities, metadata, headings, and content text, returning ranked span-granular hits with heading ids, reasons, and calibrated scores."
        }
        PrimitivePredicate::Read => {
            "Read content spans for one handle, optionally narrowed to the exact span_id returned by search or context."
        }
        PrimitivePredicate::ReadFull => {
            "Read all stored content for one handle. This bypasses the normal budget guard and requires the read_full capability."
        }
        PrimitivePredicate::Match => {
            "Run a regular expression against stored content for one already-bound handle and return matching lines."
        }
        PrimitivePredicate::Schema => {
            "List queryable stored relations, derived predicates, and engine primitives. The signature column is both the positional argument order and the accepted named-call parameter set."
        }
        PrimitivePredicate::Predicates => {
            "List rule-defined predicates with documentation and source locations."
        }
        PrimitivePredicate::Verbs => {
            "List declared verbs with query, documentation, and output schema."
        }
        PrimitivePredicate::Describe => {
            "Return documentation for a relation, predicate, primitive, verb, source, or runtime topic."
        }
        PrimitivePredicate::SourceOf => {
            "Return source file and line information for queryable runtime names."
        }
        PrimitivePredicate::Examples => "Return worked query examples for runtime names.",
        PrimitivePredicate::Sources => {
            "List linked adapters with recognition patterns, capabilities, and documentation."
        }
    }
}

/// Names runtime capabilities required before a primitive can answer honestly.
pub(in crate::runtime::introspection) fn primitive_requires(
    primitive: PrimitivePredicate,
) -> &'static [&'static str] {
    match primitive {
        PrimitivePredicate::Obligation | PrimitivePredicate::Undischarged => &[
            "`config handles { linear([...]). }` in anneal.dl. Without a linear namespace policy, no labels become obligations.",
        ],
        PrimitivePredicate::Flux => {
            &["snapshot history. On a corpus with no snapshots, status-change counts are zero."]
        }
        PrimitivePredicate::GitMtime | PrimitivePredicate::ChangedWithin => &[
            "git metadata supplied by the runtime host. Untracked files and non-git corpora produce no rows.",
        ],
        PrimitivePredicate::ReadFull => &[
            "the read_full runtime capability. Prefer read(handle, budget, ...) unless the full file is intentional.",
        ],
        PrimitivePredicate::Match => &[
            "the handle argument must already be bound; match does not scan the whole corpus by itself.",
        ],
        PrimitivePredicate::Upstream
        | PrimitivePredicate::Downstream
        | PrimitivePredicate::Impact
        | PrimitivePredicate::Neighborhood
        | PrimitivePredicate::Terminal
        | PrimitivePredicate::Active
        | PrimitivePredicate::Settled
        | PrimitivePredicate::LifecycleStatusClassification
        | PrimitivePredicate::PipelinePosition
        | PrimitivePredicate::PipelinePositionFor
        | PrimitivePredicate::Discharged
        | PrimitivePredicate::CiteCount
        | PrimitivePredicate::InDegree
        | PrimitivePredicate::OutDegree
        | PrimitivePredicate::DischargeCount
        | PrimitivePredicate::Freshness
        | PrimitivePredicate::TokenEstimate
        | PrimitivePredicate::Search
        | PrimitivePredicate::Read
        | PrimitivePredicate::Schema
        | PrimitivePredicate::Predicates
        | PrimitivePredicate::Verbs
        | PrimitivePredicate::Describe
        | PrimitivePredicate::SourceOf
        | PrimitivePredicate::Examples
        | PrimitivePredicate::Sources => &[],
    }
}

/// Places a primitive relative to adjacent runtime concepts.
pub(in crate::runtime::introspection) fn primitive_relationship(
    primitive: PrimitivePredicate,
) -> Option<&'static str> {
    match primitive {
        PrimitivePredicate::Search => Some(
            "The `search` verb wraps this primitive with TopK ranking, filters out low-confidence hits by default, and joins span hits to heading-path metadata. Scores include lexical strength plus configured status and hub boosts.",
        ),
        PrimitivePredicate::Read => Some(
            "The `read` verb wraps this primitive with typed CLI arguments for handle, budget, and targeted span reads; use a search hit's span_id to read the matched section body.",
        ),
        PrimitivePredicate::ChangedWithin => Some(
            "Lower-authority change-recency primitive over git file mtimes. Join `*handle{kind: \"file\"}` when you want one row per changed file; use `authored_age` when you need date-backed age.",
        ),
        PrimitivePredicate::GitMtime => Some(
            "Raw git timestamp primitive used by `changed_within`; compose it directly when you need exact commit times. Bulk commits can make this a degraded change oracle, so it is not authored age.",
        ),
        PrimitivePredicate::Schema => Some("The `schema` verb projects this primitive directly."),
        PrimitivePredicate::Verbs => {
            Some("Use `schema` for the verb catalog and `describe NAME` for a verb teaching card.")
        }
        PrimitivePredicate::Describe => {
            Some("The `describe` verb projects this primitive as teaching cards.")
        }
        PrimitivePredicate::SourceOf => {
            Some("The `source-of` verb projects this primitive directly.")
        }
        PrimitivePredicate::Examples => Some("`describe NAME` shows these examples inline."),
        PrimitivePredicate::Sources => Some(
            "Query this primitive directly with `anneal -e '? sources(name, recognizes, capabilities, doc).'`.",
        ),
        _ => None,
    }
}

/// Returns adjacent vocabulary for primitive drill-down.
pub(in crate::runtime::introspection) fn primitive_see_also(
    primitive: PrimitivePredicate,
) -> &'static [&'static str] {
    match primitive {
        PrimitivePredicate::Search => &["search", "context", "read", "describe"],
        PrimitivePredicate::Read | PrimitivePredicate::ReadFull => &["read", "*content", "*span"],
        PrimitivePredicate::Schema => &["describe", "examples"],
        PrimitivePredicate::Describe => &["schema", "examples"],
        PrimitivePredicate::Examples => &["describe", "schema"],
        PrimitivePredicate::GitMtime | PrimitivePredicate::ChangedWithin => {
            &["*handle", "freshness"]
        }
        PrimitivePredicate::Upstream
        | PrimitivePredicate::Downstream
        | PrimitivePredicate::Impact => {
            &["incoming_edge", "outgoing_edge", "neighborhood", "*edge"]
        }
        PrimitivePredicate::Obligation
        | PrimitivePredicate::Discharged
        | PrimitivePredicate::Undischarged
        | PrimitivePredicate::DischargeCount => &["diagnostic", "blocked", "*config"],
        _ => &[],
    }
}

/// Returns the canonical executable example for an engine primitive.
pub(in crate::runtime::introspection) fn primitive_example(
    primitive: PrimitivePredicate,
) -> Option<&'static str> {
    match primitive {
        PrimitivePredicate::Obligation => Some("? obligation(h)."),
        PrimitivePredicate::Discharged => Some("? discharged(h)."),
        PrimitivePredicate::Undischarged => Some("? undischarged(h)."),
        PrimitivePredicate::DischargeCount => Some("? discharge_count(h, n)."),
        PrimitivePredicate::Upstream => {
            Some(r#"? upstream("docs/runtime-overview.md", ancestor)."#)
        }
        PrimitivePredicate::Downstream => {
            Some(r#"? downstream("docs/runtime-overview.md", dependent)."#)
        }
        PrimitivePredicate::Neighborhood => {
            Some(r#"? neighborhood("docs/runtime-overview.md", 1, member)."#)
        }
        PrimitivePredicate::Impact => {
            Some(r#"? impact("docs/runtime-overview.md", affected, depth)."#)
        }
        PrimitivePredicate::Search => {
            Some(r#"? search("runtime overview", h, span_id, score, reason, field, low)."#)
        }
        PrimitivePredicate::Read => {
            Some(r#"? read("docs/runtime-overview.md", 4000, span, text, start, end, tokens)."#)
        }
        PrimitivePredicate::Schema => {
            Some("? schema(name, kind, signature, determinism, provenance).")
        }
        PrimitivePredicate::Describe => Some(r#"? describe("runtime", doc)."#),
        PrimitivePredicate::Sources => Some("? sources(name, recognizes, capabilities, doc)."),
        PrimitivePredicate::SourceOf => Some(r#"? source_of("search", file, lines)."#),
        PrimitivePredicate::Predicates => Some("? predicates(name, doc, file, lines)."),
        PrimitivePredicate::Verbs => Some("? verbs(name, query, doc, output_schema)."),
        PrimitivePredicate::Examples => Some(r#"? examples("search", example)."#),
        PrimitivePredicate::GitMtime => Some("? git_mtime(file, instant)."),
        PrimitivePredicate::ChangedWithin => Some("? changed_within(h, 7)."),
        PrimitivePredicate::Terminal
        | PrimitivePredicate::Active
        | PrimitivePredicate::Settled
        | PrimitivePredicate::PipelinePosition
        | PrimitivePredicate::PipelinePositionFor
        | PrimitivePredicate::CiteCount
        | PrimitivePredicate::InDegree
        | PrimitivePredicate::OutDegree
        | PrimitivePredicate::Freshness
        | PrimitivePredicate::Flux
        | PrimitivePredicate::TokenEstimate
        | PrimitivePredicate::ReadFull
        | PrimitivePredicate::Match => None,
        PrimitivePredicate::LifecycleStatusClassification => {
            Some("? lifecycle_status_classification(status, classification, origin).")
        }
    }
}
