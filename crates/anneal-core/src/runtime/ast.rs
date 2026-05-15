use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Datalog identifier. The grammar intentionally uses one lexical form
/// for variables, predicates, fields, and modules.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Ident(String);

impl Ident {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentError> {
        let value = value.into();
        if is_ident(&value) {
            Ok(Self(value))
        } else {
            Err(IdentError(value))
        }
    }

    pub fn new_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid identifier {0:?}")]
pub struct IdentError(String);

fn is_ident(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_lowercase() || first == '_')
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

/// A derived predicate reference, optionally module-qualified.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PredicateRef {
    pub module: Option<Ident>,
    pub name: Ident,
}

impl PredicateRef {
    pub fn new(name: Ident) -> Self {
        Self { module: None, name }
    }

    pub fn qualified(module: Ident, name: Ident) -> Self {
        Self {
            module: Some(module),
            name,
        }
    }

    pub fn display_name(&self) -> String {
        match &self.module {
            Some(module) => format!("{module}.{}", self.name),
            None => self.name.to_string(),
        }
    }
}

impl fmt::Display for PredicateRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.module {
            Some(module) => write!(f, "{module}.{}", self.name),
            None => write!(f, "{}", self.name),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub statements: Vec<Statement>,
}

impl Program {
    pub fn facts(&self) -> impl Iterator<Item = &Head> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                Statement::Fact(head) => Some(head),
                _ => None,
            })
    }

    pub fn rules(&self) -> impl Iterator<Item = &Rule> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                Statement::Rule(rule) => Some(rule),
                _ => None,
            })
    }

    pub fn queries(&self) -> impl Iterator<Item = &Query> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                Statement::Query(query) => Some(query),
                _ => None,
            })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Statement {
    Fact(Head),
    Rule(Rule),
    Query(Query),
    Include(String),
    Import {
        module: Ident,
        path: String,
    },
    AtBlock {
        reference: String,
        statements: Vec<Statement>,
    },
    Verb(VerbDecl),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerbDecl {
    pub args: Vec<NamedArg>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub head: Head,
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub local_rules: Vec<Rule>,
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Head {
    pub predicate: PredicateRef,
    pub terms: Vec<Term>,
}

impl Head {
    pub fn arity(&self) -> usize {
        self.terms.len()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub atoms: Vec<Atom>,
}

impl Body {
    pub fn is_empty(&self) -> bool {
        self.atoms.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Atom {
    Stored(StoredAtom),
    Derived(DerivedAtom),
    Comparison(Comparison),
    Aggregation(Aggregate),
    Negation(NegatedAtom),
    TimeBlock(TimeBlock),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredAtom {
    pub relation: Ident,
    pub fields: Vec<FieldPattern>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldPattern {
    pub field: Ident,
    pub term: Term,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DerivedAtom {
    pub predicate: PredicateRef,
    pub args: Vec<CallArg>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NegatedAtom {
    Stored(StoredAtom),
    Derived(DerivedAtom),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    pub left: Expr,
    pub op: ComparisonOp,
    pub right: Expr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComparisonOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    In,
    Matches,
    Contains,
    StartsWith,
    EndsWith,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Aggregate {
    pub result: Expr,
    pub function: AggregateFunction,
    pub args: Vec<NamedArg>,
    pub value: Expr,
    pub body: Body,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregateFunction {
    Count,
    Sum,
    Min,
    Max,
    Avg,
    List,
    Set,
    TopK,
    Rank,
    TakeUntil,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedArg {
    pub name: Ident,
    pub expr: Expr,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimeBlock {
    pub reference: String,
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CallArg {
    Positional(Expr),
}

impl CallArg {
    pub fn expr(&self) -> &Expr {
        match self {
            Self::Positional(expr) => expr,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Term {
    Expr(Expr),
    Wildcard,
}

impl Term {
    pub fn expr(&self) -> Option<&Expr> {
        match self {
            Self::Expr(expr) => Some(expr),
            Self::Wildcard => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Var(Ident),
    Literal(Literal),
    FunctionCall {
        function: Ident,
        args: Vec<CallArg>,
    },
    Binary {
        left: Box<Expr>,
        op: ArithmeticOp,
        right: Box<Expr>,
    },
    Tuple(Vec<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArithmeticOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    String(String),
    Number(NumberLiteral),
    Bool(bool),
    Null,
    List(Vec<Literal>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NumberLiteral {
    Int(i64),
    Float(f64),
}

impl Expr {
    pub fn variables(&self, out: &mut BTreeSet<Ident>) {
        match self {
            Self::Var(var) => {
                out.insert(var.clone());
            }
            Self::Literal(_) => {}
            Self::FunctionCall { args, .. } => {
                for arg in args {
                    arg.expr().variables(out);
                }
            }
            Self::Binary { left, right, .. } => {
                left.variables(out);
                right.variables(out);
            }
            Self::Tuple(items) => {
                for item in items {
                    item.variables(out);
                }
            }
        }
    }
}
