//! Dynamic rule-layer runtime for anneal v2.
//!
//! The runtime treats source facts as immutable stored relations and
//! derives query relations by fixed point. Engine-derived primitives may
//! later be plugged in as ordinary read-only relations; the rule layer
//! itself stays source-neutral and engine-replaceable.

pub mod analysis;
pub mod ast;
pub mod eval;
mod introspection;
pub mod loader;
pub mod ndjson;
pub mod parser;
pub mod prelude;
mod primitives;

pub use analysis::{AnalyzedProgram, AnalyzedQuery, StaticError, analyze, stored_relation_fields};
pub use ast::{
    Aggregate, AggregateFunction, Atom, Body, CallArg, CallStyle, Comparison, ComparisonOp,
    DocDecl, Expr, FieldPattern, Head, Ident, ImportDirective, IncludeDirective, Literal,
    NegatedAtom, Negation, NumberLiteral, PredicateDecl, PredicateRef, Program, Query, Rule,
    SourceLocation, Statement, StoredAtom, Term, TimeBlock, VerbDecl,
};
pub use eval::{
    Binding, Database, DerivationKind, DerivationNode, EvalError, EvalOptions, Evaluator,
    ExplainDepth, ExplainOptions, QueryOutput, QueryWarning, READ_FULL_CAPABILITY, Row, Tuple,
    Value,
};
pub use loader::{LoadError, ProgramLoader, load_prelude, load_program};
pub use ndjson::{NdjsonError, write_ndjson, write_ndjson_with_meta};
pub use parser::{ParseError, parse_prelude_program, parse_program};
