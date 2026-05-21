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
#[non_exhaustive]
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

    pub fn parse(value: &str) -> Result<Self, IdentError> {
        if let Some((module, name)) = value.split_once('.') {
            if name.contains('.') {
                return Err(IdentError(value.to_string()));
            }
            return Ok(Self::qualified(Ident::new(module)?, Ident::new(name)?));
        }
        Ok(Self::new(Ident::new(value)?))
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
#[non_exhaustive]
pub struct Program {
    pub statements: Vec<Statement>,
}

impl Program {
    pub fn new(statements: Vec<Statement>) -> Self {
        Self { statements }
    }

    pub(crate) fn assign_rule_layer(&mut self, layer: RuleLayer) {
        for statement in &mut self.statements {
            statement.assign_rule_layer(layer);
        }
    }

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

impl Statement {
    pub(crate) fn assign_rule_layer(&mut self, layer: RuleLayer) {
        match self {
            Self::Rule(rule) => {
                rule.origin.layer = layer;
            }
            Self::Query(query) => {
                for rule in &mut query.local_rules {
                    rule.origin.layer = RuleLayer::Inline;
                }
            }
            Self::AtBlock { statements, .. } => {
                for statement in statements {
                    statement.assign_rule_layer(layer);
                }
            }
            Self::Fact(_)
            | Self::OptionalFact(_)
            | Self::ConfigBlock(_)
            | Self::SourceBlock(_)
            | Self::Include(_)
            | Self::Import(_)
            | Self::Verb(_)
            | Self::Doc(_)
            | Self::Predicate(_)
            | Self::Cookbook(_) => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Statement {
    Fact(Head),
    OptionalFact(Head),
    ConfigBlock(ConfigBlock),
    SourceBlock(SourceBlock),
    Rule(Rule),
    Query(Query),
    Include(IncludeDirective),
    Import(ImportDirective),
    AtBlock {
        reference: String,
        statements: Vec<Statement>,
    },
    Verb(VerbDecl),
    Doc(DocDecl),
    Predicate(PredicateDecl),
    Cookbook(CookbookDecl),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ConfigBlock {
    pub section: Ident,
    pub declarations: Vec<Declaration>,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
}

impl ConfigBlock {
    pub fn new(section: Ident, declarations: Vec<Declaration>, location: SourceLocation) -> Self {
        Self {
            section,
            declarations,
            location,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SourceBlock {
    pub source: Ident,
    pub declarations: Vec<Declaration>,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
}

impl SourceBlock {
    pub fn new(source: Ident, declarations: Vec<Declaration>, location: SourceLocation) -> Self {
        Self {
            source,
            declarations,
            location,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Declaration {
    pub name: Ident,
    pub args: Vec<CallArg>,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
}

impl Declaration {
    pub fn new(name: Ident, args: Vec<CallArg>, location: SourceLocation) -> Self {
        Self {
            name,
            args,
            location,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct IncludeDirective {
    pub path: String,
    pub location: SourceLocation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ImportDirective {
    pub module: Ident,
    pub path: String,
    pub location: SourceLocation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct VerbDecl {
    #[serde(flatten)]
    pub annotation: AnnotationDecl,
}

impl VerbDecl {
    pub fn new(args: Vec<NamedArg>, location: SourceLocation) -> Self {
        Self {
            annotation: AnnotationDecl::new(args, location),
        }
    }

    pub fn string_arg(&self, name: &str) -> Option<&str> {
        self.annotation.string_arg(name)
    }

    pub fn string_list_arg(&self, name: &str) -> Option<Vec<&str>> {
        self.annotation.string_list_arg(name)
    }

    pub fn location(&self) -> &SourceLocation {
        self.annotation.location()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DocDecl {
    pub name: String,
    pub doc: String,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
}

impl DocDecl {
    pub fn new(name: impl Into<String>, doc: impl Into<String>, location: SourceLocation) -> Self {
        Self {
            name: name.into(),
            doc: doc.into(),
            location,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn doc(&self) -> &str {
        &self.doc
    }

    pub fn location(&self) -> &SourceLocation {
        &self.location
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PredicateDecl {
    #[serde(flatten)]
    pub annotation: AnnotationDecl,
}

impl PredicateDecl {
    pub fn new(args: Vec<NamedArg>, location: SourceLocation) -> Self {
        Self {
            annotation: AnnotationDecl::new(args, location),
        }
    }

    pub fn string_arg(&self, name: &str) -> Option<&str> {
        self.annotation.string_arg(name)
    }

    pub fn string_list_arg(&self, name: &str) -> Option<Vec<&str>> {
        self.annotation.string_list_arg(name)
    }

    pub fn predicate_ref(&self) -> Option<Result<PredicateRef, IdentError>> {
        self.string_arg("name").map(PredicateRef::parse)
    }

    pub fn args(&self) -> &[NamedArg] {
        &self.annotation.args
    }

    pub fn location(&self) -> &SourceLocation {
        self.annotation.location()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CookbookDecl {
    #[serde(flatten)]
    pub annotation: AnnotationDecl,
}

impl CookbookDecl {
    pub fn new(args: Vec<NamedArg>, location: SourceLocation) -> Self {
        Self {
            annotation: AnnotationDecl::new(args, location),
        }
    }

    pub fn string_arg(&self, name: &str) -> Option<&str> {
        self.annotation.string_arg(name)
    }

    pub fn string_list_arg(&self, name: &str) -> Option<Vec<&str>> {
        self.annotation.string_list_arg(name)
    }

    pub fn has_arg(&self, name: &str) -> bool {
        self.annotation.has_arg(name)
    }

    pub fn location(&self) -> &SourceLocation {
        self.annotation.location()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AnnotationDecl {
    pub args: Vec<NamedArg>,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
}

impl AnnotationDecl {
    pub fn new(args: Vec<NamedArg>, location: SourceLocation) -> Self {
        Self { args, location }
    }

    pub fn string_arg(&self, name: &str) -> Option<&str> {
        named_string_arg(&self.args, name)
    }

    pub fn string_list_arg(&self, name: &str) -> Option<Vec<&str>> {
        named_string_list_arg(&self.args, name)
    }

    pub fn has_arg(&self, name: &str) -> bool {
        self.args.iter().any(|arg| arg.name.as_str() == name)
    }

    pub fn location(&self) -> &SourceLocation {
        &self.location
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Rule {
    pub head: Head,
    pub body: Body,
    #[serde(skip, default = "RuleOrigin::unknown")]
    pub(crate) origin: RuleOrigin,
}

impl Rule {
    pub fn new(head: Head, body: Body) -> Self {
        Self {
            head,
            body,
            origin: RuleOrigin::unknown(),
        }
    }

    pub fn origin(&self) -> &RuleOrigin {
        &self.origin
    }

    pub(crate) fn with_origin(head: Head, body: Body, origin: RuleOrigin) -> Self {
        Self { head, body, origin }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Query {
    pub local_rules: Vec<Rule>,
    pub body: Body,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Head {
    pub predicate: PredicateRef,
    pub terms: Vec<Term>,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
}

impl Head {
    pub fn arity(&self) -> usize {
        self.terms.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SourceLocation {
    pub source_name: String,
    pub line: usize,
    pub column: usize,
}

impl SourceLocation {
    pub fn new(source_name: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            source_name: source_name.into(),
            line,
            column,
        }
    }

    pub fn unknown() -> Self {
        Self::new("<unknown>", 0, 0)
    }
}

impl Default for SourceLocation {
    fn default() -> Self {
        Self::unknown()
    }
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 && self.column == 0 {
            f.write_str(&self.source_name)
        } else {
            write!(f, "{}:{}:{}", self.source_name, self.line, self.column)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RuleOrigin {
    pub layer: RuleLayer,
    pub location: SourceLocation,
}

impl RuleOrigin {
    pub(crate) fn new(layer: RuleLayer, location: SourceLocation) -> Self {
        Self { layer, location }
    }

    pub(crate) fn unknown() -> Self {
        Self::new(RuleLayer::Unknown, SourceLocation::unknown())
    }

    pub fn layer(&self) -> RuleLayer {
        self.layer
    }

    pub fn location(&self) -> &SourceLocation {
        &self.location
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RuleLayer {
    Unknown,
    Prelude,
    Project,
    Import,
    Inline,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Body {
    pub atoms: Vec<Atom>,
}

impl Body {
    pub fn is_empty(&self) -> bool {
        self.atoms.is_empty()
    }

    pub fn positive_binding_variables(&self) -> BTreeSet<Ident> {
        let mut variables = BTreeSet::new();
        self.collect_positive_binding_variables(&mut variables);
        variables
    }

    pub fn collect_positive_binding_variables(&self, out: &mut BTreeSet<Ident>) {
        for atom in &self.atoms {
            atom.collect_positive_binding_variables(out);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Atom {
    Stored(StoredAtom),
    Derived(DerivedAtom),
    Comparison(Comparison),
    Aggregation(Aggregate),
    Negation(Negation),
    TimeBlock(TimeBlock),
}

impl Atom {
    pub fn location(&self) -> &SourceLocation {
        match self {
            Self::Stored(atom) => &atom.location,
            Self::Derived(atom) => &atom.location,
            Self::Comparison(atom) => &atom.location,
            Self::Aggregation(atom) => &atom.location,
            Self::Negation(atom) => &atom.location,
            Self::TimeBlock(atom) => &atom.location,
        }
    }

    pub fn collect_positive_binding_variables(&self, out: &mut BTreeSet<Ident>) {
        match self {
            Self::Stored(stored) => stored.collect_binding_variables(out),
            Self::Derived(derived) => derived.collect_binding_variables(out),
            Self::Aggregation(aggregate) => aggregate.result.binding_variables(out),
            Self::TimeBlock(time_block) => time_block.body.collect_positive_binding_variables(out),
            Self::Comparison(_) | Self::Negation(_) => {}
        }
    }
}

impl StoredAtom {
    pub fn collect_binding_variables(&self, out: &mut BTreeSet<Ident>) {
        for field in &self.fields {
            if let Some(expr) = field.term.expr() {
                expr.binding_variables(out);
            }
        }
    }
}

impl DerivedAtom {
    pub fn collect_binding_variables(&self, out: &mut BTreeSet<Ident>) {
        for arg in &self.args {
            if let Some(expr) = arg.expr() {
                expr.binding_variables(out);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StoredAtom {
    pub relation: Ident,
    pub fields: Vec<FieldPattern>,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FieldPattern {
    pub field: Ident,
    pub term: Term,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DerivedAtom {
    pub predicate: PredicateRef,
    pub args: Vec<CallArg>,
    #[serde(default, skip_serializing_if = "CallStyle::is_complete")]
    pub style: CallStyle,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallStyle {
    #[default]
    Complete,
    Pattern,
}

impl CallStyle {
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NegatedAtom {
    Stored(StoredAtom),
    Derived(DerivedAtom),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Negation {
    pub atom: NegatedAtom,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
}

impl Negation {
    pub fn location(&self) -> &SourceLocation {
        &self.location
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Comparison {
    pub left: Expr,
    pub op: ComparisonOp,
    pub right: Expr,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
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
#[non_exhaustive]
pub struct Aggregate {
    pub result: Expr,
    pub function: AggregateFunction,
    pub args: Vec<NamedArg>,
    pub value: Expr,
    pub body: Body,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
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
#[non_exhaustive]
pub struct NamedArg {
    pub name: Ident,
    pub expr: Expr,
}

pub(crate) fn named_string_arg<'a>(args: &'a [NamedArg], name: &str) -> Option<&'a str> {
    args.iter().find_map(|arg| {
        if arg.name.as_str() != name {
            return None;
        }
        let Expr::Literal(Literal::String(value)) = &arg.expr else {
            return None;
        };
        Some(value.as_str())
    })
}

pub(crate) fn named_string_list_arg<'a>(args: &'a [NamedArg], name: &str) -> Option<Vec<&'a str>> {
    args.iter().find_map(|arg| {
        if arg.name.as_str() != name {
            return None;
        }
        let Expr::Literal(Literal::List(items)) = &arg.expr else {
            return None;
        };
        let mut strings = Vec::with_capacity(items.len());
        for item in items {
            let Literal::String(value) = item else {
                return None;
            };
            strings.push(value.as_str());
        }
        Some(strings)
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TimeBlock {
    pub reference: String,
    pub body: Body,
    #[serde(default, skip_serializing)]
    pub location: SourceLocation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CallArg {
    Positional {
        expr: Expr,
        #[serde(default, skip_serializing)]
        location: SourceLocation,
    },
    Named {
        name: Ident,
        expr: Expr,
        #[serde(default, skip_serializing)]
        location: SourceLocation,
    },
    Wildcard {
        #[serde(default, skip_serializing)]
        location: SourceLocation,
    },
}

impl CallArg {
    pub fn expr(&self) -> Option<&Expr> {
        match self {
            Self::Positional { expr, .. } | Self::Named { expr, .. } => Some(expr),
            Self::Wildcard { .. } => None,
        }
    }

    pub fn expr_mut(&mut self) -> Option<&mut Expr> {
        match self {
            Self::Positional { expr, .. } | Self::Named { expr, .. } => Some(expr),
            Self::Wildcard { .. } => None,
        }
    }

    pub fn location(&self) -> &SourceLocation {
        match self {
            Self::Positional { location, .. }
            | Self::Named { location, .. }
            | Self::Wildcard { location } => location,
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

    pub fn expr_mut(&mut self) -> Option<&mut Expr> {
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
                    if let Some(expr) = arg.expr() {
                        expr.variables(out);
                    }
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

    pub fn binding_variables(&self, out: &mut BTreeSet<Ident>) {
        match self {
            Self::Var(var) => {
                out.insert(var.clone());
            }
            Self::Tuple(items) => {
                for item in items {
                    item.binding_variables(out);
                }
            }
            Self::Literal(_) | Self::FunctionCall { .. } | Self::Binary { .. } => {}
        }
    }

    pub fn input_variables(&self, out: &mut BTreeSet<Ident>) {
        match self {
            Self::Var(_) | Self::Literal(_) => {}
            Self::FunctionCall { args, .. } => {
                for arg in args {
                    if let Some(expr) = arg.expr() {
                        expr.variables(out);
                    }
                }
            }
            Self::Binary { left, right, .. } => {
                left.variables(out);
                right.variables(out);
            }
            Self::Tuple(items) => {
                for item in items {
                    item.input_variables(out);
                }
            }
        }
    }
}
