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
                ("builtin_diagnostic_policy", [code, severity, key]) => {
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
        assert_eq!(producers.len(), 17);
        assert_eq!(producers, declared);
        let forwarded = program
            .rules()
            .filter(|rule| rule.head.predicate.display_name() == "diagnostic")
            .map(|rule| {
                string_term(&rule.head.terms[0])
                    .expect("literal diagnostic code")
                    .to_owned()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(forwarded, declared.into_keys().collect());
    }
}
