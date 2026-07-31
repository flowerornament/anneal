//! Runtime schema and predicate introspection.

use crate::facts::STORED_RELATION_DESCRIPTORS;
use crate::source::SourceInfo;
use crate::trail::TRAIL_RELATION_DESCRIPTORS;

use super::analysis::{AnalyzedProgram, AnalyzedQuery};
use super::eval::{Tuple, Value};
use super::primitives::PrimitivePredicate;

mod builder;
mod catalog;
mod program;
mod projection;
mod render;
mod source;
mod topics;

use builder::IntrospectionBuilder;
use render::{DescribeCard, describe_card};
use source::{source_capability_names, source_tuple};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DescribeKind {
    RuntimeTopic,
    SourceAdapter,
    StoredRelation,
    EnginePrimitive,
    DerivedPredicate,
    Verb,
}

impl DescribeKind {
    const fn label(self) -> &'static str {
        match self {
            Self::RuntimeTopic => "runtime topic",
            Self::SourceAdapter => "source adapter",
            Self::StoredRelation => "stored relation",
            Self::EnginePrimitive => "engine primitive",
            Self::DerivedPredicate => "derived predicate",
            Self::Verb => "verb",
        }
    }

    const fn rank(self) -> u8 {
        match self {
            Self::Verb => 0,
            Self::DerivedPredicate => 1,
            Self::EnginePrimitive => 2,
            Self::StoredRelation => 3,
            Self::RuntimeTopic => 4,
            Self::SourceAdapter => 5,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DescribeEntry {
    rank: u8,
    name: String,
    tuple: Tuple,
}

impl DescribeEntry {
    fn matches_constraints(&self, constraints: &[(usize, Value)]) -> bool {
        self.tuple.matches_constraints(constraints)
    }
}

fn describe_entry(name: &str, kind: DescribeKind, doc: &str) -> DescribeEntry {
    DescribeEntry {
        rank: kind.rank(),
        name: name.to_string(),
        tuple: Tuple(vec![string_value(name), string_value(doc)]),
    }
}

#[derive(Clone, Debug, Default)]
/// Queryable schema, documentation, examples, and source locations for one runtime view.
pub(crate) struct IntrospectionIndex {
    source_descriptions: Vec<DescribeEntry>,
    source_rows: Vec<Tuple>,
    program: ProgramIntrospection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A dynamically declared stored relation that must join the static runtime catalog.
pub(crate) struct StoredRelationSummary {
    pub(crate) name: String,
    pub(crate) fields: Vec<String>,
}

impl IntrospectionIndex {
    /// Builds the source-adapter portion shared by every analyzed program and query.
    pub(crate) fn from_sources(sources: Vec<SourceInfo>) -> Self {
        let mut sources = sources;
        sources.sort_by(|left, right| left.name.cmp(right.name));
        let source_descriptions = sources
            .iter()
            .map(|source| {
                let recognizes = source
                    .recognizes
                    .iter()
                    .map(|pattern| pattern.0.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let capabilities =
                    source_capability_names(&source.capabilities, source.search.is_some())
                        .collect::<Vec<_>>()
                        .join(", ");
                describe_entry(
                    source.name,
                    DescribeKind::SourceAdapter,
                    &describe_card(DescribeCard {
                        summary: source.doc,
                        kind: Some(DescribeKind::SourceAdapter),
                        extra_lines: vec![
                            format!("Recognizes: {recognizes}."),
                            format!("Capabilities: [{capabilities}]."),
                        ],
                        ..DescribeCard::default()
                    }),
                )
            })
            .collect();
        let source_rows = sources.iter().map(source_tuple).collect();
        Self {
            source_descriptions,
            source_rows,
            program: ProgramIntrospection::default(),
        }
    }

    /// Projects one analyzed program without mutating the source-adapter baseline.
    pub(crate) fn for_program(
        &self,
        program: &AnalyzedProgram,
        dynamic_stored: Vec<StoredRelationSummary>,
    ) -> Self {
        Self {
            source_descriptions: self.source_descriptions.clone(),
            source_rows: self.source_rows.clone(),
            program: ProgramIntrospection::from_program(program, dynamic_stored),
        }
    }

    /// Adds query-local predicates to a derived view without leaking them into later queries.
    pub(crate) fn for_query(&self, query: &AnalyzedQuery) -> Self {
        Self {
            source_descriptions: self.source_descriptions.clone(),
            source_rows: self.source_rows.clone(),
            program: self.program.with_query(query),
        }
    }

    /// Evaluates one introspection primitive against the index's canonical tuple sets.
    pub(crate) fn tuples(
        &self,
        primitive: PrimitivePredicate,
        constraints: &[(usize, Value)],
    ) -> Vec<Tuple> {
        match primitive {
            PrimitivePredicate::Schema => matching_tuples(&self.program.schema, constraints),
            PrimitivePredicate::Predicates => {
                matching_tuples(&self.program.predicates, constraints)
            }
            PrimitivePredicate::Verbs => matching_tuples(&self.program.verbs, constraints),
            PrimitivePredicate::Describe => self.describe_tuples(constraints),
            PrimitivePredicate::SourceOf => matching_tuples(&self.program.source_of, constraints),
            PrimitivePredicate::Examples => matching_tuples(&self.program.examples, constraints),
            PrimitivePredicate::Sources => matching_tuples(&self.source_rows, constraints),
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
            | PrimitivePredicate::Obligation
            | PrimitivePredicate::Discharged
            | PrimitivePredicate::Undischarged
            | PrimitivePredicate::CiteCount
            | PrimitivePredicate::InDegree
            | PrimitivePredicate::OutDegree
            | PrimitivePredicate::DischargeCount
            | PrimitivePredicate::Freshness
            | PrimitivePredicate::Flux
            | PrimitivePredicate::GitMtime
            | PrimitivePredicate::ChangedWithin
            | PrimitivePredicate::TokenEstimate
            | PrimitivePredicate::Search
            | PrimitivePredicate::Read
            | PrimitivePredicate::ReadFull
            | PrimitivePredicate::Match => Vec::new(),
        }
    }

    fn describe_tuples(&self, constraints: &[(usize, Value)]) -> Vec<Tuple> {
        let mut entries = self
            .program
            .describe
            .iter()
            .chain(&self.source_descriptions)
            .filter(|entry| entry.matches_constraints(constraints))
            .cloned()
            .collect::<Vec<_>>();
        entries.sort();
        entries.into_iter().map(|entry| entry.tuple).collect()
    }
}

/// Returns whether a stored relation belongs to anneal's static schema authority.
pub(crate) fn is_static_stored_relation(name: &str) -> bool {
    STORED_RELATION_DESCRIPTORS
        .iter()
        .chain(TRAIL_RELATION_DESCRIPTORS)
        .any(|relation| relation.name == name)
}

#[derive(Clone, Debug, Default)]
struct ProgramIntrospection {
    schema: Vec<Tuple>,
    predicates: Vec<Tuple>,
    verbs: Vec<Tuple>,
    describe: Vec<DescribeEntry>,
    source_of: Vec<Tuple>,
    examples: Vec<Tuple>,
}

impl ProgramIntrospection {
    fn from_program(program: &AnalyzedProgram, dynamic_stored: Vec<StoredRelationSummary>) -> Self {
        let mut builder = IntrospectionBuilder::default();
        builder.add_runtime_overview();
        builder.add_stored_relations(dynamic_stored);
        builder.add_primitives();
        builder.add_program(program.program());
        builder.add_diagnostic_codes();
        builder.finish()
    }

    fn with_query(&self, query: &AnalyzedQuery) -> Self {
        let mut builder = IntrospectionBuilder::from_existing(self);
        builder.add_query(query);
        builder.finish()
    }
}

fn matching_tuples(tuples: &[Tuple], constraints: &[(usize, Value)]) -> Vec<Tuple> {
    tuples
        .iter()
        .filter(|tuple| tuple.matches_constraints(constraints))
        .cloned()
        .collect()
}

fn string_value(value: &str) -> Value {
    Value::String(value.to_owned())
}

fn list_value<'a>(values: impl IntoIterator<Item = &'a str>) -> Value {
    Value::List(values.into_iter().map(string_value).collect())
}
