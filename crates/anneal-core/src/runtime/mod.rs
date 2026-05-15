//! Dynamic rule-layer runtime for anneal v2.
//!
//! The runtime treats source facts as immutable stored relations and
//! derives query relations by fixed point. Engine-derived primitives may
//! later be plugged in as ordinary read-only relations; the rule layer
//! itself stays source-neutral and engine-replaceable.

pub mod analysis;
pub mod ast;
pub mod eval;
pub mod ndjson;
pub mod parser;

pub use analysis::{AnalyzedProgram, AnalyzedQuery, StaticError, analyze};
pub use ast::{
    Aggregate, AggregateFunction, Atom, Body, CallArg, Comparison, ComparisonOp, Expr,
    FieldPattern, Head, Ident, Literal, NumberLiteral, PredicateRef, Program, Query, Rule,
    Statement, StoredAtom, Term, TimeBlock, VerbDecl,
};
pub use eval::{Binding, Database, EvalError, Evaluator, QueryOutput, Row, Tuple, Value};
pub use ndjson::{NdjsonError, write_ndjson};
pub use parser::{ParseError, parse_program};
