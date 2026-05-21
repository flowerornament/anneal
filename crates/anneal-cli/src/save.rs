use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io;

use anneal_core::runtime::prelude::datalog_string_literal;
use anneal_core::runtime::{
    Atom, Body, Expr, Ident, Literal, NegatedAtom, Program, Statement, VerbDecl, analyze,
    parse_program,
};
use anneal_core::{
    PROJECT_RULE_FILE, VerbLayer, VerbName, VerbRegistry, VerbSource,
    validate_project_verb_query_program,
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use camino::Utf8PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SaveCommand {
    pub(crate) name: String,
    pub(crate) query: String,
    pub(crate) args: Vec<String>,
    pub(crate) doc: String,
    pub(crate) force: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SaveOutcome {
    pub(crate) name: String,
    pub(crate) path: Utf8PathBuf,
    pub(crate) replaced: Option<String>,
    pub(crate) shadowed: Option<String>,
}

impl SaveCommand {
    pub(crate) fn run(
        &self,
        root: &camino::Utf8Path,
        base_program: &Program,
        registry: &VerbRegistry,
    ) -> Result<SaveOutcome> {
        let name = VerbName::new(self.name.clone())?;
        let args = parse_arg_specs(&self.args)?;
        let query = normalized_query(&self.query, &args)?;
        let output_schema = infer_output_schema(&query, &args)?;
        let declaration =
            render_verb_declaration(name.as_str(), &query, &self.doc, &output_schema, &args);
        let draft = parse_verb_declaration(&declaration)?;

        validate_verb(base_program, &draft)?;

        let existing = registry.get(name.as_str());
        if let Some(entry) = existing
            && !self.force
        {
            bail!(
                "verb '{}' already exists at {}; use --force to replace a project verb or shadow a prelude verb",
                name,
                format_verb_source(entry.source())
            );
        }

        let project_path = root.join(PROJECT_RULE_FILE);
        let _lock = ProjectFileLock::acquire(&project_path)?;
        let existing_text = read_project_rules(&project_path)?;
        let mut replaced = None;
        let mut shadowed = None;
        let updated = if let Some(entry) = existing {
            match entry.source().layer() {
                VerbLayer::Project => {
                    let span = find_verb_declaration_span(&existing_text, name.as_str())
                        .with_context(|| {
                            format!(
                                "verb '{}' is registered from project source {}, but its @verb block was not found in {}",
                                name,
                                format_verb_source(entry.source()),
                                project_path
                            )
                        })?;
                    replaced = Some(format_verb_source(entry.source()));
                    replace_span(&existing_text, span, &declaration)
                }
                VerbLayer::Prelude => {
                    shadowed = Some(format_verb_source(entry.source()));
                    append_declaration(&existing_text, &declaration)
                }
            }
        } else {
            append_declaration(&existing_text, &declaration)
        };
        write_atomic(&project_path, &updated)?;
        Ok(SaveOutcome {
            name: name.to_string(),
            path: project_path,
            replaced,
            shadowed,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SaveArgSpec {
    raw: String,
    name: String,
    kind: String,
}

fn parse_arg_specs(raw: &[String]) -> Result<Vec<SaveArgSpec>> {
    let mut args = Vec::new();
    let mut seen = BTreeSet::new();
    for spec in raw {
        let (name, rest) = spec
            .split_once(':')
            .ok_or_else(|| anyhow!("save --args entry {spec:?} must use name:Type"))?;
        let name = name.trim();
        ensure!(
            Ident::new(name).is_ok(),
            "save --args entry {spec:?} has invalid argument name {name:?}"
        );
        ensure!(
            seen.insert(name.to_string()),
            "save --args declares argument {name:?} more than once"
        );
        let kind = rest
            .split_once('=')
            .map_or(rest.trim(), |(kind, _default)| kind.trim());
        ensure!(
            matches!(kind, "String" | "HandleId" | "Int" | "Number" | "Bool"),
            "save --args entry {spec:?} uses unsupported type {kind:?}; expected String, HandleId, Int, Number, or Bool"
        );
        args.push(SaveArgSpec {
            raw: spec.clone(),
            name: name.to_string(),
            kind: kind.to_string(),
        });
    }
    Ok(args)
}

fn normalized_query(query_source: &str, args: &[SaveArgSpec]) -> Result<String> {
    let program = parse_program("anneal-save:query", query_source)
        .context("save query must parse before it can become a verb")?;
    let query = single_query(&program)?;
    let existing_arg_refs = verb_arg_references(query);
    let final_bindings = query.body.positive_binding_variables();
    let mut missing = Vec::new();
    for arg in args {
        if existing_arg_refs.contains(&arg.name) {
            continue;
        }
        let ident = Ident::new(arg.name.as_str()).expect("validated save arg name");
        ensure!(
            final_bindings.contains(&ident),
            "save argument '{}' is not bound in the final query; add `verb_arg(\"{}\", {})` explicitly or use the argument variable in the query body",
            arg.name,
            arg.name,
            arg.name
        );
        missing.push(arg);
    }
    if missing.is_empty() {
        Ok(query_source.to_string())
    } else {
        inject_verb_arg_atoms(query_source, &missing)
    }
}

fn single_query(program: &Program) -> Result<&anneal_core::runtime::Query> {
    let mut queries = program.queries();
    let Some(query) = queries.next() else {
        bail!("save query must contain one final ? query");
    };
    ensure!(
        queries.next().is_none(),
        "save query must contain exactly one final ? query"
    );
    Ok(query)
}

fn verb_arg_references(query: &anneal_core::runtime::Query) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    for rule in &query.local_rules {
        collect_verb_arg_refs_from_body(&rule.body, &mut refs);
    }
    collect_verb_arg_refs_from_body(&query.body, &mut refs);
    refs
}

fn collect_verb_arg_refs_from_body(body: &Body, refs: &mut BTreeSet<String>) {
    for atom in &body.atoms {
        match atom {
            Atom::Derived(atom)
                if atom.predicate.module.is_none()
                    && atom.predicate.name.as_str() == "verb_arg"
                    && atom.args.len() == 2 =>
            {
                if let Some(Expr::Literal(Literal::String(name))) = atom.args[0].expr() {
                    refs.insert(name.clone());
                }
            }
            Atom::Negation(negation) => {
                if let NegatedAtom::Derived(atom) = &negation.atom
                    && atom.predicate.module.is_none()
                    && atom.predicate.name.as_str() == "verb_arg"
                    && atom.args.len() == 2
                    && let Some(Expr::Literal(Literal::String(name))) = atom.args[0].expr()
                {
                    refs.insert(name.clone());
                }
            }
            Atom::TimeBlock(time_block) => {
                collect_verb_arg_refs_from_body(&time_block.body, refs);
            }
            Atom::Aggregation(aggregate) => {
                collect_verb_arg_refs_from_body(&aggregate.body, refs);
            }
            Atom::Stored(_) | Atom::Derived(_) | Atom::Comparison(_) => {}
        }
    }
}

fn inject_verb_arg_atoms(query_source: &str, args: &[&SaveArgSpec]) -> Result<String> {
    let query_start = find_final_query_marker(query_source)
        .context("save query must contain one final ? query")?;
    let (prefix, rest) = query_source.split_at(query_start + '?'.len_utf8());
    let mut rendered = String::new();
    rendered.push_str(prefix);
    rendered.push(' ');
    for (idx, arg) in args.iter().enumerate() {
        if idx > 0 {
            rendered.push_str(", ");
        }
        write!(
            rendered,
            "verb_arg({}, {})",
            datalog_string_literal(&arg.name),
            arg.name
        )?;
    }
    let rest = rest.trim_start();
    if !rest.is_empty() {
        rendered.push_str(", ");
        rendered.push_str(rest);
    }
    Ok(rendered)
}

fn find_final_query_marker(source: &str) -> Option<usize> {
    let mut last_query = None;
    let mut in_string = false;
    let mut escaped = false;
    let mut in_comment = false;
    for (idx, ch) in source.char_indices() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '#' => in_comment = true,
            '"' => in_string = true,
            '?' => last_query = Some(idx),
            _ => {}
        }
    }
    last_query
}

fn infer_output_schema(query_source: &str, args: &[SaveArgSpec]) -> Result<serde_json::Value> {
    let program = parse_program("anneal-save:normalized-query", query_source)
        .context("normalized save query must parse")?;
    let query = single_query(&program)?;
    let arg_types = args
        .iter()
        .map(|arg| (arg.name.as_str(), arg.kind.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut schema = serde_json::Map::new();
    for field in query.body.positive_binding_variables() {
        let field = field.to_string();
        schema.insert(
            field.clone(),
            serde_json::Value::String(infer_field_type(&field, &arg_types).to_string()),
        );
    }
    Ok(serde_json::Value::Object(schema))
}

fn infer_field_type<'a>(field: &str, arg_types: &BTreeMap<&str, &'a str>) -> &'a str {
    if let Some(kind) = arg_types.get(field) {
        return kind;
    }
    match field {
        "h" | "handle" | "subject" | "from" | "to" | "src" | "dst" | "other" | "neighbor"
        | "citer" => "HandleId",
        "score" | "energy" | "line" | "start_line" | "end_line" | "tokens" | "days" | "count"
        | "files" | "errors" | "cross_edges" | "limit" | "budget" => "Number",
        "low_confidence" | "strict" | "active" | "terminal" => "Bool",
        _ => "String",
    }
}

fn render_verb_declaration(
    name: &str,
    query: &str,
    doc: &str,
    output_schema: &serde_json::Value,
    args: &[SaveArgSpec],
) -> String {
    let schema = serde_json::to_string(output_schema).expect("schema value serializes");
    let args = args
        .iter()
        .map(|arg| datalog_string_literal(&arg.raw))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "@verb(\n  name: {},\n  query: {},\n  doc: {},\n  output_schema: {},\n  args: [{}],\n  capabilities: [\"read\"]\n).\n",
        datalog_string_literal(name),
        datalog_string_literal(query),
        datalog_string_literal(doc),
        datalog_string_literal(&schema),
        args
    )
}

fn parse_verb_declaration(source: &str) -> Result<VerbDecl> {
    let program =
        parse_program("anneal-save:verb", source).context("generated @verb must parse")?;
    program
        .statements
        .into_iter()
        .find_map(|statement| match statement {
            Statement::Verb(verb) => Some(verb),
            _ => None,
        })
        .context("generated declaration did not contain @verb")
}

fn validate_verb(base_program: &Program, verb: &VerbDecl) -> Result<()> {
    let query_program = validate_project_verb_query_program(verb)?;
    let mut combined = base_program.clone();
    combined.statements.extend(query_program.statements);
    analyze(combined).context("saved verb query failed static analysis")?;
    Ok(())
}

fn read_project_rules(path: &camino::Utf8Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err).with_context(|| format!("failed to read {path}")),
    }
}

fn append_declaration(existing: &str, declaration: &str) -> String {
    let mut updated = String::new();
    if !existing.trim().is_empty() {
        updated.push_str(existing.trim_end());
        updated.push_str("\n\n");
    }
    updated.push_str(declaration);
    updated
}

fn replace_span(existing: &str, span: std::ops::Range<usize>, declaration: &str) -> String {
    let mut updated = String::new();
    updated.push_str(&existing[..span.start]);
    updated.push_str(declaration.trim_end());
    updated.push_str(&existing[span.end..]);
    updated
}

fn find_verb_declaration_span(source: &str, name: &str) -> Option<std::ops::Range<usize>> {
    let mut offset = 0;
    while let Some(relative) = source[offset..].find("@verb") {
        let start = offset + relative;
        let Some(end) = scan_annotation_end(source, start) else {
            offset = start + "@verb".len();
            continue;
        };
        let block = &source[start..end];
        if let Ok(program) = parse_program("anneal-save:existing-verb", block)
            && program.statements.iter().any(|statement| {
                matches!(statement, Statement::Verb(verb) if verb.string_arg("name") == Some(name))
            })
        {
            return Some(start..end);
        }
        offset = end;
    }
    None
}

fn scan_annotation_end(source: &str, start: usize) -> Option<usize> {
    let open = source[start..].find('(')? + start;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in source[open..].char_indices() {
        let absolute = open + idx;
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    let after = absolute + ch.len_utf8();
                    let whitespace = source[after..]
                        .char_indices()
                        .find(|(_, ch)| !ch.is_whitespace())
                        .map_or(source.len(), |(idx, _)| after + idx);
                    return source[whitespace..]
                        .starts_with('.')
                        .then_some(whitespace + '.'.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

struct ProjectFileLock {
    path: Utf8PathBuf,
}

impl ProjectFileLock {
    fn acquire(project_path: &camino::Utf8Path) -> Result<Self> {
        let lock_path = project_path.with_extension("dl.lock");
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("failed to create {parent}"))?;
        }
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .with_context(|| {
                format!("failed to lock {project_path}; another `anneal save` may be editing it")
            })?;
        Ok(Self { path: lock_path })
    }
}

impl Drop for ProjectFileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn write_atomic(path: &camino::Utf8Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("failed to create {parent}"))?;
    }
    let tmp = path.with_extension(format!("dl.{}.tmp", std::process::id()));
    fs::write(&tmp, contents).with_context(|| format!("failed to write temporary {tmp}"))?;
    fs::rename(&tmp, path).with_context(|| format!("failed to replace {path}"))?;
    Ok(())
}

fn format_verb_source(source: &VerbSource) -> String {
    let location = source.location();
    format!(
        "{} {}:{}",
        source.layer(),
        location.source_name,
        location.line
    )
}
