//! Dynamic rule-layer runtime for anneal.
//!
//! This is the host facade layered over the shared crate-root substrate:
//!
//! - parse and load a program;
//! - analyze signatures, safety, dependencies, and strata;
//! - evaluate stored facts and engine primitives to a fixed point;
//! - project rows, explanations, prelude helpers, and NDJSON.
//!
//! Source facts remain immutable stored relations during one evaluation.
//! Engine-derived primitives enter as read-only relations, so the rule layer
//! stays source-neutral and engine-replaceable. Errors retain their phase:
//! [`ParseError`] and [`LoadError`] precede [`StaticError`], while [`EvalError`]
//! covers planning and execution. Hosts should import this facade, never its
//! private implementation modules. See CR-D51, CR-D74, and
//! `.design/2026-07-29-anneal-core-public-api-altitude.md`.

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
