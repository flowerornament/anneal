//! Minimal dynamic-IR parser and evaluator used to measure the Phase 0
//! architecture revision: project/prelude rules are parsed at runtime and
//! evaluated outside Ascent, while Ascent remains available for fixed
//! engine-derived primitives.
//!
//! This is intentionally a skeleton. It parses a small Datalog-shaped subset
//! into stable IR structs, then evaluates the benchmark prelude rules with
//! hand-written rule plans. The point of Phase 0 is timing the dynamic rule
//! layer at large-corpus scale, not pretending this is the production evaluator.

use crate::fixture::{Edge, Handle};
use crate::loader::Corpus;
use crate::types::{EdgeKind, HandleId, HandleKind, Namespace};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const BENCH_PRELUDE: &str = r#"
terminal(h) :- handle(h, _kind, status, _ns, _file, _area, _date), is_terminal(status).
active(h) :- handle(h, _kind, status, _ns, _file, _area, _date), is_active(status).
settled(h) :- handle(h, _kind, status, _ns, _file, _area, _date), is_settled(status).
upstream(h, anc) :- edge(h, anc, "DependsOn", _file, _line).
upstream(h, anc) :- edge(h, mid, "DependsOn", _file, _line), upstream(mid, anc).
obligation(h) :- handle(h, "label", _status, "OQ", _file, _area, _date).
discharged(h) :- edge(_from, h, "Discharges", _file, _line).
undischarged(h) :- obligation(h), not discharged(h), not terminal(h).
open_oq(h) :- handle(h, "label", _status, "OQ", _file, _area, _date), not terminal(h).
downstream_settled(q, h) :- open_oq(q), edge(h, q, "DependsOn", _file, _line), settled(h).
oq_pressure(q, n) :- count downstream_settled(q, _h).
oq_per_area(area, n) :- count open_oq_in_area(area, _h).
release_blocker(h) :- undischarged(h).
release_blocker(h) :- edge(h, t, "DependsOn", _file, _line), active(h), terminal(t).
"#;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Program {
    rules: Vec<Rule>,
}

impl Program {
    pub fn parse(input: &str) -> Result<Self, IrError> {
        let mut rules = Vec::new();
        let mut current = String::new();
        for line in input.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            current.push_str(line);
            current.push(' ');
            while let Some(idx) = current.find('.') {
                let raw = current[..idx].trim();
                if !raw.is_empty() {
                    rules.push(parse_rule(raw)?);
                }
                current = current[idx + 1..].trim_start().to_string();
            }
        }
        if !current.trim().is_empty() {
            return Err(IrError::UnterminatedRule(current));
        }
        Ok(Self { rules })
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    fn has_head(&self, predicate: &str) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.head.predicate == predicate)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Rule {
    pub head: Atom,
    pub body: Vec<Literal>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Literal {
    pub negated: bool,
    pub atom: Atom,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Atom {
    pub predicate: String,
    pub terms: Vec<Term>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Term {
    Var(String),
    Wildcard,
    Str(String),
}

#[derive(Debug, thiserror::Error)]
pub enum IrError {
    #[error("unterminated rule: {0}")]
    UnterminatedRule(String),

    #[error("invalid rule {0:?}: missing head")]
    MissingHead(String),

    #[error("invalid atom {0:?}: missing '('")]
    MissingOpenParen(String),

    #[error("invalid atom {0:?}: missing ')'")]
    MissingCloseParen(String),

    #[error("invalid atom {0:?}: empty predicate")]
    EmptyPredicate(String),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EvalSummary {
    pub rule_count: usize,
    pub relation_counts: BTreeMap<&'static str, usize>,
}

pub struct Evaluator {
    program: Program,
}

impl Evaluator {
    pub fn new(program: Program) -> Self {
        Self { program }
    }

    #[allow(clippy::too_many_lines)]
    pub fn eval(&self, corpus: &Corpus) -> EvalSummary {
        let handles = &corpus.handles;
        let edges = &corpus.edges;

        let terminal = if self.program.has_head("terminal") {
            handles
                .iter()
                .filter(|h| h.status.is_terminal())
                .map(|h| h.id)
                .collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };
        let active = if self.program.has_head("active") {
            handles
                .iter()
                .filter(|h| h.status.is_active())
                .map(|h| h.id)
                .collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };
        let settled = if self.program.has_head("settled") {
            handles
                .iter()
                .filter(|h| h.status.is_settled())
                .map(|h| h.id)
                .collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };
        let upstream = if self.program.has_head("upstream") {
            transitive_upstream(edges)
        } else {
            HashSet::new()
        };
        let obligation = if self.program.has_head("obligation") {
            oq_labels(handles).collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };
        let discharged = if self.program.has_head("discharged") {
            edges
                .iter()
                .filter(|e| e.kind == EdgeKind::Discharges)
                .map(|e| e.to)
                .collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };
        let undischarged = if self.program.has_head("undischarged") {
            obligation
                .iter()
                .copied()
                .filter(|h| !discharged.contains(h) && !terminal.contains(h))
                .collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };
        let open_oq = if self.program.has_head("open_oq") {
            oq_labels(handles)
                .filter(|h| !terminal.contains(h))
                .collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };
        let downstream_settled = if self.program.has_head("downstream_settled") {
            edges
                .iter()
                .filter(|e| {
                    e.kind == EdgeKind::DependsOn
                        && open_oq.contains(&e.to)
                        && settled.contains(&e.from)
                })
                .map(|e| (e.to, e.from))
                .collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };
        let oq_pressure = if self.program.has_head("oq_pressure") {
            grouped_count(downstream_settled.iter().map(|(q, _)| *q))
        } else {
            HashMap::new()
        };
        let oq_per_area = if self.program.has_head("oq_per_area") {
            grouped_count(handles.iter().filter_map(|h| {
                (h.kind == HandleKind::Label
                    && h.namespace == Namespace("OQ")
                    && !terminal.contains(&h.id))
                .then_some(h.area)
            }))
        } else {
            HashMap::new()
        };
        let release_blocker = if self.program.has_head("release_blocker") {
            let mut blockers = undischarged.clone();
            for edge in edges {
                if edge.kind == EdgeKind::DependsOn
                    && active.contains(&edge.from)
                    && terminal.contains(&edge.to)
                {
                    blockers.insert(edge.from);
                }
            }
            blockers
        } else {
            HashSet::new()
        };

        let mut counts = BTreeMap::new();
        counts.insert("terminal", terminal.len());
        counts.insert("active", active.len());
        counts.insert("settled", settled.len());
        counts.insert("upstream", upstream.len());
        counts.insert("obligation", obligation.len());
        counts.insert("discharged", discharged.len());
        counts.insert("undischarged", undischarged.len());
        counts.insert("open_oq", open_oq.len());
        counts.insert("downstream_settled", downstream_settled.len());
        counts.insert("oq_pressure", oq_pressure.len());
        counts.insert("oq_per_area", oq_per_area.len());
        counts.insert("release_blocker", release_blocker.len());

        EvalSummary {
            rule_count: self.program.rules.len(),
            relation_counts: counts,
        }
    }
}

fn parse_rule(raw: &str) -> Result<Rule, IrError> {
    let (head, body) = raw
        .split_once(":-")
        .map_or((raw.trim(), ""), |(head, body)| (head.trim(), body.trim()));
    if head.is_empty() {
        return Err(IrError::MissingHead(raw.to_string()));
    }
    let body = if body.is_empty() {
        Vec::new()
    } else {
        split_top_level(body, ',')
            .into_iter()
            .map(|part| {
                let part = part.trim();
                let (negated, atom) = part
                    .strip_prefix("not ")
                    .map_or((false, part), |stripped| (true, stripped.trim()));
                Ok(Literal {
                    negated,
                    atom: parse_atom(atom)?,
                })
            })
            .collect::<Result<Vec<_>, IrError>>()?
    };
    Ok(Rule {
        head: parse_atom(head)?,
        body,
    })
}

fn parse_atom(raw: &str) -> Result<Atom, IrError> {
    let open = raw
        .find('(')
        .ok_or_else(|| IrError::MissingOpenParen(raw.to_string()))?;
    let close = raw
        .rfind(')')
        .ok_or_else(|| IrError::MissingCloseParen(raw.to_string()))?;
    let predicate = raw[..open].trim();
    if predicate.is_empty() {
        return Err(IrError::EmptyPredicate(raw.to_string()));
    }
    let args = raw[open + 1..close]
        .trim()
        .is_empty()
        .then(Vec::new)
        .unwrap_or_else(|| split_top_level(&raw[open + 1..close], ','))
        .into_iter()
        .map(|part| parse_term(part.trim()))
        .collect();
    Ok(Atom {
        predicate: predicate.to_string(),
        terms: args,
    })
}

fn parse_term(raw: &str) -> Term {
    if raw == "_" || raw.starts_with('_') {
        Term::Wildcard
    } else if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        Term::Str(raw[1..raw.len() - 1].to_string())
    } else {
        Term::Var(raw.to_string())
    }
}

fn split_top_level(input: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut prev_escape = false;
    for (idx, ch) in input.char_indices() {
        if in_string {
            if ch == '"' && !prev_escape {
                in_string = false;
            }
            prev_escape = ch == '\\' && !prev_escape;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 => {
                parts.push(input[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
        prev_escape = false;
    }
    parts.push(input[start..].trim());
    parts
}

fn oq_labels(handles: &[Handle]) -> impl Iterator<Item = HandleId> + '_ {
    handles
        .iter()
        .filter(|h| h.kind == HandleKind::Label && h.namespace == Namespace("OQ"))
        .map(|h| h.id)
}

fn transitive_upstream(edges: &[Edge]) -> HashSet<(HandleId, HandleId)> {
    let mut adjacency: HashMap<HandleId, Vec<HandleId>> = HashMap::new();
    for edge in edges.iter().filter(|e| e.kind == EdgeKind::DependsOn) {
        adjacency.entry(edge.from).or_default().push(edge.to);
    }

    let mut upstream = HashSet::new();
    for (&start, direct) in &adjacency {
        let mut seen = HashSet::new();
        let mut stack = direct.clone();
        while let Some(ancestor) = stack.pop() {
            if seen.insert(ancestor) {
                upstream.insert((start, ancestor));
                if let Some(next) = adjacency.get(&ancestor) {
                    stack.extend(next);
                }
            }
        }
    }
    upstream
}

fn grouped_count<K: Eq + std::hash::Hash>(keys: impl IntoIterator<Item = K>) -> HashMap<K, usize> {
    let mut counts = HashMap::new();
    for key in keys {
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{EDGES, HANDLES};

    #[test]
    fn parser_handles_negation_and_strings() {
        let program = Program::parse(r#"open_oq(h) :- handle(h, "label"), not terminal(h)."#)
            .expect("program parses");
        assert_eq!(program.rules().len(), 1);
        let rule = &program.rules()[0];
        assert_eq!(rule.head.predicate, "open_oq");
        assert_eq!(rule.body.len(), 2);
        assert_eq!(rule.body[0].atom.terms[1], Term::Str("label".to_string()));
        assert!(rule.body[1].negated);
    }

    #[test]
    fn bench_prelude_parses() {
        let program = Program::parse(BENCH_PRELUDE).expect("benchmark prelude parses");
        assert_eq!(program.rules().len(), 14);
        assert!(program.has_head("release_blocker"));
    }

    #[test]
    fn evaluator_finds_fixture_upstream_and_blockers() {
        let program = Program::parse(BENCH_PRELUDE).expect("benchmark prelude parses");
        let corpus = Corpus {
            handles: HANDLES.to_vec(),
            edges: EDGES.to_vec(),
        };
        let summary = Evaluator::new(program).eval(&corpus);
        assert_eq!(summary.relation_counts["open_oq"], 5);
        assert_eq!(summary.relation_counts["upstream"], 8);
        assert_eq!(summary.relation_counts["release_blocker"], 5);
    }
}
