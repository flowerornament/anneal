use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde_json::Value as JsonValue;

use crate::runtime::ast::{Expr, Literal, Program, Query, SourceLocation, Statement, VerbDecl};
use crate::runtime::parser::{ParseError, parse_program};
use crate::source::ActorContext;

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

/// One resolved verb entry exposed to surfaces.
#[derive(Clone, Debug, PartialEq)]
pub struct VerbEntry {
    name: VerbName,
    query_source: String,
    query: Query,
    doc: String,
    output_schema: JsonValue,
    default_args: Vec<String>,
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

    pub fn default_args(&self) -> &[String] {
        &self.default_args
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
}

impl VerbRunPlan {
    fn from_entry(entry: &VerbEntry) -> Self {
        Self {
            name: entry.name.clone(),
            query_source: entry.query_source.clone(),
        }
    }

    pub fn name(&self) -> &VerbName {
        &self.name
    }

    pub fn query_source(&self) -> &str {
        &self.query_source
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
            default_args: spec.default_args,
            capabilities: spec.capabilities,
            source: VerbSource::new(layer, verb.location().clone()),
            shadowed: Vec::new(),
        })
    }
}

pub(crate) fn validate_project_verb_query_program(
    verb: &VerbDecl,
) -> Result<Program, VerbRegistryError> {
    parse_verb_decl(verb, SchemaPolicy::Exact).map(|spec| spec.query_program)
}

struct ParsedVerbDecl {
    name: VerbName,
    query_source: String,
    query_program: Program,
    query: Query,
    doc: String,
    output_schema: JsonValue,
    default_args: Vec<String>,
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
    let default_args = required_string_list(verb, "default_args")?;
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
        default_args,
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
              default_args: [],
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
              default_args: [],
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
            @verb(name: "x", query: "? a(h).", doc: "A.", output_schema: "{\"h\":\"String\"}", default_args: [], capabilities: []).
            @verb(name: "x", query: "? b(h).", doc: "B.", output_schema: "{\"h\":\"String\"}", default_args: [], capabilities: []).
            a("a").
            b("b").
            "#,
        );

        let err = VerbRegistry::from_layers(&[(VerbLayer::Project, &project)])
            .expect_err("duplicate rejected");
        assert!(matches!(err, VerbRegistryError::DuplicateInLayer { .. }));
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
              default_args: ["milestone"],
              capabilities: ["read", "release"]
            ).
            release_blocker("h", "why").
            "#,
        );

        let registry =
            VerbRegistry::from_layers(&[(VerbLayer::Project, &project)]).expect("registry builds");
        let entry = registry.require("release-blockers").expect("verb resolves");

        assert_eq!(entry.default_args(), &["milestone".to_string()]);
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
              default_args: [],
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
              default_args: [],
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
