use std::collections::BTreeMap;
use std::path::Path;

use crate::config_schema::{
    RuntimeConfigEntryError, RuntimeConfigLifecycle, runtime_config_declaration_for,
};
use crate::facts::ConfigFact;
use crate::ids::CorpusId;
use crate::runtime::analysis::{StaticError, analyze};
use crate::runtime::ast::{
    CallArg, Declaration, Expr, Head, Literal, NumberLiteral, Program, SourceLocation, Statement,
    Term,
};
use crate::runtime::loader::{LoadError, load_program};
use crate::runtime::parser::ParseError;
use crate::source::{
    ConfigEntry, ConfigFacts, ConfigKey, ConfigValueShape, DuplicateConfigOrdinal, SourceInfo,
};
use crate::verbs::{
    VerbRegistryError, validate_no_verb_arg_definitions, validate_project_verb_query_program,
};

pub const PROJECT_RULE_FILE: &str = "anneal.dl";

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectExtension {
    discovery: ConfigFacts,
    runtime_config: ConfigFacts,
    program: Program,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectExtensionParts {
    pub discovery: ConfigFacts,
    pub runtime_config: ConfigFacts,
    pub program: Program,
}

impl ProjectExtension {
    pub fn discovery(&self) -> &ConfigFacts {
        &self.discovery
    }

    pub fn runtime_config(&self) -> &ConfigFacts {
        &self.runtime_config
    }

    pub fn runtime_config_facts(&self, corpus: &CorpusId) -> Vec<ConfigFact> {
        self.runtime_config
            .entries()
            .iter()
            .map(|entry| ConfigFact {
                corpus: corpus.clone(),
                key: entry.key.clone(),
                value: entry.value.clone(),
                ordinal: entry.ordinal,
            })
            .collect()
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn into_parts(self) -> ProjectExtensionParts {
        ProjectExtensionParts {
            discovery: self.discovery,
            runtime_config: self.runtime_config,
            program: self.program,
        }
    }
}

pub fn load_project_extension(
    root: impl AsRef<Path>,
    sources: &[SourceInfo],
    base_program: &Program,
) -> Result<ProjectExtension, ProjectLoadError> {
    let loaded = load_program(root, PROJECT_RULE_FILE)?;
    let (discovery, runtime_config, program) = split_project_program(loaded, sources)?;
    validate_no_verb_arg_definitions(&program)?;
    validate_verbs(&program, base_program)?;
    Ok(ProjectExtension {
        discovery,
        runtime_config,
        program,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShadowWarning {
    pub predicate: String,
    pub location: SourceLocation,
    pub replaced_clauses: usize,
}

pub fn merge_program_layers(base: Program, extension: Program) -> (Program, Vec<ShadowWarning>) {
    let shadowed = shadowed_predicates(&extension);
    if shadowed.is_empty() {
        let mut statements = base.statements;
        statements.extend(extension.statements);
        return (Program::new(statements), Vec::new());
    }

    let mut replaced = BTreeMap::<String, usize>::new();
    let mut statements = Vec::new();
    for statement in base.statements {
        if let Some(predicate) = statement_defined_predicate(&statement)
            && shadowed.contains_key(&predicate)
        {
            *replaced.entry(predicate).or_default() += 1;
            continue;
        }
        statements.push(statement);
    }
    statements.extend(extension.statements);

    let warnings = shadowed
        .into_iter()
        .filter_map(|(predicate, location)| {
            replaced
                .get(&predicate)
                .copied()
                .map(|replaced_clauses| ShadowWarning {
                    predicate,
                    location,
                    replaced_clauses,
                })
        })
        .collect();
    (Program::new(statements), warnings)
}

fn shadowed_predicates(program: &Program) -> BTreeMap<String, SourceLocation> {
    let mut shadowed = BTreeMap::new();
    for statement in &program.statements {
        if let Some((predicate, location)) = statement_definition(statement) {
            shadowed.entry(predicate).or_insert(location);
        }
    }
    shadowed
}

fn statement_defined_predicate(statement: &Statement) -> Option<String> {
    statement_definition(statement).map(|(predicate, _)| predicate)
}

fn statement_definition(statement: &Statement) -> Option<(String, SourceLocation)> {
    match statement {
        Statement::Fact(head) | Statement::OptionalFact(head) => {
            Some((head.predicate.display_name(), head.location.clone()))
        }
        Statement::Rule(rule) => Some((
            rule.head.predicate.display_name(),
            rule.head.location.clone(),
        )),
        Statement::Doc(doc) => Some((doc.name().to_string(), doc.location().clone())),
        Statement::Query(_)
        | Statement::ConfigBlock(_)
        | Statement::SourceBlock(_)
        | Statement::Include(_)
        | Statement::Import(_)
        | Statement::AtBlock { .. }
        | Statement::Verb(_)
        | Statement::Predicate(_)
        | Statement::Cookbook(_) => None,
    }
}

fn split_project_program(
    program: Program,
    sources: &[SourceInfo],
) -> Result<(ConfigFacts, ConfigFacts, Program), ProjectLoadError> {
    let resolver = DiscoveryResolver::new(sources);
    let mut discovery = Vec::new();
    let mut runtime_config = Vec::new();
    let mut statements = Vec::new();

    for statement in program.statements {
        match statement {
            Statement::ConfigBlock(block) => {
                runtime_config.extend(config_block_entries(&block)?);
            }
            Statement::SourceBlock(block) => {
                discovery.extend(resolver.entries_for_source_block(&block)?);
            }
            Statement::Fact(head) => match resolver.entries_for_required_fact(&head)? {
                Some(entries) => discovery.extend(entries),
                None => statements.push(Statement::Fact(head)),
            },
            Statement::OptionalFact(head) => {
                discovery.extend(resolver.entries_for_optional_fact(&head)?);
            }
            other => statements.push(other),
        }
    }

    let discovery = ConfigFacts::try_from_entries(discovery)
        .map_err(ProjectLoadError::DuplicateDiscoveryOrdinal)?;
    let runtime_config = ConfigFacts::try_from_entries(runtime_config)
        .map_err(ProjectLoadError::DuplicateRuntimeConfigOrdinal)?;
    Ok((discovery, runtime_config, Program::new(statements)))
}

struct DiscoveryResolver {
    schemas: BTreeMap<String, ConfigKey>,
    by_local: BTreeMap<String, Vec<String>>,
}

impl DiscoveryResolver {
    fn new(sources: &[SourceInfo]) -> Self {
        let mut schemas = BTreeMap::new();
        let mut by_local = BTreeMap::<String, Vec<String>>::new();
        for source in sources {
            for key in &source.config_keys {
                schemas.insert(key.key().to_string(), key.clone());
                if let Some((_, local)) = key.key().split_once('.') {
                    by_local
                        .entry(local.to_string())
                        .or_default()
                        .push(key.key().to_string());
                }
            }
        }
        Self { schemas, by_local }
    }

    fn entries_for_required_fact(
        &self,
        head: &Head,
    ) -> Result<Option<Vec<ConfigEntry>>, ProjectLoadError> {
        let Some(key) = self.resolve_required_key(head)? else {
            return Ok(None);
        };
        let schema = self.schema(&key).expect("resolved key has schema");
        Ok(Some(config_entries(schema, head)?))
    }

    fn entries_for_optional_fact(&self, head: &Head) -> Result<Vec<ConfigEntry>, ProjectLoadError> {
        let Some(key) = self.resolve_optional_key(head)? else {
            return Ok(Vec::new());
        };
        let schema = self.schema(&key).expect("resolved key has schema");
        config_entries(schema, head)
    }

    fn entries_for_source_block(
        &self,
        block: &crate::runtime::ast::SourceBlock,
    ) -> Result<Vec<ConfigEntry>, ProjectLoadError> {
        let mut entries = Vec::new();
        for declaration in &block.declarations {
            let key = format!("{}.{}", block.source, declaration.name);
            let Some(schema) = self.schema(&key) else {
                return Err(ProjectLoadError::UnknownDiscoveryKey {
                    key,
                    location: declaration.location.clone(),
                });
            };
            entries.extend(declaration_entries(schema, declaration)?);
        }
        Ok(entries)
    }

    fn resolve_required_key(&self, head: &Head) -> Result<Option<String>, ProjectLoadError> {
        if head.predicate.module.is_some() {
            let key = head.predicate.display_name();
            if self.schemas.contains_key(&key) {
                return Ok(Some(key));
            }
            return if is_project_rule_file(&head.location) {
                Err(ProjectLoadError::UnknownDiscoveryKey {
                    key,
                    location: head.location.clone(),
                })
            } else {
                Ok(None)
            };
        }

        self.resolve_unqualified_key(head)
    }

    fn resolve_optional_key(&self, head: &Head) -> Result<Option<String>, ProjectLoadError> {
        if head.predicate.module.is_some() {
            let key = head.predicate.display_name();
            if self.schemas.contains_key(&key) {
                return Ok(Some(key));
            }
            return Ok(None);
        }

        self.resolve_unqualified_key(head)
    }

    fn resolve_unqualified_key(&self, head: &Head) -> Result<Option<String>, ProjectLoadError> {
        let local = head.predicate.name.as_str();
        let Some(matches) = self.by_local.get(local) else {
            return Ok(None);
        };
        match matches.as_slice() {
            [key] => Ok(Some(key.clone())),
            [] => Ok(None),
            many => Err(ProjectLoadError::AmbiguousDiscoveryFact {
                name: local.to_string(),
                candidates: many.to_vec(),
                location: head.location.clone(),
            }),
        }
    }

    fn schema(&self, key: &str) -> Option<&ConfigKey> {
        self.schemas.get(key)
    }
}

fn is_project_rule_file(location: &crate::runtime::ast::SourceLocation) -> bool {
    location.source_name == PROJECT_RULE_FILE
}

fn config_entries(schema: &ConfigKey, head: &Head) -> Result<Vec<ConfigEntry>, ProjectLoadError> {
    let mut values = Vec::new();
    for term in &head.terms {
        let Some(value) = config_value(term) else {
            return Err(ProjectLoadError::NonLiteralDiscoveryValue {
                key: schema.key().to_string(),
                location: head.location.clone(),
            });
        };
        values.push(value);
    }

    discovery_entries(schema, values)
}

fn config_block_entries(
    block: &crate::runtime::ast::ConfigBlock,
) -> Result<Vec<ConfigEntry>, ProjectLoadError> {
    let mut entries = Vec::new();
    for declaration in &block.declarations {
        let section = block.section.as_str();
        let name = declaration.name.as_str();
        let Some(schema) = runtime_config_declaration_for(section, name) else {
            return Err(ProjectLoadError::UnknownConfigDeclaration {
                section: section.to_string(),
                key: name.to_string(),
                location: declaration.location.clone(),
            });
        };
        if schema.lifecycle() == RuntimeConfigLifecycle::ObsoleteConfirmedNamespace {
            return Err(ProjectLoadError::ObsoleteConfirmedNamespaceConfig {
                location: declaration.location.clone(),
            });
        }
        let values = declaration_values(declaration)?;
        entries.extend(schema.entries(values)?);
    }
    Ok(entries)
}

fn declaration_entries(
    schema: &ConfigKey,
    declaration: &Declaration,
) -> Result<Vec<ConfigEntry>, ProjectLoadError> {
    if let Some(arg) = declaration
        .args
        .iter()
        .find(|arg| matches!(arg, CallArg::Named { .. }))
    {
        return Err(ProjectLoadError::NamedDiscoveryArgument {
            key: schema.key().to_string(),
            location: arg.location().clone(),
        });
    }
    let values = declaration_values(declaration)?;
    discovery_entries(schema, values)
}

fn discovery_entries(
    schema: &ConfigKey,
    values: Vec<String>,
) -> Result<Vec<ConfigEntry>, ProjectLoadError> {
    validate_discovery_shape(schema, values.len())?;
    if values.len() <= 1 {
        return Ok(vec![ConfigEntry::scalar(
            schema.key().to_string(),
            values.into_iter().next().unwrap_or_default(),
        )]);
    }
    ordered_entries(schema.key(), values)
}

fn ordered_entries(key: &str, values: Vec<String>) -> Result<Vec<ConfigEntry>, ProjectLoadError> {
    values
        .into_iter()
        .enumerate()
        .map(|(idx, value)| {
            let ordinal = u32::try_from(idx)
                .map_err(|_| ProjectLoadError::OrderedConfigIndexOverflow { key: key.into() })?;
            Ok(ConfigEntry::ordered(key, value, ordinal))
        })
        .collect()
}

fn validate_discovery_shape(schema: &ConfigKey, actual: usize) -> Result<(), ProjectLoadError> {
    let ok = match schema.shape() {
        ConfigValueShape::Any => true,
        ConfigValueShape::Exactly(expected) => actual == expected,
        ConfigValueShape::AtLeast(minimum) => actual >= minimum,
    };
    if ok {
        return Ok(());
    }
    let expected = match schema.shape() {
        ConfigValueShape::Any => "any number of values",
        ConfigValueShape::Exactly(1) => "exactly one value",
        ConfigValueShape::Exactly(_) => "an exact tuple arity",
        ConfigValueShape::AtLeast(1) => "one or more values",
        ConfigValueShape::AtLeast(_) => "a minimum tuple arity",
    };
    Err(ProjectLoadError::InvalidDiscoveryArity {
        key: schema.key().to_string(),
        expected,
        actual,
    })
}

fn declaration_values(declaration: &Declaration) -> Result<Vec<String>, ProjectLoadError> {
    let mut values = Vec::new();
    for arg in &declaration.args {
        let literal =
            literal_arg(arg).ok_or_else(|| ProjectLoadError::NonLiteralDeclarationValue {
                name: declaration.name.to_string(),
                location: arg.location().clone(),
            })?;
        push_literal_values(&mut values, literal);
    }
    Ok(values)
}

fn literal_arg(arg: &CallArg) -> Option<&Literal> {
    match arg.expr()? {
        Expr::Literal(literal) => Some(literal),
        _ => None,
    }
}

fn push_literal_values(out: &mut Vec<String>, literal: &Literal) {
    match literal {
        Literal::List(items) => {
            for item in items {
                push_literal_values(out, item);
            }
        }
        _ => out.push(literal_value(literal)),
    }
}

fn config_value(term: &Term) -> Option<String> {
    let Term::Expr(Expr::Literal(literal)) = term else {
        return None;
    };
    Some(match literal {
        Literal::String(_) | Literal::Number(_) | Literal::Bool(_) | Literal::Null => {
            literal_value(literal)
        }
        Literal::List(_) => return None,
    })
}

fn literal_value(literal: &Literal) -> String {
    match literal {
        Literal::String(value) => value.clone(),
        Literal::Number(NumberLiteral::Int(value)) => value.to_string(),
        Literal::Number(NumberLiteral::Float(value)) => value.to_string(),
        Literal::Bool(value) => value.to_string(),
        Literal::Null => "null".to_string(),
        Literal::List(_) => unreachable!("list values are flattened before scalar conversion"),
    }
}

fn validate_verbs(program: &Program, base_program: &Program) -> Result<(), ProjectLoadError> {
    let mut query_programs = Vec::new();
    collect_verb_query_programs(&program.statements, false, &mut query_programs)?;

    let mut combined = base_program.clone();
    combined.statements.extend(program.statements.clone());
    for query_program in query_programs {
        combined.statements.extend(query_program.statements);
    }
    analyze(combined).map_err(|source| ProjectLoadError::VerbQueriesStatic {
        source: Box::new(source),
    })?;
    Ok(())
}

fn collect_verb_query_programs(
    statements: &[Statement],
    inside_at_block: bool,
    out: &mut Vec<Program>,
) -> Result<(), ProjectLoadError> {
    for statement in statements {
        match statement {
            Statement::Verb(verb) if inside_at_block => {
                return Err(ProjectLoadError::VerbInsideAtBlock {
                    location: verb.location().clone(),
                });
            }
            Statement::Verb(verb) => {
                out.push(
                    validate_project_verb_query_program(verb).map_err(ProjectLoadError::from)?,
                );
            }
            Statement::AtBlock { statements, .. } => {
                collect_verb_query_programs(statements, true, out)?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectLoadError {
    #[error(transparent)]
    Load(#[from] LoadError),
    #[error("{location}: unknown discovery fact '{key}'")]
    UnknownDiscoveryKey {
        key: String,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("{location}: ambiguous discovery fact '{name}' ({candidates:?})")]
    AmbiguousDiscoveryFact {
        name: String,
        candidates: Vec<String>,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("{location}: discovery fact '{key}' values must be literal scalars")]
    NonLiteralDiscoveryValue {
        key: String,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("discovery declaration '{key}' expects {expected}; got {actual} values")]
    InvalidDiscoveryArity {
        key: String,
        expected: &'static str,
        actual: usize,
    },
    #[error("{location}: discovery declaration '{key}' does not support named arguments yet")]
    NamedDiscoveryArgument {
        key: String,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("discovery facts contain {0}")]
    DuplicateDiscoveryOrdinal(DuplicateConfigOrdinal),
    #[error("runtime config declarations contain {0}")]
    DuplicateRuntimeConfigOrdinal(DuplicateConfigOrdinal),
    #[error("{location}: unknown config declaration 'config {section} {{ {key}(...) }}'")]
    UnknownConfigDeclaration {
        section: String,
        key: String,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error(
        "{location}: config handles confirmed(...) is no longer valid. Label namespaces are inferred automatically; delete this declaration, or use config handles force([...]) only for sparse prefixes that need an explicit override. Run `anneal init --dry-run` to preview the repaired config"
    )]
    ObsoleteConfirmedNamespaceConfig {
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("{location}: declaration '{name}' values must be static literals")]
    NonLiteralDeclarationValue {
        name: String,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("config declaration '{key}' expects {expected}; got {actual} values")]
    InvalidConfigArity {
        key: String,
        expected: &'static str,
        actual: usize,
    },
    #[error("ordered config declaration '{key}' overflowed u32 ordinals")]
    OrderedConfigIndexOverflow { key: String },
    #[error("{location}: @verb missing string field '{field}'")]
    MissingVerbField {
        field: String,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("{location}: invalid verb name '{name}'")]
    InvalidVerbName {
        name: String,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("{location}: @verb output_schema must be valid JSON: {source}")]
    InvalidVerbSchema {
        location: crate::runtime::ast::SourceLocation,
        source: serde_json::Error,
    },
    #[error("{location}: @verb output_schema must be a JSON object")]
    VerbSchemaMustBeObject {
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("{location}: @verb '{name}' query must parse: {source}")]
    VerbQueryParse {
        name: String,
        location: crate::runtime::ast::SourceLocation,
        source: Box<ParseError>,
    },
    #[error("{location}: @verb '{name}' must contain exactly one query, found {count}")]
    VerbQueryCount {
        name: String,
        count: usize,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("{location}: @verb must not be declared inside at() blocks")]
    VerbInsideAtBlock {
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("{location}: @verb field '{field}' must be a list of strings")]
    InvalidVerbListField {
        field: String,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("{location}: @verb args entry '{spec}' must use name:Type or name:Type=default")]
    InvalidVerbArgSpec {
        spec: String,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("{location}: duplicate @verb arg '{name}'")]
    DuplicateVerbArg {
        name: String,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("{location}: verb_arg is reserved for runtime verb invocation arguments")]
    ReservedVerbArgDefinition {
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("{location}: @verb '{name}' has invalid verb_arg reference: {message}")]
    InvalidVerbArgReference {
        name: String,
        message: String,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error(
        "{location}: @verb '{name}' references undeclared argument '{arg}'; declared args: {expected:?}"
    )]
    UnknownVerbArgReference {
        name: String,
        arg: String,
        expected: Vec<String>,
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("{location}: @verb output_schema uses an unsupported shape")]
    UnsupportedVerbSchema {
        location: crate::runtime::ast::SourceLocation,
    },
    #[error("@verb query failed static analysis: {source}")]
    VerbQueriesStatic { source: Box<StaticError> },
    #[error(
        "{location}: @verb '{name}' output_schema fields {expected:?} do not match query fields {actual:?}"
    )]
    VerbSchemaMismatch {
        name: String,
        expected: Vec<String>,
        actual: Vec<String>,
        location: crate::runtime::ast::SourceLocation,
    },
}

impl From<RuntimeConfigEntryError> for ProjectLoadError {
    fn from(source: RuntimeConfigEntryError) -> Self {
        match source {
            RuntimeConfigEntryError::InvalidArity {
                key,
                expected,
                actual,
            } => Self::InvalidConfigArity {
                key,
                expected,
                actual,
            },
            RuntimeConfigEntryError::OrderedConfigIndexOverflow { key } => {
                Self::OrderedConfigIndexOverflow { key }
            }
            RuntimeConfigEntryError::MissingLowering { key } => Self::InvalidConfigArity {
                key,
                expected: "a schema-backed fact lowering",
                actual: 0,
            },
            RuntimeConfigEntryError::Obsolete(_) => Self::ObsoleteConfirmedNamespaceConfig {
                location: SourceLocation::unknown(),
            },
        }
    }
}

impl From<VerbRegistryError> for ProjectLoadError {
    fn from(value: VerbRegistryError) -> Self {
        match value {
            VerbRegistryError::MissingField { field, location } => {
                Self::MissingVerbField { field, location }
            }
            VerbRegistryError::InvalidNameAt { name, location }
            | VerbRegistryError::DuplicateInLayer { name, location, .. } => {
                Self::InvalidVerbName { name, location }
            }
            VerbRegistryError::InvalidName { name } => Self::InvalidVerbName {
                name,
                location: crate::runtime::ast::SourceLocation::unknown(),
            },
            VerbRegistryError::InvalidSchema { location, source } => {
                Self::InvalidVerbSchema { location, source }
            }
            VerbRegistryError::SchemaMustBeObject { location } => {
                Self::VerbSchemaMustBeObject { location }
            }
            VerbRegistryError::QueryParse {
                name,
                location,
                source,
            } => Self::VerbQueryParse {
                name,
                location,
                source,
            },
            VerbRegistryError::QueryCount {
                name,
                count,
                location,
            } => Self::VerbQueryCount {
                name,
                count,
                location,
            },
            VerbRegistryError::VerbInsideAtBlock { location } => {
                Self::VerbInsideAtBlock { location }
            }
            VerbRegistryError::InvalidListField { field, location } => {
                Self::InvalidVerbListField { field, location }
            }
            VerbRegistryError::InvalidArgSpec { spec, location } => {
                Self::InvalidVerbArgSpec { spec, location }
            }
            VerbRegistryError::DuplicateArg { name, location } => {
                Self::DuplicateVerbArg { name, location }
            }
            VerbRegistryError::ReservedArgDefinition { location } => {
                Self::ReservedVerbArgDefinition { location }
            }
            VerbRegistryError::InvalidArgReference {
                name,
                location,
                message,
            } => Self::InvalidVerbArgReference {
                name,
                message,
                location,
            },
            VerbRegistryError::UnknownArgReference {
                name,
                arg,
                expected,
                location,
            } => Self::UnknownVerbArgReference {
                name,
                arg,
                expected,
                location,
            },
            VerbRegistryError::UnsupportedSchema { location } => {
                Self::UnsupportedVerbSchema { location }
            }
            VerbRegistryError::ArgFactParse { location, source } => Self::VerbQueryParse {
                name: "@verb args".to_string(),
                location,
                source,
            },
            VerbRegistryError::SchemaMismatch {
                name,
                expected,
                actual,
                location,
            } => Self::VerbSchemaMismatch {
                name,
                expected,
                actual,
                location,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::runtime::prelude::standard_prelude_program;
    use crate::runtime::{Database, Evaluator, Value, analyze, parse_program};
    use crate::source::{ConfigKey, Pattern, SourceCapabilities};

    fn source(name: &'static str, keys: Vec<ConfigKey>) -> SourceInfo {
        SourceInfo {
            name,
            recognizes: vec![Pattern::new("**/*")],
            doc: "test source",
            config_keys: keys,
            capabilities: SourceCapabilities::default(),
            search: None,
        }
    }

    fn write_project(root: &Path, source: &str) {
        fs::write(root.join(PROJECT_RULE_FILE), source).expect("write anneal.dl");
    }

    #[test]
    fn single_adapter_sugar_qualifies_discovery_facts() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            file_extension(".md").
            label_pattern("OQ", "OQ-(\\d+)", "any").
            project_seed("x").
            "#,
        );
        let sources = [source(
            "markdown",
            vec![
                ConfigKey::required("md.file_extension"),
                ConfigKey::optional("md.label_pattern"),
            ],
        )];

        let extension =
            load_project_extension(root.path(), &sources, &standard_prelude_program().unwrap())
                .expect("project loads");

        assert_eq!(
            extension.discovery().entries(),
            &[
                ConfigEntry::scalar("md.file_extension", ".md"),
                ConfigEntry::ordered("md.label_pattern", "OQ", 0),
                ConfigEntry::ordered("md.label_pattern", r"OQ-(\d+)", 1),
                ConfigEntry::ordered("md.label_pattern", "any", 2),
            ]
        );
        assert_eq!(extension.program().facts().count(), 1);
    }

    #[test]
    fn project_config_blocks_lower_to_runtime_config_facts() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            config convergence {
              ordering(["raw", "draft", "current"]).
              active(["draft", "current"]).
              terminal("archived").
            }

            config handles {
              force(["REQ"]).
              linear(["OQ"]).
            }

            config frontmatter {
              field("depends-on", "DependsOn", "forward").
            }

            config concerns {
              group("release", ["REQ", "REL"]).
            }
            "#,
        );

        let extension =
            load_project_extension(root.path(), &[], &standard_prelude_program().unwrap())
                .expect("project loads");

        assert_eq!(
            extension.runtime_config().entries(),
            &[
                ConfigEntry::ordered("convergence.ordering", "raw", 0),
                ConfigEntry::ordered("convergence.ordering", "draft", 1),
                ConfigEntry::ordered("convergence.ordering", "current", 2),
                ConfigEntry::scalar("convergence.active", "draft"),
                ConfigEntry::scalar("convergence.active", "current"),
                ConfigEntry::scalar("convergence.terminal", "archived"),
                ConfigEntry::scalar("handles.force", "REQ"),
                ConfigEntry::scalar("handles.linear", "OQ"),
                ConfigEntry::scalar("frontmatter.field.depends-on.edge_kind", "DependsOn"),
                ConfigEntry::scalar("frontmatter.field.depends-on.direction", "forward"),
                ConfigEntry::scalar("concerns.group.release", "REQ"),
                ConfigEntry::scalar("concerns.group.release", "REL"),
            ]
        );
        assert!(extension.program().statements.is_empty());
    }

    #[test]
    fn project_source_blocks_lower_to_adapter_discovery_facts() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            source md {
              file_extension(".md").
              scan_root("notes").
              scan_exclude("archive").
              label_pattern("OQ", "OQ-(\\d+)", "any").
            }
            "#,
        );
        let sources = [source(
            "markdown",
            vec![
                ConfigKey::required("md.file_extension"),
                ConfigKey::required("md.scan_root"),
                ConfigKey::optional("md.scan_exclude"),
                ConfigKey::optional_exact("md.label_pattern", 3),
            ],
        )];

        let extension =
            load_project_extension(root.path(), &sources, &standard_prelude_program().unwrap())
                .expect("project loads");

        assert_eq!(
            extension.discovery().entries(),
            &[
                ConfigEntry::scalar("md.file_extension", ".md"),
                ConfigEntry::scalar("md.scan_root", "notes"),
                ConfigEntry::scalar("md.scan_exclude", "archive"),
                ConfigEntry::ordered("md.label_pattern", "OQ", 0),
                ConfigEntry::ordered("md.label_pattern", r"OQ-(\d+)", 1),
                ConfigEntry::ordered("md.label_pattern", "any", 2),
            ]
        );
    }

    #[test]
    fn project_source_blocks_validate_adapter_declared_arity() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            source md {
              label_pattern("OQ", "OQ-(\\d+)").
            }
            "#,
        );
        let sources = [source(
            "markdown",
            vec![ConfigKey::optional_exact("md.label_pattern", 3)],
        )];

        let err = load_project_extension(root.path(), &sources, &Program::new(Vec::new()))
            .expect_err("bad source declaration arity rejected");

        assert!(matches!(
            err,
            ProjectLoadError::InvalidDiscoveryArity { .. }
        ));
    }

    #[test]
    fn project_source_blocks_reject_named_arguments_until_source_schemas_name_fields() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            source md {
              label_pattern("OQ", regex: "OQ-(\\d+)", scope: "any").
            }
            "#,
        );
        let sources = [source(
            "markdown",
            vec![ConfigKey::optional_exact("md.label_pattern", 3)],
        )];

        let err = load_project_extension(root.path(), &sources, &Program::new(Vec::new()))
            .expect_err("named source declaration args rejected");

        assert!(matches!(
            err,
            ProjectLoadError::NamedDiscoveryArgument { .. }
        ));
    }

    #[test]
    fn project_config_blocks_reject_unknown_keys() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            config convergence {
              typo(["draft"]).
            }
            "#,
        );

        let err = load_project_extension(root.path(), &[], &standard_prelude_program().unwrap())
            .expect_err("unknown config rejected");

        assert!(matches!(
            err,
            ProjectLoadError::UnknownConfigDeclaration { .. }
        ));
    }

    #[test]
    fn project_config_blocks_reject_confirmed_with_upgrade_hint() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            config handles {
              confirmed(["OQ"]).
            }
            "#,
        );

        let err = load_project_extension(root.path(), &[], &standard_prelude_program().unwrap())
            .expect_err("obsolete confirmed rejected");

        assert!(matches!(
            err,
            ProjectLoadError::ObsoleteConfirmedNamespaceConfig { .. }
        ));
        let msg = err.to_string();
        assert!(msg.contains("confirmed(...) is no longer valid"), "{msg}");
        assert!(msg.contains("anneal init --dry-run"), "{msg}");
    }

    #[test]
    fn multi_adapter_unqualified_discovery_fact_errors() {
        let root = tempdir().expect("tempdir");
        write_project(root.path(), r#"file_extension(".md")."#);
        let sources = [
            source("markdown", vec![ConfigKey::required("md.file_extension")]),
            source("mdx", vec![ConfigKey::required("mdx.file_extension")]),
        ];

        let err = load_project_extension(root.path(), &sources, &Program::new(Vec::new()))
            .expect_err("ambiguous discovery fact rejected");
        assert!(matches!(
            err,
            ProjectLoadError::AmbiguousDiscoveryFact { .. }
        ));
    }

    #[test]
    fn optional_unknown_adapter_discovery_fact_is_skipped() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            optional code.module_pattern("**/*.rs").
            md.file_extension(".md").
            "#,
        );
        let sources = [source(
            "markdown",
            vec![ConfigKey::required("md.file_extension")],
        )];

        let extension =
            load_project_extension(root.path(), &sources, &standard_prelude_program().unwrap())
                .expect("project loads");
        assert_eq!(
            extension.discovery().entries(),
            &[ConfigEntry::scalar("md.file_extension", ".md")]
        );
    }

    #[test]
    fn required_unknown_adapter_discovery_fact_errors() {
        let root = tempdir().expect("tempdir");
        write_project(root.path(), r#"code.module_pattern("**/*.rs")."#);

        let err = load_project_extension(root.path(), &[], &Program::new(Vec::new()))
            .expect_err("unknown discovery key rejected");
        assert!(matches!(err, ProjectLoadError::UnknownDiscoveryKey { .. }));
    }

    #[test]
    fn imported_qualified_facts_are_not_discovery_facts() {
        let root = tempdir().expect("tempdir");
        write_project(root.path(), r#"import helper from "helper.dl"."#);
        fs::write(root.path().join("helper.dl"), r#"seed("x")."#).expect("write helper");
        let sources = [source(
            "markdown",
            vec![ConfigKey::required("md.file_extension")],
        )];

        let extension =
            load_project_extension(root.path(), &sources, &standard_prelude_program().unwrap())
                .expect("project loads");
        let names = extension
            .program()
            .facts()
            .map(|fact| fact.predicate.display_name())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["helper.seed"]);
        assert!(extension.discovery().entries().is_empty());
    }

    #[test]
    fn project_verb_validates_query_and_schema() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            project_seed("x").
            @verb(
              name: "project-seeds",
              query: "? project_seed(h).",
              doc: "List project seed facts.",
              output_schema: "{\"h\":\"String\"}",
              args: [],
              capabilities: []
            ).
            "#,
        );

        load_project_extension(root.path(), &[], &standard_prelude_program().unwrap())
            .expect("verb validates");
    }

    #[test]
    fn introspection_lists_prelude_and_project_verbs_together() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            project_seed("x").
            @verb(
              name: "project-seeds",
              query: "? project_seed(h).",
              doc: "List project seed facts.",
              output_schema: "{\"h\":\"String\"}",
              args: [],
              capabilities: []
            ).
            "#,
        );

        let mut program = standard_prelude_program().unwrap();
        let extension = load_project_extension(root.path(), &[], &program).expect("project loads");
        let extension = extension.into_parts();
        program.statements.extend(extension.program.statements);
        let query_program =
            parse_program("verbs-query", "? verbs(name, query, doc, output_schema).").unwrap();
        program.statements.extend(query_program.statements);

        let analyzed = analyze(program).expect("combined program analyzes");
        let query = analyzed.queries().next().cloned().expect("verbs query");
        let evaluator = Evaluator::new(analyzed, Database::default());
        let output = evaluator.eval_query(&query).expect("verbs evaluate");
        let names = output
            .rows
            .iter()
            .filter_map(|row| match row.fields.get("name") {
                Some(Value::String(name)) => Some(name.as_str()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();

        assert!(names.contains("verbs"));
        assert!(names.contains("project-seeds"));
    }

    #[test]
    fn introspection_verbs_use_resolved_registry_shadowing() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            project_seed("x").
            @verb(
              name: "work",
              query: "? project_seed(h).",
              doc: "Project work view.",
              output_schema: "{\"h\":\"String\"}",
              args: [],
              capabilities: []
            ).
            "#,
        );

        let mut program = standard_prelude_program().unwrap();
        let extension = load_project_extension(root.path(), &[], &program).expect("project loads");
        let extension = extension.into_parts();
        program.statements.extend(extension.program.statements);
        let query_program =
            parse_program("verbs-query", "? verbs(name, query, doc, output_schema).").unwrap();
        program.statements.extend(query_program.statements);

        let analyzed = analyze(program).expect("combined program analyzes");
        let query = analyzed.queries().next().cloned().expect("verbs query");
        let evaluator = Evaluator::new(analyzed, Database::default());
        let output = evaluator.eval_query(&query).expect("verbs evaluate");
        let work_rows = output
            .rows
            .iter()
            .filter(|row| row.fields.get("name") == Some(&Value::String("work".to_string())))
            .collect::<Vec<_>>();

        assert_eq!(work_rows.len(), 1);
        assert_eq!(
            work_rows[0].fields.get("query"),
            Some(&Value::String("? project_seed(h).".to_string()))
        );
    }

    #[test]
    fn merge_program_layers_replaces_earlier_predicate_definitions() {
        let base = parse_program(
            "base",
            r#"
            @doc(name: "blocked", doc: "base blocked").
            blocked("base").
            blocked(h) := active(h).
            active("x").
            "#,
        )
        .expect("base parses");
        let extension = parse_program(
            PROJECT_RULE_FILE,
            r#"
            @doc(name: "blocked", doc: "project blocked").
            blocked("project").
            "#,
        )
        .expect("extension parses");

        let (merged, warnings) = merge_program_layers(base, extension);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].predicate, "blocked");
        assert_eq!(warnings[0].replaced_clauses, 3);
        let blocked_count = merged
            .statements
            .iter()
            .filter(|statement| match statement {
                Statement::Fact(head) => head.predicate.display_name() == "blocked",
                Statement::Rule(rule) => rule.head.predicate.display_name() == "blocked",
                Statement::Doc(doc) => doc.name() == "blocked",
                _ => false,
            })
            .count();
        assert_eq!(blocked_count, 2);
        assert!(merged.facts().any(|head| {
            head.predicate.display_name() == "active"
                && matches!(
                    head.terms.as_slice(),
                    [Term::Expr(Expr::Literal(Literal::String(value)))] if value == "x"
                )
        }));
    }

    #[test]
    fn project_verb_bad_schema_errors_at_declaration() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            project_seed("x").
            @verb(
              name: "project-seeds",
              query: "? project_seed(h).",
              doc: "List project seed facts.",
              output_schema: "{\"other\":\"String\"}",
              args: [],
              capabilities: []
            ).
            "#,
        );

        let err = load_project_extension(root.path(), &[], &standard_prelude_program().unwrap())
            .expect_err("schema mismatch rejected");
        assert!(matches!(err, ProjectLoadError::VerbSchemaMismatch { .. }));
    }

    #[test]
    fn project_verb_missing_doc_is_rejected() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            project_seed("x").
            @verb(
              name: "project-seeds",
              query: "? project_seed(h).",
              output_schema: "{\"h\":\"String\"}",
              args: [],
              capabilities: []
            ).
            "#,
        );

        let err = load_project_extension(root.path(), &[], &standard_prelude_program().unwrap())
            .expect_err("missing doc rejected");
        assert!(matches!(
            err,
            ProjectLoadError::MissingVerbField { field, .. } if field == "doc"
        ));
    }

    #[test]
    fn project_verb_unsupported_schema_shape_is_rejected() {
        let root = tempdir().expect("tempdir");
        write_project(
            root.path(),
            r#"
            project_seed("x").
            @verb(
              name: "project-seeds",
              query: "? project_seed(h).",
              doc: "List project seed facts.",
              output_schema: "{\"h\":[\"String\"]}",
              args: [],
              capabilities: []
            ).
            "#,
        );

        let err = load_project_extension(root.path(), &[], &standard_prelude_program().unwrap())
            .expect_err("unsupported schema rejected");
        assert!(matches!(
            err,
            ProjectLoadError::UnsupportedVerbSchema { .. }
        ));
    }

    #[test]
    fn optional_fact_outside_project_loader_is_rejected_by_analysis() {
        let program = parse_program("inline", r#"optional md.file_extension(".md")."#)
            .expect("optional fact parses");

        let err = analyze(program).expect_err("optional discovery rejected");
        assert!(matches!(err, StaticError::OptionalDiscoveryFact { .. }));
    }
}
