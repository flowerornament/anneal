//! Private syntax crate for anneal's Datalog dialect.
//!
//! This crate owns parsing, source locations, AST shapes, and host-neutral
//! include/import loading. Runtime analysis and evaluation stay in
//! `anneal-core`.

pub mod ast;
pub mod loader;
pub mod parser;

pub use ast::{
    Aggregate, AggregateFunction, ArithmeticOp, Atom, Body, CallArg, CallStyle, Comparison,
    ComparisonOp, DerivedAtom, DocDecl, Expr, FieldPattern, Head, Ident, IdentError,
    ImportDirective, IncludeDirective, Literal, NamedArg, NegatedAtom, Negation, NumberLiteral,
    PredicateDecl, PredicateRef, Program, Query, Rule, RuleLayer, RuleOrigin, SourceLocation,
    Statement, StoredAtom, Term, TimeBlock, VerbDecl,
};
pub use loader::{LoadError, ProgramLoader, load_prelude, load_program};
pub use parser::{ParseError, parse_prelude_program, parse_program};
