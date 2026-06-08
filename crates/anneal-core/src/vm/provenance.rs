//! Derivation provenance for planned execution and explain output.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;

use crate::ir::plan::{AggregateProvenance, CompareProvenance, NegationProvenance, RuleProvenance};
use crate::runtime::ast::{AggregateFunction, Ident, PredicateRef, RuleLayer, SourceLocation};
use crate::runtime::eval::{ExplainOptions, Value};

pub(crate) type DerivationRef = Arc<DerivationNode>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DerivationNode {
    kind: DerivationKind,
    label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    relation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    predicate: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tuple: Vec<Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    fields: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    truncated: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    children: Vec<Self>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivationKind {
    Query,
    Rule,
    Fact,
    Stored,
    Primitive,
    Comparison,
    Aggregate,
    Negation,
    TimeBlock,
    RecursiveChain,
    Truncated,
}

impl DerivationNode {
    #[must_use]
    pub fn synthetic_query(children: Vec<Self>) -> Self {
        Self::query(children)
    }

    #[must_use]
    pub const fn kind(&self) -> DerivationKind {
        self.kind
    }

    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    #[must_use]
    pub fn children(&self) -> &[Self] {
        &self.children
    }

    #[must_use]
    pub fn fields(&self) -> &BTreeMap<String, Value> {
        &self.fields
    }

    pub(crate) fn query(children: Vec<Self>) -> Self {
        Self {
            kind: DerivationKind::Query,
            label: "query output row".to_string(),
            relation: None,
            predicate: None,
            tuple: Vec::new(),
            fields: BTreeMap::new(),
            source: None,
            line: None,
            column: None,
            truncated: None,
            children,
        }
    }

    pub(crate) fn planned_rule(
        provenance: &RuleProvenance,
        tuple: &[Value],
        children: Vec<Self>,
    ) -> Self {
        Self::rule_from_parts(
            provenance.predicate.display_name(),
            provenance.layer,
            &provenance.location,
            tuple,
            children,
        )
    }

    pub(crate) fn rule_from_parts(
        predicate: String,
        layer: RuleLayer,
        location: &SourceLocation,
        tuple: &[Value],
        children: Vec<Self>,
    ) -> Self {
        Self {
            kind: DerivationKind::Rule,
            label: format!("rule {predicate} fired from {layer:?}"),
            relation: None,
            predicate: Some(predicate),
            tuple: tuple.to_vec(),
            fields: BTreeMap::new(),
            source: Some(location.source_name.clone()),
            line: non_zero(location.line),
            column: non_zero(location.column),
            truncated: None,
            children,
        }
    }

    pub(crate) fn fact(predicate: &PredicateRef, tuple: &[Value]) -> Self {
        Self {
            kind: DerivationKind::Fact,
            label: format!("fact {}", predicate.display_name()),
            relation: None,
            predicate: Some(predicate.display_name()),
            tuple: tuple.to_vec(),
            fields: BTreeMap::new(),
            source: None,
            line: None,
            column: None,
            truncated: None,
            children: Vec::new(),
        }
    }

    pub(crate) fn stored(
        relation: &Ident,
        fields: BTreeMap<String, Value>,
        source: Option<String>,
        line: Option<usize>,
    ) -> Self {
        Self {
            kind: DerivationKind::Stored,
            label: format!("stored *{relation} row matched"),
            relation: Some(relation.to_string()),
            predicate: None,
            tuple: Vec::new(),
            fields,
            source,
            line,
            column: None,
            truncated: None,
            children: Vec::new(),
        }
    }

    pub(crate) fn primitive(predicate: &PredicateRef, tuple: &[Value]) -> Self {
        Self {
            kind: DerivationKind::Primitive,
            label: format!("primitive {} returned a tuple", predicate.display_name()),
            relation: None,
            predicate: Some(predicate.display_name()),
            tuple: tuple.to_vec(),
            fields: BTreeMap::new(),
            source: None,
            line: None,
            column: None,
            truncated: None,
            children: Vec::new(),
        }
    }

    pub(crate) fn planned_comparison(provenance: &CompareProvenance) -> Self {
        Self::located(
            DerivationKind::Comparison,
            "comparison matched",
            provenance.location.clone(),
        )
    }

    pub(crate) fn planned_aggregate(provenance: &AggregateProvenance, children: Vec<Self>) -> Self {
        Self::aggregate_from_parts(provenance.function, provenance.location.clone(), children)
    }

    pub(crate) fn aggregate_from_parts(
        function: AggregateFunction,
        location: SourceLocation,
        children: Vec<Self>,
    ) -> Self {
        let mut node = Self::located(
            DerivationKind::Aggregate,
            &format!("aggregate {function:?} produced a value"),
            location,
        );
        node.children = children;
        node
    }

    pub(crate) fn planned_negation(provenance: &NegationProvenance) -> Self {
        Self::negation_from_location(provenance.location.clone())
    }

    pub(crate) fn negation_from_location(location: SourceLocation) -> Self {
        Self::located(
            DerivationKind::Negation,
            "negated atom had no matches",
            location,
        )
    }

    pub(crate) fn time_block(
        reference: &str,
        location: SourceLocation,
        children: Vec<Self>,
    ) -> Self {
        let mut node = Self::located(
            DerivationKind::TimeBlock,
            &format!("evaluated at {reference:?}"),
            location,
        );
        node.children = children;
        node
    }

    fn located(kind: DerivationKind, label: &str, location: SourceLocation) -> Self {
        Self {
            kind,
            label: label.to_string(),
            relation: None,
            predicate: None,
            tuple: Vec::new(),
            fields: BTreeMap::new(),
            source: Some(location.source_name),
            line: non_zero(location.line),
            column: non_zero(location.column),
            truncated: None,
            children: Vec::new(),
        }
    }

    pub(crate) fn bounded(&self, options: &ExplainOptions) -> Self {
        let mut rule_stack = Vec::new();
        self.bounded_inner(options.depth().get(), options, &mut rule_stack)
    }

    pub(crate) fn evidence_truncated(omitted: usize) -> Self {
        Self {
            kind: DerivationKind::Truncated,
            label: format!("... {omitted} more aggregate evidence nodes omitted"),
            relation: None,
            predicate: None,
            tuple: Vec::new(),
            fields: BTreeMap::new(),
            source: None,
            line: None,
            column: None,
            truncated: Some("aggregate evidence limit reached".to_string()),
            children: Vec::new(),
        }
    }

    fn bounded_inner(
        &self,
        remaining_depth: usize,
        options: &ExplainOptions,
        rule_stack: &mut Vec<String>,
    ) -> Self {
        if remaining_depth == 0 {
            return Self {
                kind: DerivationKind::Truncated,
                label: "... more derivation levels (use --explain-depth)".to_string(),
                relation: None,
                predicate: None,
                tuple: Vec::new(),
                fields: BTreeMap::new(),
                source: None,
                line: None,
                column: None,
                truncated: Some("depth limit reached".to_string()),
                children: Vec::new(),
            };
        }

        let fingerprint = self.rule_fingerprint();
        if !options.explicit_depth()
            && let Some(fingerprint) = &fingerprint
            && rule_stack.contains(fingerprint)
        {
            let hops = rule_stack
                .iter()
                .filter(|existing| *existing == fingerprint)
                .count()
                + 1;
            return Self {
                kind: DerivationKind::RecursiveChain,
                label: format!("via {fingerprint} x {hops} recursive hops"),
                relation: None,
                predicate: self.predicate.clone(),
                tuple: self.tuple.clone(),
                fields: BTreeMap::new(),
                source: self.source.clone(),
                line: self.line,
                column: self.column,
                truncated: Some("recursive chain summarized".to_string()),
                children: Vec::new(),
            };
        }

        if let Some(fingerprint) = &fingerprint {
            rule_stack.push(fingerprint.clone());
        }
        let mut node = self.clone();
        node.children = self
            .children
            .iter()
            .map(|child| child.bounded_inner(remaining_depth - 1, options, rule_stack))
            .collect();
        if fingerprint.is_some() {
            rule_stack.pop();
        }
        node
    }

    fn rule_fingerprint(&self) -> Option<String> {
        if self.kind != DerivationKind::Rule {
            return None;
        }
        let predicate = self.predicate.as_ref()?;
        Some(match (&self.source, self.line) {
            (Some(source), Some(line)) => format!("{predicate}@{source}:{line}"),
            (Some(source), None) => format!("{predicate}@{source}"),
            (None, _) => predicate.clone(),
        })
    }
}

pub(crate) fn derivation_ref(node: DerivationNode) -> DerivationRef {
    Arc::new(node)
}

fn non_zero(value: usize) -> Option<usize> {
    (value != 0).then_some(value)
}
