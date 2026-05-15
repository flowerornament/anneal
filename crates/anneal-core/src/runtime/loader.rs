use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::runtime::ast::{
    Atom, Body, Head, Ident, ImportDirective, IncludeDirective, NegatedAtom, PredicateRef, Program,
    Query, Rule, RuleLayer, SourceLocation, Statement, TimeBlock,
};
use crate::runtime::parser::{ParseError, parse_program};

pub fn load_program(root: impl AsRef<Path>, entry: impl AsRef<Path>) -> Result<Program, LoadError> {
    ProgramLoader::new(root)?.load(entry)
}

pub fn load_prelude(root: impl AsRef<Path>, entry: impl AsRef<Path>) -> Result<Program, LoadError> {
    ProgramLoader::new(root)?.load_prelude(entry)
}

#[derive(Clone, Debug)]
pub struct ProgramLoader {
    root: PathBuf,
}

impl ProgramLoader {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, LoadError> {
        let root = canonicalize_root(root.as_ref())?;
        Ok(Self { root })
    }

    pub fn load(&self, entry: impl AsRef<Path>) -> Result<Program, LoadError> {
        self.load_in_scope(entry, &LoadScope::Project)
    }

    pub fn load_prelude(&self, entry: impl AsRef<Path>) -> Result<Program, LoadError> {
        self.load_in_scope(entry, &LoadScope::Prelude)
    }

    fn load_in_scope(
        &self,
        entry: impl AsRef<Path>,
        scope: &LoadScope,
    ) -> Result<Program, LoadError> {
        let location = SourceLocation::new(entry.as_ref().display().to_string(), 0, 0);
        let entry = self.resolve_path(&self.root, entry.as_ref(), &location)?;
        let mut context = LoadContext::default();
        let statements = self.load_file(&entry, scope, &mut context, &location)?;
        Ok(Program { statements })
    }

    fn load_file(
        &self,
        path: &Path,
        scope: &LoadScope,
        context: &mut LoadContext,
        requested_at: &SourceLocation,
    ) -> Result<Vec<Statement>, LoadError> {
        let key = LoadKey {
            path: path.to_path_buf(),
            scope: scope.clone(),
        };
        if context.active.iter().any(|active| active == &key) {
            let mut chain = context
                .active
                .iter()
                .map(|active| active.path.clone())
                .collect::<Vec<_>>();
            chain.push(path.to_path_buf());
            return Err(LoadError::IncludeCycle {
                location: requested_at.clone(),
                path: path.to_path_buf(),
                chain: chain.into(),
            });
        }
        if !context.loaded.insert(key.clone()) {
            return Ok(Vec::new());
        }

        context.active.push(key);
        let input = fs::read_to_string(path).map_err(|source| LoadError::Io {
            location: requested_at.clone(),
            path: path.to_path_buf(),
            source,
        })?;
        let parsed = parse_program(&self.source_name(path), &input)?;
        let base = path.parent().unwrap_or(&self.root);
        let mut statements = Vec::new();

        for statement in parsed.statements {
            match statement {
                Statement::Include(directive) => {
                    let child = self.resolve_directive_path(base, &directive)?;
                    statements.extend(self.load_file(
                        &child,
                        scope,
                        context,
                        &directive.location,
                    )?);
                }
                Statement::Import(directive) => {
                    statements.extend(self.load_import(base, &directive, context)?);
                }
                mut statement => {
                    statement.assign_rule_layer(scope.layer());
                    statements.push(statement);
                }
            }
        }

        context.active.pop();
        Ok(statements)
    }

    fn load_import(
        &self,
        base: &Path,
        directive: &ImportDirective,
        context: &mut LoadContext,
    ) -> Result<Vec<Statement>, LoadError> {
        let child = self.resolve_import_path(base, directive)?;
        let scope = LoadScope::Import(directive.module.clone());
        let mut statements = self.load_file(&child, &scope, context, &directive.location)?;
        let local_definitions = collect_unqualified_definitions(&statements);
        qualify_statements(&mut statements, &directive.module, &local_definitions);
        Ok(statements)
    }

    fn resolve_directive_path(
        &self,
        base: &Path,
        directive: &IncludeDirective,
    ) -> Result<PathBuf, LoadError> {
        self.resolve_path(base, Path::new(&directive.path), &directive.location)
    }

    fn resolve_import_path(
        &self,
        base: &Path,
        directive: &ImportDirective,
    ) -> Result<PathBuf, LoadError> {
        self.resolve_path(base, Path::new(&directive.path), &directive.location)
    }

    fn resolve_path(
        &self,
        base: &Path,
        requested: &Path,
        location: &SourceLocation,
    ) -> Result<PathBuf, LoadError> {
        let raw = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            base.join(requested)
        };
        let path = raw.canonicalize().map_err(|source| LoadError::Io {
            location: location.clone(),
            path: raw.clone(),
            source,
        })?;
        if !path.starts_with(&self.root) {
            return Err(LoadError::PathEscapesRoot {
                location: location.clone(),
                path,
                root: self.root.clone(),
            });
        }
        Ok(path)
    }

    fn source_name(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }
}

#[derive(Clone, Debug, Default)]
struct LoadContext {
    active: Vec<LoadKey>,
    loaded: BTreeSet<LoadKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct LoadKey {
    path: PathBuf,
    scope: LoadScope,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LoadScope {
    Prelude,
    Project,
    Import(Ident),
}

impl LoadScope {
    fn layer(&self) -> RuleLayer {
        match self {
            Self::Prelude => RuleLayer::Prelude,
            Self::Project => RuleLayer::Project,
            Self::Import(_) => RuleLayer::Import,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{location}: {path:?}: {source}")]
    Io {
        location: SourceLocation,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{location}: {path:?} escapes rule root {root:?}")]
    PathEscapesRoot {
        location: SourceLocation,
        path: PathBuf,
        root: PathBuf,
    },
    #[error("{location}: include cycle: {chain}")]
    IncludeCycle {
        location: SourceLocation,
        path: PathBuf,
        chain: IncludeCycle,
    },
    #[error(transparent)]
    Parse(#[from] ParseError),
}

#[derive(Debug)]
pub struct IncludeCycle(Vec<PathBuf>);

impl From<Vec<PathBuf>> for IncludeCycle {
    fn from(value: Vec<PathBuf>) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for IncludeCycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (idx, path) in self.0.iter().enumerate() {
            if idx > 0 {
                f.write_str(" -> ")?;
            }
            write!(f, "{}", path.display())?;
        }
        Ok(())
    }
}

fn canonicalize_root(root: &Path) -> Result<PathBuf, LoadError> {
    root.canonicalize().map_err(|source| LoadError::Io {
        location: SourceLocation::new(root.display().to_string(), 0, 0),
        path: root.to_path_buf(),
        source,
    })
}

fn collect_unqualified_definitions(statements: &[Statement]) -> BTreeSet<Ident> {
    let mut definitions = BTreeSet::new();
    for statement in statements {
        collect_statement_definitions(statement, &mut definitions);
    }
    definitions
}

fn collect_statement_definitions(statement: &Statement, definitions: &mut BTreeSet<Ident>) {
    match statement {
        Statement::Fact(head) => collect_head_definition(head, definitions),
        Statement::Rule(rule) => collect_head_definition(&rule.head, definitions),
        Statement::Query(query) => {
            for rule in &query.local_rules {
                collect_head_definition(&rule.head, definitions);
            }
        }
        Statement::AtBlock { statements, .. } => {
            for statement in statements {
                collect_statement_definitions(statement, definitions);
            }
        }
        Statement::Include(_) | Statement::Import(_) | Statement::Verb(_) | Statement::Doc(_) => {}
    }
}

fn collect_head_definition(head: &Head, definitions: &mut BTreeSet<Ident>) {
    if head.predicate.module.is_none() {
        definitions.insert(head.predicate.name.clone());
    }
}

fn qualify_statements(
    statements: &mut [Statement],
    module: &Ident,
    local_definitions: &BTreeSet<Ident>,
) {
    for statement in statements {
        qualify_statement(statement, module, local_definitions);
    }
}

fn qualify_statement(
    statement: &mut Statement,
    module: &Ident,
    local_definitions: &BTreeSet<Ident>,
) {
    match statement {
        Statement::Fact(head) => qualify_head(head, module, local_definitions),
        Statement::Rule(rule) => qualify_rule(rule, module, local_definitions),
        Statement::Query(query) => qualify_query(query, module, local_definitions),
        Statement::AtBlock { statements, .. } => {
            qualify_statements(statements, module, local_definitions);
        }
        Statement::Include(_) | Statement::Import(_) | Statement::Verb(_) | Statement::Doc(_) => {}
    }
}

fn qualify_query(query: &mut Query, module: &Ident, local_definitions: &BTreeSet<Ident>) {
    for rule in &mut query.local_rules {
        qualify_rule(rule, module, local_definitions);
    }
    qualify_body(&mut query.body, module, local_definitions);
}

fn qualify_rule(rule: &mut Rule, module: &Ident, local_definitions: &BTreeSet<Ident>) {
    qualify_head(&mut rule.head, module, local_definitions);
    qualify_body(&mut rule.body, module, local_definitions);
}

fn qualify_head(head: &mut Head, module: &Ident, local_definitions: &BTreeSet<Ident>) {
    qualify_predicate(&mut head.predicate, module, local_definitions);
}

fn qualify_body(body: &mut Body, module: &Ident, local_definitions: &BTreeSet<Ident>) {
    for atom in &mut body.atoms {
        qualify_atom(atom, module, local_definitions);
    }
}

fn qualify_atom(atom: &mut Atom, module: &Ident, local_definitions: &BTreeSet<Ident>) {
    match atom {
        Atom::Derived(derived) => {
            qualify_predicate(&mut derived.predicate, module, local_definitions);
        }
        Atom::Negation(negation) => {
            if let NegatedAtom::Derived(derived) = &mut negation.atom {
                qualify_predicate(&mut derived.predicate, module, local_definitions);
            }
        }
        Atom::Aggregation(aggregate) => {
            qualify_body(&mut aggregate.body, module, local_definitions);
        }
        Atom::TimeBlock(TimeBlock { body, .. }) => qualify_body(body, module, local_definitions),
        Atom::Stored(_) | Atom::Comparison(_) => {}
    }
}

fn qualify_predicate(
    predicate: &mut PredicateRef,
    module: &Ident,
    local_definitions: &BTreeSet<Ident>,
) {
    if predicate.module.is_none() && local_definitions.contains(&predicate.name) {
        *predicate = PredicateRef::qualified(module.clone(), predicate.name.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::analysis::{StaticError, analyze};
    use crate::runtime::ast::{Expr, Literal, Term};

    #[test]
    fn load_include_and_import_resolves_predicates() {
        let root = test_root("include-import");
        write(
            &root.join("anneal.dl"),
            r#"
            include "checks/global.dl".
            import strict from "checks/strict.dl".
            ? global(h), strict.blocker(h).
            "#,
        );
        fs::create_dir_all(root.join("checks")).expect("checks dir");
        write(
            &root.join("checks/global.dl"),
            r"global(h) := *handle{id: h}.",
        );
        write(
            &root.join("checks/strict.dl"),
            r"
            blocker(h) := helper(h).
            helper(h) := *handle{id: h}.
            ",
        );

        let program = load_program(&root, "anneal.dl").expect("program loads");
        let analyzed = analyze(program).expect("program analyzes");
        let names = analyzed
            .predicates()
            .map(PredicateRef::display_name)
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "global"));
        assert!(names.iter().any(|name| name == "strict.blocker"));
        assert!(names.iter().any(|name| name == "strict.helper"));
    }

    #[test]
    fn import_preserves_references_to_global_predicates() {
        let root = test_root("import-global-reference");
        write(
            &root.join("anneal.dl"),
            r#"
            active(h) := *handle{id: h}.
            import strict from "strict.dl".
            ? strict.blocker(h).
            "#,
        );
        write(&root.join("strict.dl"), r"blocker(h) := active(h).");

        let program = load_program(&root, "anneal.dl").expect("program loads");
        analyze(program).expect("imported rule can call global predicate");
    }

    #[test]
    fn loaded_rules_preserve_child_file_source_locations() {
        let root = test_root("child-source-location");
        fs::create_dir_all(root.join("checks")).expect("checks dir");
        write(&root.join("anneal.dl"), r#"include "checks/bad.dl"."#);
        write(&root.join("checks/bad.dl"), r"bad(h) := missing(h).");

        let program = load_program(&root, "anneal.dl").expect("program loads");
        let err = analyze(program).expect_err("unknown predicate rejected");
        let StaticError::UnknownPredicate {
            location: actual, ..
        } = err
        else {
            panic!("expected unknown predicate");
        };
        assert_eq!(
            actual,
            SourceLocation::new("checks/bad.dl", 1, "bad(h) := ".len() + 1)
        );
    }

    #[test]
    fn diamond_includes_are_loaded_once_per_scope() {
        let root = test_root("diamond");
        write(
            &root.join("anneal.dl"),
            r#"
            include "left.dl".
            include "right.dl".
            ? shared(h).
            "#,
        );
        write(&root.join("left.dl"), r#"include "shared.dl"."#);
        write(&root.join("right.dl"), r#"include "shared.dl"."#);
        write(&root.join("shared.dl"), r"shared(h) := *handle{id: h}.");

        let program = load_program(&root, "anneal.dl").expect("program loads");
        assert_eq!(program.rules().count(), 1);
        analyze(program).expect("program analyzes");
    }

    #[test]
    fn prelude_loader_allows_reserved_diagnostic_ids() {
        let root = test_root("prelude");
        write(
            &root.join("checks.dl"),
            r#"diagnostic("E001", "error", h) := *handle{id: h}."#,
        );

        let program = load_prelude(&root, "checks.dl").expect("prelude loads");
        analyze(program).expect("prelude diagnostics analyze");
    }

    #[test]
    fn duplicate_diagnostic_id_beats_reserved_prefix_error() {
        let root = test_root("duplicate-prelude-project");
        write(
            &root.join("prelude.dl"),
            r#"diagnostic("E001", "error", h) := *handle{id: h}."#,
        );
        write(
            &root.join("project.dl"),
            r#"diagnostic("E001", "warning", h) := *handle{id: h}."#,
        );

        let mut program = load_prelude(&root, "prelude.dl").expect("prelude loads");
        program.statements.extend(
            load_program(&root, "project.dl")
                .expect("project loads")
                .statements,
        );
        let err = analyze(program).expect_err("duplicate id rejected");
        assert!(matches!(err, StaticError::DuplicateDiagnosticId { .. }));
    }

    #[test]
    fn rejects_include_cycles() {
        let root = test_root("cycle");
        write(&root.join("a.dl"), r#"include "b.dl"."#);
        write(&root.join("b.dl"), r#"include "a.dl"."#);

        let err = load_program(&root, "a.dl").expect_err("cycle rejected");
        assert!(matches!(err, LoadError::IncludeCycle { .. }));
    }

    #[test]
    fn rejects_paths_outside_rule_root() {
        let root = test_root("escape-root");
        let outside = root
            .parent()
            .expect("temp root has parent")
            .join("anneal-core-loader-outside.dl");
        write(&outside, r"escaped(h) := *handle{id: h}.");
        write(
            &root.join("anneal.dl"),
            r#"include "../anneal-core-loader-outside.dl"."#,
        );

        let err = load_program(&root, "anneal.dl").expect_err("escape rejected");
        assert!(matches!(err, LoadError::PathEscapesRoot { .. }));
        assert!(err.to_string().contains("anneal.dl:1:1"));
    }

    #[test]
    fn import_qualifies_diagnostic_rules() {
        let root = test_root("import-diagnostic");
        write(
            &root.join("anneal.dl"),
            r#"import strict from "strict.dl"."#,
        );
        write(
            &root.join("strict.dl"),
            r#"diagnostic("PROJ-001", "error", h) := *handle{id: h}."#,
        );

        let program = load_program(&root, "anneal.dl").expect("program loads");
        let rule = program.rules().next().expect("imported rule");
        assert_eq!(rule.head.predicate.display_name(), "strict.diagnostic");
        let Term::Expr(Expr::Literal(Literal::String(id))) = &rule.head.terms[0] else {
            panic!("expected diagnostic id literal");
        };
        assert_eq!(id, "PROJ-001");
    }

    fn test_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("anneal-core-loader-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove stale test root");
        }
        fs::create_dir_all(&root).expect("create test root");
        root
    }

    fn write(path: &Path, input: &str) {
        fs::write(path, input).expect("write test file");
    }
}
