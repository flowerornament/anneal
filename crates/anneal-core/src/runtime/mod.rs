//! Dynamic rule-layer runtime for anneal.
//!
//! The runtime treats source facts as immutable stored relations and
//! derives query relations by fixed point. Engine-derived primitives may
//! later be plugged in as ordinary read-only relations; the rule layer
//! itself stays source-neutral and engine-replaceable.

pub(crate) mod analysis;
pub(crate) mod ast;
pub(crate) mod eval;
mod evaluator;
mod introspection;
pub(crate) mod loader;
pub(crate) mod ndjson;
pub(crate) mod parser;
pub(crate) mod prelude;
pub(crate) mod primitives;
pub(crate) mod schedule;

pub use crate::vm::provenance::{DerivationKind, DerivationNode};
pub use crate::vm::value::NumberValue;
pub use analysis::{
    AnalyzedProgram, AnalyzedQuery, DependencyCycle, StaticError, StoredFieldSet, Stratum, analyze,
    stored_relation_fields,
};
pub use ast::{
    Aggregate, AggregateFunction, Atom, Body, CallArg, CallStyle, Comparison, ComparisonOp,
    DerivedAtom, DocDecl, Expr, FieldPattern, Head, Ident, ImportDirective, IncludeDirective,
    Literal, NegatedAtom, Negation, NumberLiteral, PredicateDecl, PredicateRef, Program, Query,
    Rule, SourceLocation, Statement, StoredAtom, Term, TimeBlock, VerbDecl,
};
pub use eval::{
    Database, EvalError, EvalOptions, ExplainDepth, ExplainOptions, ExplainRowLimit,
    PlanningErrorKind, QueryOutput, QueryWarning, READ_FULL_CAPABILITY, Row, Tuple, Value,
};
pub use evaluator::Evaluator;
pub use loader::{LoadError, ProgramLoader, load_prelude, load_program};
pub use ndjson::{NdjsonError, write_ndjson, write_ndjson_with_meta};
pub use parser::{ParseError, parse_prelude_program, parse_program};
pub use prelude::{
    CONTEXT_OUTPUT_SCHEMA, ContextQueryArgs, LoadedPrelude, PreludeCompatibility, PreludeError,
    PreludeFile, PreludeHash, PreludeLoadError, PreludeSet, PreludeSourceFile, PreludeSourceMap,
    datalog_string_literal, low_confidence_filter, render_context_query, standard_prelude_program,
};
