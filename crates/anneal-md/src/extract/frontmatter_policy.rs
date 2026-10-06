//! Canonical markdown frontmatter semantics and conventional alias teaching.
//!
//! The core prelude is tested against this authority without depending on md.

use super::config::{Direction, FrontmatterFieldMapping};

pub(super) struct CanonicalField {
    pub key: &'static str,
    kind: &'static str,
    direction: Direction,
    scaffold: bool,
}

impl CanonicalField {
    pub fn mapping(&self) -> FrontmatterFieldMapping {
        FrontmatterFieldMapping {
            edge_kind: self.kind.to_string(),
            direction: self.direction,
        }
    }
}

const fn canonical(
    key: &'static str,
    kind: &'static str,
    direction: Direction,
    scaffold: bool,
) -> CanonicalField {
    CanonicalField {
        key,
        kind,
        direction,
        scaffold,
    }
}

pub(super) const CANONICAL: &[CanonicalField] = &[
    canonical("references", "Cites", Direction::Forward, true),
    canonical("cites", "Cites", Direction::Forward, true),
    canonical("sources", "Cites", Direction::Forward, true),
    canonical("source", "Cites", Direction::Forward, true),
    canonical("depends-on", "DependsOn", Direction::Forward, false),
    canonical("based-on", "DependsOn", Direction::Forward, true),
    canonical("supersedes", "Supersedes", Direction::Inverse, false),
    canonical("superseded-by", "Supersedes", Direction::Forward, false),
    canonical("discharges", "Discharges", Direction::Forward, false),
    canonical("verifies", "Verifies", Direction::Forward, false),
    canonical("affects", "DependsOn", Direction::Inverse, true),
];

pub(super) struct Alias {
    pub key: &'static str,
    pub canonical: &'static str,
    scaffold: bool,
}

const fn alias(key: &'static str, canonical: &'static str, scaffold: bool) -> Alias {
    Alias {
        key,
        canonical,
        scaffold,
    }
}

pub(super) const ALIASES: &[Alias] = &[
    alias("references", "references", false),
    alias("refs", "references", true),
    alias("relates", "references", false),
    alias("related", "references", true),
    alias("related-to", "references", false),
    alias("related_to", "references", false),
    alias("pins", "references", false),
    alias("companion-to", "references", false),
    alias("companion_to", "references", false),
    alias("see-also", "references", true),
    alias("see_also", "references", false),
    alias("tracked-by", "references", false),
    alias("tracked_by", "references", false),
    alias("cites", "references", false),
    alias("builds-on", "depends-on", true),
    alias("builds_on", "depends-on", false),
    alias("extends", "depends-on", true),
    alias("requires", "depends-on", false),
    alias("dependencies", "depends-on", false),
    alias("depends_on", "depends-on", false),
    alias("superseded_by", "superseded-by", false),
    alias("replaced-by", "superseded-by", false),
    alias("replaced_by", "superseded-by", false),
    alias("replaces", "supersedes", false),
    alias("obsoletes", "supersedes", false),
    alias("impacts", "affects", true),
    alias("resolves", "discharges", true),
    alias("addresses", "discharges", true),
    alias("parent", "depends-on", true),
];

pub(super) fn proposed_mapping(key: &str) -> Option<FrontmatterFieldMapping> {
    let lower = key.to_lowercase();
    let canonical = CANONICAL
        .iter()
        .find(|entry| entry.key == lower && entry.scaffold)
        .or_else(|| {
            ALIASES
                .iter()
                .find(|entry| entry.key == lower && entry.scaffold)
                .and_then(|alias| CANONICAL.iter().find(|entry| entry.key == alias.canonical))
        });
    canonical.map(CanonicalField::mapping)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use anneal_core::runtime::{
        Database, Evaluator, QueryOutput, Value, analyze, parse_program, standard_prelude_program,
    };
    use anneal_core::{ConfigFact, CorpusId, FactBatch, FactStore, Generation, SourceName};
    use camino::Utf8PathBuf;

    use super::{ALIASES, CANONICAL};
    use crate::extract::{adapter, config, parse};

    fn evaluate(batch: Option<FactBatch>, intentions: &[&str], query: &str) -> QueryOutput {
        let mut store = FactStore::default();
        if let Some(batch) = batch {
            store.merge(batch).expect("fixture merge");
        }
        store
            .replace_configs(
                &CorpusId::from("test"),
                intentions
                    .iter()
                    .map(|key| ConfigFact {
                        corpus: CorpusId::from("test"),
                        key: "frontmatter.unmapped".into(),
                        value: (*key).into(),
                        ordinal: None,
                    })
                    .collect(),
            )
            .expect("fixture intentions");
        let mut program = standard_prelude_program().expect("prelude");
        program.statements.extend(
            parse_program("policy-test", query)
                .expect("query")
                .statements,
        );
        let analyzed = analyze(program).expect("analysis");
        let query = analyzed.queries().next().expect("one query").clone();
        let mut evaluator = Evaluator::new(analyzed, Database::from_store(&store));
        evaluator.run_fixpoint_for_query(&query).expect("fixpoint");
        evaluator.eval_query(&query).expect("evaluation")
    }

    fn alias_rows() -> BTreeSet<Vec<String>> {
        evaluate(
            None,
            &[],
            "? frontmatter_mapping_alias(key, field, kind, direction).",
        )
        .rows
        .into_iter()
        .map(|row| {
            ["key", "field", "kind", "direction"]
                .iter()
                .map(|key| {
                    let Value::String(value) = &row.fields[*key] else {
                        panic!("string policy field")
                    };
                    value.clone()
                })
                .collect()
        })
        .collect()
    }

    fn expected_rows() -> BTreeSet<Vec<String>> {
        ALIASES
            .iter()
            .map(|alias| {
                let entry = CANONICAL
                    .iter()
                    .find(|entry| entry.key == alias.canonical)
                    .expect("canonical alias destination");
                let direction = match entry.direction {
                    config::Direction::Forward => "forward",
                    config::Direction::Inverse => "inverse",
                };
                vec![
                    alias.key.into(),
                    alias.canonical.into(),
                    entry.kind.into(),
                    direction.into(),
                ]
            })
            .collect()
    }

    fn assert_parity(actual: &BTreeSet<Vec<String>>, expected: &BTreeSet<Vec<String>>) {
        assert_eq!(
            actual, expected,
            "core prelude differs from markdown frontmatter policy"
        );
    }

    #[test]
    fn frontmatter_policy_prelude_parity() {
        let actual = alias_rows();
        assert_eq!(actual.len(), 29);
        assert_parity(&actual, &expected_rows());
    }

    #[test]
    #[should_panic(expected = "core prelude differs from markdown frontmatter policy")]
    fn frontmatter_policy_parity_mismatch_control() {
        let mut changed = alias_rows();
        let row = changed
            .iter()
            .find(|row| row[0] == "refs")
            .expect("refs is in actual prelude")
            .clone();
        changed.remove(&row);
        let mut row = row;
        row[2] = "DependsOn".into();
        changed.insert(row);
        assert_parity(&changed, &expected_rows());
    }

    #[test]
    fn frontmatter_policy_all_defaults_and_scaffold_population() {
        let defaults = config::FrontmatterConfig::default();
        assert_eq!(defaults.fields.len(), 11);
        assert_eq!(CANONICAL.len(), 11);
        for entry in CANONICAL {
            let parsed = parse::parse_frontmatter(&format!("{}: target.md", entry.key), &defaults);
            assert_eq!(
                parsed.field_edges.len(),
                1,
                "{} at one occurrence",
                entry.key
            );
            let mapping = &defaults.fields[entry.key];
            assert_eq!(mapping.edge_kind, entry.kind);
            assert_eq!(mapping.direction, entry.direction);
            assert_eq!(parsed.field_edges[0].edge_kind.as_str(), entry.kind);
            assert_eq!(
                parsed.field_edges[0].inverse,
                entry.direction == config::Direction::Inverse
            );
            assert_eq!(
                super::proposed_mapping(&entry.key.to_uppercase()).is_some(),
                entry.scaffold
            );
        }
        for alias in ALIASES {
            if CANONICAL.iter().any(|entry| entry.key == alias.key) {
                continue;
            }
            assert!(
                !defaults.fields.contains_key(alias.key),
                "alias must be inert"
            );
            assert_eq!(super::proposed_mapping(alias.key).is_some(), alias.scaffold);
        }
    }

    #[test]
    fn frontmatter_policy_opt_out_preserves_metadata_and_e001_is_honest() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(temp.path().join("corpus")).expect("utf8");
        std::fs::create_dir(&root).expect("corpus");
        std::fs::write(
            root.join("spec.md"),
            "---\nreferences: [target.md, missing.md]\nrefs: missing.md\nparent: missing.md\n---\n# Spec\n",
        )
        .expect("fixture");
        std::fs::write(root.join("target.md"), "# Target\n").expect("target");
        let extract = || {
            adapter::extract_markdown_facts(
                &root,
                CorpusId::from("test"),
                SourceName::from("md"),
                Generation::initial(),
            )
            .expect("extract")
        };
        let before = extract();
        assert_eq!(before.edges.len(), 2);
        assert!(
            before
                .edges
                .iter()
                .all(|edge| edge.kind.as_str() == "Cites")
        );
        assert_eq!(before.edges[0].kind.as_str(), "Cites");
        let warnings = evaluate(
            Some(before),
            &[],
            "? diagnostic{code: code, subject: subject}, code in [\"E001\", \"W007\"].",
        );
        assert_eq!(
            warnings.rows.len(),
            3,
            "missing citation plus two alias warnings"
        );
        std::fs::write(root.join("anneal.dl"), "config frontmatter { unmapped(\"references\"). unmapped(\"refs\"). unmapped(\"parent\"). }").expect("intentions");
        let after = extract();
        assert!(after.edges.is_empty());
        for key in ["references", "refs", "parent"] {
            assert!(after.meta.iter().any(|row| row.key == key
                && row.value == "missing.md"
                && row.role == anneal_core::MetaRole::AuthoredUnmodeled));
        }
        assert!(
            evaluate(
                Some(after),
                &["references", "refs", "parent"],
                "? diagnostic{code: code}, code in [\"E001\", \"W007\"]."
            )
            .rows
            .is_empty()
        );
        assert_eq!(
            evaluate(
                None,
                &["refs"],
                "? frontmatter_intentionally_unmapped(key)."
            )
            .rows
            .len(),
            1
        );
    }
}
