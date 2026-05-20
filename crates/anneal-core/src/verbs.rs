use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde_json::Value as JsonValue;

use crate::runtime::ast::{
    Atom, Body, CallArg, Expr, Ident, Literal, NumberLiteral, Program, Query, Rule, SourceLocation,
    Statement, VerbDecl,
};
use crate::runtime::parser::{ParseError, parse_program};
use crate::runtime::prelude::datalog_string_literal;
use crate::source::ActorContext;

pub const VERB_ARG_PREDICATE: &str = "verb_arg";

/// Layer that contributed a verb declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VerbLayer {
    Prelude,
    Project,
}

impl fmt::Display for VerbLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Prelude => f.write_str("prelude"),
            Self::Project => f.write_str("project"),
        }
    }
}

/// Validated verb name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VerbName(String);

impl VerbName {
    pub fn new(value: impl Into<String>) -> Result<Self, VerbRegistryError> {
        let value = value.into();
        if is_verb_name(&value) {
            Ok(Self(value))
        } else {
            Err(VerbRegistryError::InvalidName { name: value })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VerbName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Capability label declared by `@verb(capabilities: [...])`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VerbCapability {
    Builtin(VerbBuiltinPermission),
    Actor(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VerbBuiltinPermission {
    Read,
    Search,
    Schema,
    Describe,
    Verbs,
}

impl VerbBuiltinPermission {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Search => "search",
            Self::Schema => "schema",
            Self::Describe => "describe",
            Self::Verbs => "verbs",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Self::Read),
            "search" => Some(Self::Search),
            "schema" => Some(Self::Schema),
            "describe" => Some(Self::Describe),
            "verbs" => Some(Self::Verbs),
            _ => None,
        }
    }
}

impl VerbCapability {
    fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        VerbBuiltinPermission::parse(&value).map_or(Self::Actor(value), Self::Builtin)
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Builtin(permission) => permission.as_str(),
            Self::Actor(capability) => capability,
        }
    }
}

impl fmt::Display for VerbCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for VerbBuiltinPermission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source location plus semantic layer for a verb declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerbSource {
    layer: VerbLayer,
    location: SourceLocation,
}

impl VerbSource {
    fn new(layer: VerbLayer, location: SourceLocation) -> Self {
        Self { layer, location }
    }

    pub fn layer(&self) -> VerbLayer {
        self.layer
    }

    pub fn location(&self) -> &SourceLocation {
        &self.location
    }
}

/// Typed argument declared by `@verb(args: [...])`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerbArg {
    name: String,
    kind: VerbArgKind,
    default: Option<String>,
}

impl VerbArg {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> VerbArgKind {
        self.kind
    }

    pub fn default(&self) -> Option<&str> {
        self.default.as_deref()
    }

    pub fn parse_literal(&self, value: &str) -> Result<Literal, VerbArgValueError> {
        self.kind.parse_literal(&self.name, value)
    }

    fn sample_literal(&self) -> Literal {
        match self.kind {
            VerbArgKind::String | VerbArgKind::HandleId => Literal::String("sample".to_string()),
            VerbArgKind::Int | VerbArgKind::Number => Literal::Number(NumberLiteral::Int(1)),
            VerbArgKind::Bool => Literal::Bool(true),
        }
    }
}

/// Value type accepted by a verb argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerbArgKind {
    String,
    HandleId,
    Int,
    Number,
    Bool,
}

impl VerbArgKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::String => "String",
            Self::HandleId => "HandleId",
            Self::Int => "Int",
            Self::Number => "Number",
            Self::Bool => "Bool",
        }
    }

    fn parse_literal(self, name: &str, value: &str) -> Result<Literal, VerbArgValueError> {
        match self {
            Self::String | Self::HandleId => Ok(Literal::String(value.to_string())),
            Self::Int => value
                .parse::<i64>()
                .map(|number| Literal::Number(NumberLiteral::Int(number)))
                .map_err(|_| VerbArgValueError::Invalid {
                    name: name.to_string(),
                    kind: self,
                    value: value.to_string(),
                }),
            Self::Number => {
                if let Ok(number) = value.parse::<i64>() {
                    return Ok(Literal::Number(NumberLiteral::Int(number)));
                }
                if let Ok(number) = value.parse::<f64>()
                    && number.is_finite()
                {
                    return Ok(Literal::Number(NumberLiteral::Float(number)));
                }
                Err(VerbArgValueError::Invalid {
                    name: name.to_string(),
                    kind: self,
                    value: value.to_string(),
                })
            }
            Self::Bool => match value {
                "true" => Ok(Literal::Bool(true)),
                "false" => Ok(Literal::Bool(false)),
                _ => Err(VerbArgValueError::Invalid {
                    name: name.to_string(),
                    kind: self,
                    value: value.to_string(),
                }),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum VerbArgValueError {
    #[error("argument '{name}' expects {kind}; got {value:?}")]
    Invalid {
        name: String,
        kind: VerbArgKind,
        value: String,
    },
}

impl fmt::Display for VerbArgKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn render_verb_arg_facts(bindings: &[(String, Literal)]) -> String {
    let mut rendered = String::new();
    for (name, value) in bindings {
        rendered.push_str(&render_verb_arg_fact(name, value));
    }
    rendered
}

pub fn render_verb_arg_fact(name: &str, value: &Literal) -> String {
    format!(
        "{VERB_ARG_PREDICATE}({}, {}).\n",
        datalog_string_literal(name),
        literal_to_datalog(value)
    )
}

fn literal_to_datalog(value: &Literal) -> String {
    match value {
        Literal::String(value) => datalog_string_literal(value),
        Literal::Number(NumberLiteral::Int(value)) => value.to_string(),
        Literal::Number(NumberLiteral::Float(value)) => value.to_string(),
        Literal::Bool(value) => value.to_string(),
        Literal::Null => "null".to_string(),
        Literal::List(_) => unreachable!("verb arg literals are scalar"),
    }
}

/// One resolved verb entry exposed to surfaces.
#[derive(Clone, Debug, PartialEq)]
pub struct VerbEntry {
    name: VerbName,
    query_source: String,
    query: Query,
    doc: String,
    output_schema: JsonValue,
    args: Vec<VerbArg>,
    capabilities: Vec<VerbCapability>,
    examples: Vec<String>,
    source: VerbSource,
    shadowed: Vec<VerbSource>,
}

impl VerbEntry {
    pub fn name(&self) -> &VerbName {
        &self.name
    }

    pub fn query_source(&self) -> &str {
        &self.query_source
    }

    pub fn query(&self) -> &Query {
        &self.query
    }

    pub fn doc(&self) -> &str {
        &self.doc
    }

    pub fn output_schema(&self) -> &JsonValue {
        &self.output_schema
    }

    pub fn args(&self) -> &[VerbArg] {
        &self.args
    }

    pub fn capabilities(&self) -> &[VerbCapability] {
        &self.capabilities
    }

    pub fn examples(&self) -> &[String] {
        &self.examples
    }

    pub fn source(&self) -> &VerbSource {
        &self.source
    }

    pub fn shadowed(&self) -> &[VerbSource] {
        &self.shadowed
    }

    pub fn required_capability(&self, actor: &ActorContext) -> Option<&VerbCapability> {
        self.capabilities
            .iter()
            .find(|capability| match capability {
                VerbCapability::Builtin(_) => false,
                VerbCapability::Actor(name) => !actor.has_capability(name),
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerbRunPlan {
    name: VerbName,
    query_source: String,
    args: Vec<VerbArg>,
}

impl VerbRunPlan {
    fn from_entry(entry: &VerbEntry) -> Self {
        Self {
            name: entry.name.clone(),
            query_source: entry.query_source.clone(),
            args: entry.args.clone(),
        }
    }

    pub fn name(&self) -> &VerbName {
        &self.name
    }

    pub fn query_source(&self) -> &str {
        &self.query_source
    }

    pub fn args(&self) -> &[VerbArg] {
        &self.args
    }
}

/// Resolved registry of callable verbs after layer shadowing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VerbRegistry {
    entries: BTreeMap<VerbName, VerbEntry>,
}

impl VerbRegistry {
    pub fn from_layers(layers: &[(VerbLayer, &Program)]) -> Result<Self, VerbRegistryError> {
        let mut builder = VerbRegistryBuilder::rejecting_layer_duplicates();
        for (layer, program) in layers {
            builder.add_program(*layer, program)?;
        }
        Ok(Self {
            entries: builder.entries,
        })
    }

    pub(crate) fn from_ordered_program(program: &Program) -> Result<Self, VerbRegistryError> {
        let mut builder = VerbRegistryBuilder::allowing_ordered_shadowing();
        builder.add_program(VerbLayer::Project, program)?;
        Ok(Self {
            entries: builder.entries,
        })
    }

    pub fn get(&self, name: &str) -> Option<&VerbEntry> {
        self.entries.get(&VerbName(name.to_string()))
    }

    pub fn require(&self, name: &str) -> Result<&VerbEntry, VerbDispatchError> {
        self.get(name)
            .ok_or_else(|| VerbDispatchError::MissingVerb {
                name: name.to_string(),
            })
    }

    pub fn resolve_for_actor(
        &self,
        name: &str,
        actor: &ActorContext,
    ) -> Result<&VerbEntry, VerbDispatchError> {
        let entry = self.require(name)?;
        if let Some(capability) = entry.required_capability(actor) {
            return Err(VerbDispatchError::CapabilityDenied {
                name: name.to_string(),
                capability: capability.to_string(),
            });
        }
        Ok(entry)
    }

    pub fn run_plan_for_actor(
        &self,
        name: &str,
        actor: &ActorContext,
    ) -> Result<VerbRunPlan, VerbDispatchError> {
        self.resolve_for_actor(name, actor)
            .map(VerbRunPlan::from_entry)
    }

    pub fn iter(&self) -> impl Iterator<Item = &VerbEntry> {
        self.entries.values()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Default)]
struct VerbRegistryBuilder {
    entries: BTreeMap<VerbName, VerbEntry>,
    seen_in_layer: BTreeSet<(VerbLayer, VerbName)>,
    reject_duplicate_in_layer: bool,
}

impl VerbRegistryBuilder {
    fn rejecting_layer_duplicates() -> Self {
        Self {
            reject_duplicate_in_layer: true,
            ..Self::default()
        }
    }

    fn allowing_ordered_shadowing() -> Self {
        Self {
            reject_duplicate_in_layer: false,
            ..Self::default()
        }
    }

    fn add_program(
        &mut self,
        layer: VerbLayer,
        program: &Program,
    ) -> Result<(), VerbRegistryError> {
        self.add_statements(layer, &program.statements, false)
    }

    fn add_statements(
        &mut self,
        layer: VerbLayer,
        statements: &[Statement],
        inside_at_block: bool,
    ) -> Result<(), VerbRegistryError> {
        for statement in statements {
            match statement {
                Statement::Verb(verb) if inside_at_block => {
                    return Err(VerbRegistryError::VerbInsideAtBlock {
                        location: verb.location().clone(),
                    });
                }
                Statement::Verb(verb) => self.add_verb(layer, verb)?,
                Statement::AtBlock { statements, .. } => {
                    self.add_statements(layer, statements, true)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn add_verb(&mut self, layer: VerbLayer, verb: &VerbDecl) -> Result<(), VerbRegistryError> {
        let mut entry = VerbEntry::from_decl(layer, verb)?;
        let layer_key = (layer, entry.name.clone());
        if self.reject_duplicate_in_layer && !self.seen_in_layer.insert(layer_key.clone()) {
            return Err(VerbRegistryError::DuplicateInLayer {
                name: entry.name.to_string(),
                layer,
                location: verb.location().clone(),
            });
        }
        if !self.reject_duplicate_in_layer {
            self.seen_in_layer.insert(layer_key);
        }
        if let Some(shadowed) = self.entries.remove(&entry.name) {
            entry.shadowed.extend(shadowed.shadowed);
            entry.shadowed.push(shadowed.source);
        }
        self.entries.insert(entry.name.clone(), entry);
        Ok(())
    }
}

impl VerbEntry {
    fn from_decl(layer: VerbLayer, verb: &VerbDecl) -> Result<Self, VerbRegistryError> {
        let spec = parse_verb_decl(verb, SchemaPolicy::Projection)?;
        Ok(Self {
            examples: vec![spec.query_source.clone()],
            name: spec.name,
            query_source: spec.query_source,
            query: spec.query,
            doc: spec.doc,
            output_schema: spec.output_schema,
            args: spec.args,
            capabilities: spec.capabilities,
            source: VerbSource::new(layer, verb.location().clone()),
            shadowed: Vec::new(),
        })
    }
}

pub(crate) fn validate_project_verb_query_program(
    verb: &VerbDecl,
) -> Result<Program, VerbRegistryError> {
    let spec = parse_verb_decl(verb, SchemaPolicy::Exact)?;
    validate_verb_arg_references(
        spec.name.as_str(),
        &spec.query_program,
        &spec.args,
        verb.location(),
    )?;
    let mut program = sample_arg_facts(&spec.args, verb.location())?;
    program.statements.extend(spec.query_program.statements);
    Ok(program)
}

pub(crate) fn validate_no_verb_arg_definitions(program: &Program) -> Result<(), VerbRegistryError> {
    for statement in &program.statements {
        validate_statement_has_no_verb_arg_definition(statement)?;
    }
    Ok(())
}

struct ParsedVerbDecl {
    name: VerbName,
    query_source: String,
    query_program: Program,
    query: Query,
    doc: String,
    output_schema: JsonValue,
    args: Vec<VerbArg>,
    capabilities: Vec<VerbCapability>,
}

#[derive(Clone, Copy)]
enum SchemaPolicy {
    Projection,
    Exact,
}

fn parse_verb_decl(
    verb: &VerbDecl,
    schema_policy: SchemaPolicy,
) -> Result<ParsedVerbDecl, VerbRegistryError> {
    let name = VerbName::new(required_string(verb, "name")?.to_string())
        .map_err(|err| err.with_location(verb.location().clone()))?;
    let query_source = required_string(verb, "query")?.to_string();
    let doc = required_string(verb, "doc")?.to_string();
    let output_schema = parse_output_schema(verb)?;
    let args = parse_args(verb)?;
    let capabilities = required_string_list(verb, "capabilities")?
        .into_iter()
        .map(VerbCapability::new)
        .collect::<Vec<_>>();
    let query_program = parse_program(&format!("{}:@verb:{name}", verb.location()), &query_source)
        .map_err(|source| VerbRegistryError::QueryParse {
            name: name.to_string(),
            location: verb.location().clone(),
            source: Box::new(source),
        })?;
    let query = single_query(name.as_str(), verb, &query_program)?;
    if matches!(schema_policy, SchemaPolicy::Exact) {
        validate_query_schema(name.as_str(), verb, &query, &output_schema)?;
    }
    Ok(ParsedVerbDecl {
        name,
        query_source,
        query_program,
        query,
        doc,
        output_schema,
        args,
        capabilities,
    })
}

fn required_string<'a>(verb: &'a VerbDecl, field: &str) -> Result<&'a str, VerbRegistryError> {
    verb.string_arg(field)
        .ok_or_else(|| VerbRegistryError::MissingField {
            field: field.to_string(),
            location: verb.location().clone(),
        })
}

fn parse_output_schema(verb: &VerbDecl) -> Result<JsonValue, VerbRegistryError> {
    let schema = required_string(verb, "output_schema")?;
    let value = serde_json::from_str::<JsonValue>(schema).map_err(|source| {
        VerbRegistryError::InvalidSchema {
            location: verb.location().clone(),
            source,
        }
    })?;
    if !value.is_object() {
        return Err(VerbRegistryError::SchemaMustBeObject {
            location: verb.location().clone(),
        });
    }
    validate_schema_value(&value, verb.location())?;
    Ok(value)
}

fn parse_args(verb: &VerbDecl) -> Result<Vec<VerbArg>, VerbRegistryError> {
    let mut args = Vec::new();
    let mut seen = BTreeSet::new();
    for spec in required_string_list(verb, "args")? {
        let arg = parse_arg_spec(&spec, verb.location())?;
        if !seen.insert(arg.name.clone()) {
            return Err(VerbRegistryError::DuplicateArg {
                name: arg.name,
                location: verb.location().clone(),
            });
        }
        args.push(arg);
    }
    Ok(args)
}

fn sample_arg_facts(
    args: &[VerbArg],
    location: &SourceLocation,
) -> Result<Program, VerbRegistryError> {
    let mut source = String::new();
    for arg in args {
        source.push_str(&render_verb_arg_fact(arg.name(), &arg.sample_literal()));
    }
    parse_program(&format!("{location}:@verb:args"), &source).map_err(|source| {
        VerbRegistryError::ArgFactParse {
            location: location.clone(),
            source: Box::new(source),
        }
    })
}

fn validate_statement_has_no_verb_arg_definition(
    statement: &Statement,
) -> Result<(), VerbRegistryError> {
    match statement {
        Statement::Fact(head) | Statement::OptionalFact(head)
            if is_verb_arg_predicate(&head.predicate) =>
        {
            Err(VerbRegistryError::ReservedArgDefinition {
                location: head.location.clone(),
            })
        }
        Statement::Rule(rule) if is_verb_arg_predicate(&rule.head.predicate) => {
            Err(VerbRegistryError::ReservedArgDefinition {
                location: rule.head.location.clone(),
            })
        }
        Statement::AtBlock { statements, .. } => {
            for nested in statements {
                validate_statement_has_no_verb_arg_definition(nested)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_verb_arg_references(
    name: &str,
    program: &Program,
    args: &[VerbArg],
    location: &SourceLocation,
) -> Result<(), VerbRegistryError> {
    for statement in &program.statements {
        validate_statement_verb_arg_references(name, statement, args, location)?;
    }
    Ok(())
}

fn validate_statement_verb_arg_references(
    name: &str,
    statement: &Statement,
    args: &[VerbArg],
    location: &SourceLocation,
) -> Result<(), VerbRegistryError> {
    match statement {
        Statement::Fact(head) | Statement::OptionalFact(head)
            if is_verb_arg_predicate(&head.predicate) =>
        {
            Err(VerbRegistryError::ReservedArgDefinition {
                location: head.location.clone(),
            })
        }
        Statement::Rule(rule) => {
            if is_verb_arg_predicate(&rule.head.predicate) {
                return Err(VerbRegistryError::ReservedArgDefinition {
                    location: rule.head.location.clone(),
                });
            }
            validate_rule_verb_arg_references(name, rule, args, location)
        }
        Statement::Query(query) => {
            for rule in &query.local_rules {
                validate_rule_verb_arg_references(name, rule, args, location)?;
            }
            validate_body_verb_arg_references(name, &query.body, args, location)
        }
        Statement::AtBlock { statements, .. } => {
            for nested in statements {
                validate_statement_verb_arg_references(name, nested, args, location)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_rule_verb_arg_references(
    name: &str,
    rule: &Rule,
    args: &[VerbArg],
    location: &SourceLocation,
) -> Result<(), VerbRegistryError> {
    validate_body_verb_arg_references(name, &rule.body, args, location)
}

fn validate_body_verb_arg_references(
    name: &str,
    body: &Body,
    args: &[VerbArg],
    location: &SourceLocation,
) -> Result<(), VerbRegistryError> {
    for atom in &body.atoms {
        match atom {
            Atom::Derived(atom) if is_verb_arg_predicate(&atom.predicate) => {
                validate_verb_arg_atom(name, &atom.args, args, &atom.location, location)?;
            }
            Atom::Negation(negation) => {
                if let crate::runtime::ast::NegatedAtom::Derived(atom) = &negation.atom
                    && is_verb_arg_predicate(&atom.predicate)
                {
                    validate_verb_arg_atom(name, &atom.args, args, &atom.location, location)?;
                }
            }
            Atom::TimeBlock(time_block) => {
                validate_body_verb_arg_references(name, &time_block.body, args, location)?;
            }
            Atom::Aggregation(aggregate) => {
                validate_body_verb_arg_references(name, &aggregate.body, args, location)?;
            }
            Atom::Stored(_) | Atom::Derived(_) | Atom::Comparison(_) => {}
        }
    }
    Ok(())
}

fn validate_verb_arg_atom(
    name: &str,
    call_args: &[CallArg],
    args: &[VerbArg],
    atom_location: &SourceLocation,
    verb_location: &SourceLocation,
) -> Result<(), VerbRegistryError> {
    let [first, _second] = call_args else {
        return Err(VerbRegistryError::InvalidArgReference {
            name: name.to_string(),
            location: atom_location.clone(),
            message: format!("{VERB_ARG_PREDICATE}/2 expects a string arg name and a value"),
        });
    };
    let Expr::Literal(Literal::String(arg_name)) = first.expr() else {
        return Err(VerbRegistryError::InvalidArgReference {
            name: name.to_string(),
            location: first.location().clone(),
            message: format!("{VERB_ARG_PREDICATE}/2 first argument must be a string literal"),
        });
    };
    if args.iter().any(|arg| arg.name() == arg_name) {
        Ok(())
    } else {
        Err(VerbRegistryError::UnknownArgReference {
            name: name.to_string(),
            arg: arg_name.clone(),
            expected: args
                .iter()
                .map(|arg| arg.name().to_string())
                .collect::<Vec<_>>(),
            location: verb_location.clone(),
        })
    }
}

fn is_verb_arg_predicate(predicate: &crate::runtime::ast::PredicateRef) -> bool {
    predicate.module.is_none() && predicate.name.as_str() == VERB_ARG_PREDICATE
}

fn parse_arg_spec(spec: &str, location: &SourceLocation) -> Result<VerbArg, VerbRegistryError> {
    let (name, rest) = spec
        .split_once(':')
        .ok_or_else(|| VerbRegistryError::InvalidArgSpec {
            spec: spec.to_string(),
            location: location.clone(),
        })?;
    let name = name.trim();
    if Ident::new(name).is_err() {
        return Err(VerbRegistryError::InvalidArgSpec {
            spec: spec.to_string(),
            location: location.clone(),
        });
    }
    let (kind, default) = rest
        .split_once('=')
        .map_or((rest.trim(), None), |(kind, default)| {
            (kind.trim(), Some(default.trim().to_string()))
        });
    let kind = match kind {
        "String" => VerbArgKind::String,
        "HandleId" => VerbArgKind::HandleId,
        "Int" => VerbArgKind::Int,
        "Number" => VerbArgKind::Number,
        "Bool" => VerbArgKind::Bool,
        _ => {
            return Err(VerbRegistryError::InvalidArgSpec {
                spec: spec.to_string(),
                location: location.clone(),
            });
        }
    };
    if default.as_ref().is_some_and(String::is_empty) {
        return Err(VerbRegistryError::InvalidArgSpec {
            spec: spec.to_string(),
            location: location.clone(),
        });
    }
    if let Some(default) = &default {
        validate_arg_default(kind, default).map_err(|()| VerbRegistryError::InvalidArgSpec {
            spec: spec.to_string(),
            location: location.clone(),
        })?;
    }
    Ok(VerbArg {
        name: name.to_string(),
        kind,
        default,
    })
}

fn validate_arg_default(kind: VerbArgKind, value: &str) -> Result<(), ()> {
    kind.parse_literal("<default>", value)
        .map(|_| ())
        .map_err(|_| ())
}

fn validate_schema_value(
    value: &JsonValue,
    location: &SourceLocation,
) -> Result<(), VerbRegistryError> {
    match value {
        JsonValue::String(kind) if !kind.is_empty() => Ok(()),
        JsonValue::Object(fields) => {
            for field in fields.values() {
                validate_schema_value(field, location)?;
            }
            Ok(())
        }
        JsonValue::Array(items) => match items.as_slice() {
            [JsonValue::Object(_)] => validate_schema_value(&items[0], location),
            _ => Err(VerbRegistryError::UnsupportedSchema {
                location: location.clone(),
            }),
        },
        _ => Err(VerbRegistryError::UnsupportedSchema {
            location: location.clone(),
        }),
    }
}

fn required_string_list(verb: &VerbDecl, field: &str) -> Result<Vec<String>, VerbRegistryError> {
    let Some(arg) = verb
        .annotation
        .args
        .iter()
        .find(|arg| arg.name.as_str() == field)
    else {
        return Err(VerbRegistryError::MissingField {
            field: field.to_string(),
            location: verb.location().clone(),
        });
    };
    let Expr::Literal(Literal::List(items)) = &arg.expr else {
        return Err(VerbRegistryError::InvalidListField {
            field: field.to_string(),
            location: verb.location().clone(),
        });
    };
    items
        .iter()
        .map(|item| match item {
            Literal::String(value) => Ok(value.clone()),
            _ => Err(VerbRegistryError::InvalidListField {
                field: field.to_string(),
                location: verb.location().clone(),
            }),
        })
        .collect()
}

fn single_query(
    name: &str,
    verb: &VerbDecl,
    program: &Program,
) -> Result<Query, VerbRegistryError> {
    let mut queries = program.queries();
    let Some(query) = queries.next() else {
        return Err(VerbRegistryError::QueryCount {
            name: name.to_string(),
            count: 0,
            location: verb.location().clone(),
        });
    };
    if queries.next().is_some() {
        return Err(VerbRegistryError::QueryCount {
            name: name.to_string(),
            count: 2 + queries.count(),
            location: verb.location().clone(),
        });
    }
    Ok(query.clone())
}

fn validate_query_schema(
    name: &str,
    verb: &VerbDecl,
    query: &Query,
    schema: &JsonValue,
) -> Result<(), VerbRegistryError> {
    let expected = output_schema_fields(schema);
    let actual = query_output_fields(query);
    if expected != actual {
        return Err(VerbRegistryError::SchemaMismatch {
            name: name.to_string(),
            expected: expected.into_iter().collect(),
            actual: actual.into_iter().collect(),
            location: verb.location().clone(),
        });
    }
    Ok(())
}

fn output_schema_fields(schema: &JsonValue) -> BTreeSet<String> {
    schema
        .as_object()
        .expect("schema was validated as object")
        .keys()
        .cloned()
        .collect()
}

fn query_output_fields(query: &Query) -> BTreeSet<String> {
    query
        .body
        .positive_binding_variables()
        .into_iter()
        .map(|ident| ident.to_string())
        .collect()
}

fn is_verb_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_lowercase() || first == '_')
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

#[derive(Debug, thiserror::Error)]
pub enum VerbDispatchError {
    #[error("unknown verb '{name}'")]
    MissingVerb { name: String },
    #[error("verb '{name}' requires capability '{capability}'")]
    CapabilityDenied { name: String, capability: String },
}

#[derive(Debug, thiserror::Error)]
pub enum VerbRegistryError {
    #[error("{location}: @verb missing string field '{field}'")]
    MissingField {
        field: String,
        location: SourceLocation,
    },
    #[error("{location}: invalid verb name '{name}'")]
    InvalidNameAt {
        name: String,
        location: SourceLocation,
    },
    #[error("invalid verb name '{name}'")]
    InvalidName { name: String },
    #[error("{location}: @verb output_schema must be valid JSON: {source}")]
    InvalidSchema {
        location: SourceLocation,
        source: serde_json::Error,
    },
    #[error("{location}: @verb output_schema must be a JSON object")]
    SchemaMustBeObject { location: SourceLocation },
    #[error("{location}: @verb '{name}' query must parse: {source}")]
    QueryParse {
        name: String,
        location: SourceLocation,
        source: Box<ParseError>,
    },
    #[error("{location}: @verb '{name}' must contain exactly one query, found {count}")]
    QueryCount {
        name: String,
        count: usize,
        location: SourceLocation,
    },
    #[error("{location}: @verb must not be declared inside at() blocks")]
    VerbInsideAtBlock { location: SourceLocation },
    #[error("{location}: @verb field '{field}' must be a list of strings")]
    InvalidListField {
        field: String,
        location: SourceLocation,
    },
    #[error("{location}: @verb args entry '{spec}' must use name:Type or name:Type=default")]
    InvalidArgSpec {
        spec: String,
        location: SourceLocation,
    },
    #[error("{location}: duplicate @verb arg '{name}'")]
    DuplicateArg {
        name: String,
        location: SourceLocation,
    },
    #[error("{location}: 'verb_arg' is reserved for runtime verb invocation arguments")]
    ReservedArgDefinition { location: SourceLocation },
    #[error("{location}: @verb '{name}' has invalid verb_arg reference: {message}")]
    InvalidArgReference {
        name: String,
        location: SourceLocation,
        message: String,
    },
    #[error(
        "{location}: @verb '{name}' references undeclared argument '{arg}'; declared args: {expected:?}"
    )]
    UnknownArgReference {
        name: String,
        arg: String,
        expected: Vec<String>,
        location: SourceLocation,
    },
    #[error("{location}: @verb argument facts could not be generated: {source}")]
    ArgFactParse {
        location: SourceLocation,
        source: Box<ParseError>,
    },
    #[error("{location}: @verb output_schema uses an unsupported shape")]
    UnsupportedSchema { location: SourceLocation },
    #[error(
        "{location}: @verb '{name}' output_schema fields {expected:?} do not match query fields {actual:?}"
    )]
    SchemaMismatch {
        name: String,
        expected: Vec<String>,
        actual: Vec<String>,
        location: SourceLocation,
    },
    #[error("{location}: duplicate {layer} @verb '{name}'")]
    DuplicateInLayer {
        name: String,
        layer: VerbLayer,
        location: SourceLocation,
    },
}

impl VerbRegistryError {
    fn with_location(self, location: SourceLocation) -> Self {
        match self {
            Self::InvalidName { name } => Self::InvalidNameAt { name, location },
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{parse_prelude_program, parse_program};

    fn program(source: &str, input: &str) -> Program {
        parse_program(source, input).expect("program parses")
    }

    fn prelude(input: &str) -> Program {
        parse_prelude_program("prelude.dl", input).expect("prelude parses")
    }

    #[test]
    fn registry_exposes_resolved_project_shadow_over_prelude() {
        let prelude = prelude(
            r#"
            @verb(
              name: "work",
              query: "? prelude_work(h).",
              doc: "Prelude work.",
              output_schema: "{\"h\":\"HandleId\"}",
              args: [],
              capabilities: ["read"]
            ).
            prelude_work("p").
            "#,
        );
        let project = program(
            "anneal.dl",
            r#"
            @verb(
              name: "work",
              query: "? project_work(h).",
              doc: "Project work.",
              output_schema: "{\"h\":\"HandleId\"}",
              args: [],
              capabilities: ["read"]
            ).
            project_work("p").
            "#,
        );

        let registry = VerbRegistry::from_layers(&[
            (VerbLayer::Prelude, &prelude),
            (VerbLayer::Project, &project),
        ])
        .expect("registry builds");

        let work = registry.require("work").expect("work resolves");
        assert_eq!(work.source().layer(), VerbLayer::Project);
        assert_eq!(work.doc(), "Project work.");
        assert_eq!(work.shadowed().len(), 1);
        assert_eq!(work.shadowed()[0].layer(), VerbLayer::Prelude);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn registry_rejects_duplicate_verbs_in_one_layer() {
        let project = program(
            "anneal.dl",
            r#"
            @verb(name: "x", query: "? a(h).", doc: "A.", output_schema: "{\"h\":\"String\"}", args: [], capabilities: []).
            @verb(name: "x", query: "? b(h).", doc: "B.", output_schema: "{\"h\":\"String\"}", args: [], capabilities: []).
            a("a").
            b("b").
            "#,
        );

        let err = VerbRegistry::from_layers(&[(VerbLayer::Project, &project)])
            .expect_err("duplicate rejected");
        assert!(matches!(err, VerbRegistryError::DuplicateInLayer { .. }));
    }

    #[test]
    fn registry_rejects_mistyped_verb_arg_defaults() {
        let project = program(
            "anneal.dl",
            r#"
            @verb(
              name: "searchish",
              query: "? item(h).",
              doc: "Searchish.",
              output_schema: "{\"h\":\"String\"}",
              args: ["limit:Number=many"],
              capabilities: []
            ).
            item("h").
            "#,
        );

        let err = VerbRegistry::from_layers(&[(VerbLayer::Project, &project)])
            .expect_err("mistyped default rejected");
        assert!(matches!(err, VerbRegistryError::InvalidArgSpec { .. }));
    }

    #[test]
    fn project_verb_validation_rejects_unknown_arg_reference() {
        let project = program(
            "anneal.dl",
            r#"
            @verb(
              name: "release-blockers",
              query: "release_row(h) := verb_arg(\"milestone\", milestone), h = milestone. ? release_row(h).",
              doc: "Open blockers.",
              output_schema: "{\"h\":\"String\"}",
              args: ["target:String"],
              capabilities: []
            ).
            "#,
        );
        let verb = project
            .statements
            .iter()
            .find_map(|statement| match statement {
                Statement::Verb(verb) => Some(verb),
                _ => None,
            })
            .expect("verb present");

        let err = validate_project_verb_query_program(verb).expect_err("unknown arg rejected");

        assert!(matches!(
            err,
            VerbRegistryError::UnknownArgReference { arg, .. } if arg == "milestone"
        ));
    }

    #[test]
    fn project_program_rejects_global_verb_arg_definitions() {
        let project = program("anneal.dl", r#"verb_arg("milestone", "v0.11")."#);

        let err = validate_no_verb_arg_definitions(&project).expect_err("reserved fact rejected");

        assert!(matches!(
            err,
            VerbRegistryError::ReservedArgDefinition { .. }
        ));
    }

    #[test]
    fn registry_keeps_query_ast_schema_capabilities_and_examples() {
        let project = program(
            "anneal.dl",
            r#"
            @verb(
              name: "release-blockers",
              query: "? release_blocker(h, why).",
              doc: "Open blockers.",
              output_schema: "{\"h\":\"HandleId\",\"why\":\"String\"}",
              args: ["milestone:String"],
              capabilities: ["read", "release"]
            ).
            release_blocker("h", "why").
            "#,
        );

        let registry =
            VerbRegistry::from_layers(&[(VerbLayer::Project, &project)]).expect("registry builds");
        let entry = registry.require("release-blockers").expect("verb resolves");

        assert_eq!(entry.args().len(), 1);
        assert_eq!(entry.args()[0].name(), "milestone");
        assert_eq!(entry.args()[0].kind(), VerbArgKind::String);
        assert_eq!(entry.args()[0].default(), None);
        assert_eq!(
            entry
                .capabilities()
                .iter()
                .map(VerbCapability::as_str)
                .collect::<Vec<_>>(),
            ["read", "release"]
        );
        assert_eq!(
            entry.examples(),
            &["? release_blocker(h, why).".to_string()]
        );
        assert!(!entry.query().body.is_empty());
        assert_eq!(
            entry.output_schema()["why"],
            JsonValue::String("String".to_string())
        );
    }

    #[test]
    fn registry_reports_missing_and_capability_denied_dispatch() {
        let project = program(
            "anneal.dl",
            r#"
            @verb(
              name: "deploy",
              query: "? deploy_target(h).",
              doc: "Deploy target.",
              output_schema: "{\"h\":\"String\"}",
              args: [],
              capabilities: ["release"]
            ).
            deploy_target("prod").
            "#,
        );
        let registry =
            VerbRegistry::from_layers(&[(VerbLayer::Project, &project)]).expect("registry builds");

        assert!(matches!(
            registry.require("missing"),
            Err(VerbDispatchError::MissingVerb { .. })
        ));
        assert!(matches!(
            registry.resolve_for_actor("deploy", &ActorContext::anonymous_mcp()),
            Err(VerbDispatchError::CapabilityDenied { .. })
        ));
        let actor = ActorContext::anonymous_mcp();
        let actor = ActorContext {
            actor: actor.actor,
            capabilities: BTreeSet::from(["release".to_string()]),
        };
        registry
            .resolve_for_actor("deploy", &actor)
            .expect("capability admits verb");
    }

    #[test]
    fn read_verbs_are_intrinsically_callable() {
        let project = program(
            "anneal.dl",
            r#"
            @verb(
              name: "readable",
              query: "? item(h).",
              doc: "Public read.",
              output_schema: "{\"h\":\"String\"}",
              args: [],
              capabilities: ["read"]
            ).
            item("h").
            "#,
        );
        let registry =
            VerbRegistry::from_layers(&[(VerbLayer::Project, &project)]).expect("registry builds");

        registry
            .resolve_for_actor("readable", &ActorContext::anonymous_mcp())
            .expect("read is always allowed");
    }
}
