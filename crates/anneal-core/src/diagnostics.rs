//! Source-backed diagnostic declaration and promotion metadata.

use std::collections::{BTreeMap, BTreeSet};

use crate::runtime::ast::{Expr, Literal, Program, Term};

pub(crate) const ESCALATION_PREFIX: &str = "diagnostics.escalate.";

pub(crate) struct DiagnosticDeclaration {
    pub(crate) severity: String,
    pub(crate) config_key: String,
}

#[derive(Default)]
pub(crate) struct DiagnosticCatalog {
    pub(crate) declarations: BTreeMap<String, DiagnosticDeclaration>,
    promotions: BTreeSet<(String, String)>,
}

impl DiagnosticCatalog {
    pub(crate) fn from_program(program: &Program) -> Self {
        let mut catalog = Self::default();
        for fact in program
            .facts()
            .filter(|fact| fact.predicate.module.is_none())
        {
            match (fact.predicate.name.as_str(), fact.terms.as_slice()) {
                ("builtin_diagnostic_policy", [code, severity, key])
                | ("project_diagnostic_declaration", [code, severity, key, _]) => {
                    if let (Some(code), Some(severity), Some(key)) =
                        (string_term(code), string_term(severity), string_term(key))
                    {
                        catalog.declarations.insert(
                            code.to_owned(),
                            DiagnosticDeclaration {
                                severity: severity.to_owned(),
                                config_key: key.to_owned(),
                            },
                        );
                    }
                }
                ("diagnostic_severity_promotion", [declared, effective]) => {
                    if let (Some(declared), Some(effective)) =
                        (string_term(declared), string_term(effective))
                    {
                        catalog
                            .promotions
                            .insert((declared.to_owned(), effective.to_owned()));
                    }
                }
                _ => {}
            }
        }
        catalog
    }

    pub(crate) fn is_promotion(&self, declared: &str, effective: &str) -> bool {
        self.promotions
            .contains(&(declared.to_owned(), effective.to_owned()))
    }

    pub(crate) fn validate(&self, code: &str, effective: &str) -> Result<(), String> {
        let Some(declaration) = self.declarations.get(code) else {
            return Err(format!("unknown diagnostic code '{code}'"));
        };
        if declaration.severity == effective || self.is_promotion(&declaration.severity, effective)
        {
            return Ok(());
        }
        if !self
            .declarations
            .values()
            .any(|declared| declared.severity == effective)
        {
            return Err(format!("unknown severity '{effective}' for '{code}'"));
        }
        Err(format!(
            "'{code}' declares {}; cannot re-grade it to {effective}. Only upward promotions to warning or error are allowed; info and suggestion are distinct classes and cannot convert into each other. Accept individual rows with config suppress {{ rule(CODE, target). }}",
            declaration.severity
        ))
    }
}

fn string_term(term: &Term) -> Option<&str> {
    match term {
        Term::Expr(Expr::Literal(Literal::String(value))) => Some(value),
        _ => None,
    }
}

/// Validated teaching metadata. Ownership is distinct from grading origin.
pub(crate) struct ProjectDiagnosticCard {
    pub(crate) code: String,
    pub(crate) severity: String,
    pub(crate) doc: String,
    pub(crate) rule: String,
    pub(crate) evidence: Vec<String>,
    pub(crate) location: crate::runtime::ast::SourceLocation,
}

pub(crate) fn project_cards(program: &Program) -> Result<Vec<ProjectDiagnosticCard>, String> {
    use crate::runtime::ast::Statement;
    let mut cards = Vec::new();
    let mut codes = BTreeSet::new();
    let definitions = program
        .statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::Fact(head) => Some(head.predicate.display_name()),
            Statement::Rule(rule) => Some(rule.head.predicate.display_name()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    for statement in &program.statements {
        let Statement::Diagnostic(decl) = statement else {
            continue;
        };
        let fail = |reason: &str| format!("{}: @diagnostic {reason}", decl.location());
        let mut names = BTreeSet::new();
        for arg in &decl.args {
            if !["code", "severity", "doc", "rule", "evidence"].contains(&arg.name.as_str())
                || !names.insert(arg.name.as_str())
            {
                return Err(fail("has an unknown or repeated argument"));
            }
        }
        let code = decl
            .string_arg("code")
            .ok_or_else(|| fail("requires string code"))?;
        if !project_code(code) {
            return Err(fail("code must be P followed by one or more digits"));
        }
        if !codes.insert(code) {
            return Err(fail("duplicates a code declaration"));
        }
        let severity = decl
            .string_arg("severity")
            .ok_or_else(|| fail("requires string severity"))?;
        if !["error", "warning", "info", "suggestion"].contains(&severity) {
            return Err(fail("severity must be error, warning, info or suggestion"));
        }
        let doc = decl
            .string_arg("doc")
            .filter(|doc| !doc.trim().is_empty())
            .ok_or_else(|| fail("requires nonempty string doc"))?;
        let rule = decl
            .args
            .iter()
            .find(|arg| arg.name.as_str() == "rule")
            .and_then(|arg| match &arg.expr {
                Expr::Var(name) => Some(name.as_str()),
                Expr::Literal(Literal::String(name)) => Some(name.as_str()),
                _ => None,
            })
            .ok_or_else(|| fail("requires rule predicate name"))?;
        if !definitions.contains(rule) {
            return Err(fail("names an unknown rule predicate"));
        }
        let evidence = decl
            .string_list_arg("evidence")
            .filter(|names| !names.is_empty() && names.iter().all(|name| !name.trim().is_empty()))
            .ok_or_else(|| fail("requires nonempty evidence name list"))?;
        cards.push(ProjectDiagnosticCard {
            code: code.to_owned(),
            severity: severity.to_owned(),
            doc: doc.to_owned(),
            rule: rule.to_owned(),
            evidence: evidence.into_iter().map(str::to_owned).collect(),
            location: decl.location().clone(),
        });
    }
    Ok(cards)
}

fn project_code(code: &str) -> bool {
    code.strip_prefix('P').is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

/// Lower once: keep the authored rule body and origin, tag the output with its
/// clause location, and leave validation/union to the sealed stdlib.
pub(crate) fn lower_project(program: &mut Program) -> Result<(), String> {
    use crate::runtime::ast::{Head, PredicateRef, Statement};
    let cards = project_cards(program)?;
    for statement in &mut program.statements {
        let head = match statement {
            Statement::Fact(head) => head,
            Statement::Rule(rule) => &mut rule.head,
            _ => continue,
        };
        if head.predicate.name.as_str() != "project_diagnostic" {
            continue;
        }
        if head.terms.len() != 6 {
            return Err(format!(
                "{}: project_diagnostic requires six columns",
                head.location
            ));
        }
        if let Some(code) = string_term(&head.terms[0]) {
            if !project_code(code) {
                return Err(format!(
                    "{}: project diagnostic code must be P followed by digits",
                    head.location
                ));
            }
            if let Some(card) = cards.iter().find(|card| card.code == code)
                && matches!(&head.terms[1], Term::Expr(Expr::Literal(_)))
                && string_term(&head.terms[1]) != Some(card.severity.as_str())
            {
                return Err(format!(
                    "{}: {code} declares {}; literal row severity differs",
                    head.location, card.severity
                ));
            }
        } else if matches!(&head.terms[0], Term::Expr(Expr::Literal(_))) {
            return Err(format!(
                "{}: project diagnostic code must be a P-code string",
                head.location
            ));
        }
        if head.predicate.module.is_some() {
            continue;
        }
        let origin = format!("project_diagnostic at {}", head.location);
        head.predicate = PredicateRef::parse("project_diagnostic_producer")
            .map_err(|error| error.to_string())?;
        head.terms
            .push(Term::Expr(Expr::Literal(Literal::String(origin))));
    }
    for card in cards {
        let terms = [
            &card.code,
            &card.severity,
            &format!("{ESCALATION_PREFIX}{}", card.code),
            &format!("suppress.rule.{}", card.code),
        ]
        .into_iter()
        .map(|value| Term::Expr(Expr::Literal(Literal::String(value.clone()))))
        .collect();
        // This fact is emitted only after authored definitions have passed the
        // protected-layer check. It is not an additional authoring surface.
        let head = Head::new(
            PredicateRef::parse("project_diagnostic_declaration")
                .map_err(|error| error.to_string())?,
            terms,
            card.location,
        );
        program.statements.push(Statement::Fact(head));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_registry_agrees_with_all_builtin_producers() {
        let program = crate::runtime::prelude::standard_prelude_program().expect("prelude");
        let catalog = DiagnosticCatalog::from_program(&program);
        let producers = program
            .rules()
            .filter(|rule| rule.head.predicate.display_name() == "builtin_diagnostic")
            .map(|rule| {
                let terms = &rule.head.terms;
                (
                    string_term(&terms[0])
                        .expect("literal producer code")
                        .to_owned(),
                    string_term(&terms[1])
                        .expect("literal producer severity")
                        .to_owned(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let declared = catalog
            .declarations
            .iter()
            .map(|(code, declaration)| {
                assert_eq!(declaration.config_key, format!("{ESCALATION_PREFIX}{code}"));
                (code.clone(), declaration.severity.clone())
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(producers.len(), 18);
        assert_eq!(producers, declared);
        let forwarded = program
            .rules()
            .filter(|rule| {
                rule.head.predicate.display_name() == "diagnostic"
                    && string_term(&rule.head.terms[0]).is_some()
            })
            .map(|rule| {
                string_term(&rule.head.terms[0])
                    .expect("literal diagnostic code")
                    .to_owned()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(forwarded, declared.into_keys().collect());
    }
}
