use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::runtime::analysis::{StaticError, analyze};
use crate::runtime::ast::{
    Expr, Head, Literal, NumberLiteral, Program, SourceLocation, Statement, Term,
};
use crate::runtime::loader::{LoadError, load_program};
use crate::runtime::parser::ParseError;
use crate::source::{ConfigEntry, ConfigFacts, DuplicateConfigOrdinal, SourceInfo};
use crate::verbs::{VerbRegistryError, validate_project_verb_query_program};

pub const PROJECT_RULE_FILE: &str = "anneal.dl";

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectExtension {
    discovery: ConfigFacts,
    program: Program,
}

impl ProjectExtension {
    pub fn discovery(&self) -> &ConfigFacts {
        &self.discovery
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn into_parts(self) -> (ConfigFacts, Program) {
        (self.discovery, self.program)
    }
}

pub fn load_project_extension(
    root: impl AsRef<Path>,
    sources: &[SourceInfo],
    base_program: &Program,
) -> Result<ProjectExtension, ProjectLoadError> {
    let loaded = load_program(root, PROJECT_RULE_FILE)?;
    let (discovery, program) = split_project_program(loaded, sources)?;
    validate_verbs(&program, base_program)?;
    Ok(ProjectExtension { discovery, program })
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
        | Statement::Include(_)
        | Statement::Import(_)
        | Statement::AtBlock { .. }
        | Statement::Verb(_) => None,
    }
}

fn split_project_program(
    program: Program,
    sources: &[SourceInfo],
) -> Result<(ConfigFacts, Program), ProjectLoadError> {
    let resolver = DiscoveryResolver::new(sources);
    let mut discovery = Vec::new();
    let mut statements = Vec::new();

    for statement in program.statements {
        match statement {
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
    Ok((discovery, Program::new(statements)))
}

struct DiscoveryResolver {
    exact: BTreeSet<String>,
    by_local: BTreeMap<String, Vec<String>>,
}

impl DiscoveryResolver {
    fn new(sources: &[SourceInfo]) -> Self {
        let mut exact = BTreeSet::new();
        let mut by_local = BTreeMap::<String, Vec<String>>::new();
        for source in sources {
            for key in &source.config_keys {
                exact.insert(key.key.clone());
                if let Some((_, local)) = key.key.split_once('.') {
                    by_local
                        .entry(local.to_string())
                        .or_default()
                        .push(key.key.clone());
                }
            }
        }
        Self { exact, by_local }
    }

    fn entries_for_required_fact(
        &self,
        head: &Head,
    ) -> Result<Option<Vec<ConfigEntry>>, ProjectLoadError> {
        let Some(key) = self.resolve_required_key(head)? else {
            return Ok(None);
        };
        Ok(Some(config_entries(key, head)?))
    }

    fn entries_for_optional_fact(&self, head: &Head) -> Result<Vec<ConfigEntry>, ProjectLoadError> {
        let Some(key) = self.resolve_optional_key(head)? else {
            return Ok(Vec::new());
        };
        config_entries(key, head)
    }

    fn resolve_required_key(&self, head: &Head) -> Result<Option<String>, ProjectLoadError> {
        if head.predicate.module.is_some() {
            let key = head.predicate.display_name();
            if self.exact.contains(&key) {
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
            if self.exact.contains(&key) {
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
}

fn is_project_rule_file(location: &crate::runtime::ast::SourceLocation) -> bool {
    location.source_name == PROJECT_RULE_FILE
}

fn config_entries(key: String, head: &Head) -> Result<Vec<ConfigEntry>, ProjectLoadError> {
    let mut values = Vec::new();
    for term in &head.terms {
        let Some(value) = config_value(term) else {
            return Err(ProjectLoadError::NonLiteralDiscoveryValue {
                key,
                location: head.location.clone(),
            });
        };
        values.push(value);
    }

    if values.len() <= 1 {
        return Ok(vec![ConfigEntry::scalar(
            key,
            values.into_iter().next().unwrap_or_default(),
        )]);
    }

    Ok(values
        .into_iter()
        .enumerate()
        .map(|(idx, value)| {
            ConfigEntry::ordered(
                key.clone(),
                value,
                u32::try_from(idx).expect("config arity fits u32"),
            )
        })
        .collect())
}

fn config_value(term: &Term) -> Option<String> {
    let Term::Expr(Expr::Literal(literal)) = term else {
        return None;
    };
    Some(match literal {
        Literal::String(value) => value.clone(),
        Literal::Number(NumberLiteral::Int(value)) => value.to_string(),
        Literal::Number(NumberLiteral::Float(value)) => value.to_string(),
        Literal::Bool(value) => value.to_string(),
        Literal::Null => "null".to_string(),
        Literal::List(_) => return None,
    })
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
    #[error("discovery facts contain {0}")]
    DuplicateDiscoveryOrdinal(DuplicateConfigOrdinal),
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
            VerbRegistryError::UnsupportedSchema { location } => {
                Self::UnsupportedVerbSchema { location }
            }
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
              default_args: [],
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
              default_args: [],
              capabilities: []
            ).
            "#,
        );

        let mut program = standard_prelude_program().unwrap();
        let extension = load_project_extension(root.path(), &[], &program).expect("project loads");
        program
            .statements
            .extend(extension.into_parts().1.statements);
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
              default_args: [],
              capabilities: []
            ).
            "#,
        );

        let mut program = standard_prelude_program().unwrap();
        let extension = load_project_extension(root.path(), &[], &program).expect("project loads");
        program
            .statements
            .extend(extension.into_parts().1.statements);
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
              default_args: [],
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
              default_args: [],
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
              default_args: [],
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
