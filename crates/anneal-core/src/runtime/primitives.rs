use crate::runtime::ast::{Ident, PredicateRef};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrimitivePredicate {
    Upstream,
    Downstream,
    Impact,
    Neighborhood,
    Terminal,
    Active,
    Settled,
    PipelinePosition,
    PipelinePositionFor,
    Obligation,
    Discharged,
    Undischarged,
    CiteCount,
    InDegree,
    OutDegree,
    DischargeCount,
    Freshness,
    Flux,
    GitMtime,
    Recent,
    TokenEstimate,
    Search,
    Read,
    ReadFull,
    Match,
    Schema,
    Predicates,
    Verbs,
    Describe,
    SourceOf,
    Examples,
    Sources,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PrimitiveSignature {
    pub(crate) parameters: &'static [&'static str],
    pub(crate) sealed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RequiredPrimitiveInput {
    pub(crate) position: usize,
    pub(crate) argument: &'static str,
}

impl PrimitiveSignature {
    pub(crate) fn arity(self) -> usize {
        self.parameters.len()
    }
}

impl PrimitivePredicate {
    pub(crate) const ALL: &'static [Self] = &[
        Self::Upstream,
        Self::Downstream,
        Self::Impact,
        Self::Neighborhood,
        Self::Terminal,
        Self::Active,
        Self::Settled,
        Self::PipelinePosition,
        Self::PipelinePositionFor,
        Self::Obligation,
        Self::Discharged,
        Self::Undischarged,
        Self::CiteCount,
        Self::InDegree,
        Self::OutDegree,
        Self::DischargeCount,
        Self::Freshness,
        Self::Flux,
        Self::GitMtime,
        Self::Recent,
        Self::TokenEstimate,
        Self::Search,
        Self::Read,
        Self::ReadFull,
        Self::Match,
        Self::Schema,
        Self::Predicates,
        Self::Verbs,
        Self::Describe,
        Self::SourceOf,
        Self::Examples,
        Self::Sources,
    ];

    pub(crate) fn from_predicate(predicate: &PredicateRef) -> Option<Self> {
        if predicate.module.is_some() {
            return None;
        }
        match predicate.name.as_str() {
            "upstream" => Some(Self::Upstream),
            "downstream" => Some(Self::Downstream),
            "impact" => Some(Self::Impact),
            "neighborhood" => Some(Self::Neighborhood),
            "terminal" => Some(Self::Terminal),
            "active" => Some(Self::Active),
            "settled" => Some(Self::Settled),
            "pipeline_position" => Some(Self::PipelinePosition),
            "pipeline_position_for" => Some(Self::PipelinePositionFor),
            "obligation" => Some(Self::Obligation),
            "discharged" => Some(Self::Discharged),
            "undischarged" => Some(Self::Undischarged),
            "cite_count" => Some(Self::CiteCount),
            "in_degree" => Some(Self::InDegree),
            "out_degree" => Some(Self::OutDegree),
            "discharge_count" => Some(Self::DischargeCount),
            "freshness" => Some(Self::Freshness),
            "flux" => Some(Self::Flux),
            "git_mtime" => Some(Self::GitMtime),
            "recent" => Some(Self::Recent),
            "token_estimate" => Some(Self::TokenEstimate),
            "search" => Some(Self::Search),
            "read" => Some(Self::Read),
            "read_full" => Some(Self::ReadFull),
            "match" => Some(Self::Match),
            "schema" => Some(Self::Schema),
            "predicates" => Some(Self::Predicates),
            "verbs" => Some(Self::Verbs),
            "describe" => Some(Self::Describe),
            "source_of" => Some(Self::SourceOf),
            "examples" => Some(Self::Examples),
            "sources" => Some(Self::Sources),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Upstream => "upstream",
            Self::Downstream => "downstream",
            Self::Impact => "impact",
            Self::Neighborhood => "neighborhood",
            Self::Terminal => "terminal",
            Self::Active => "active",
            Self::Settled => "settled",
            Self::PipelinePosition => "pipeline_position",
            Self::PipelinePositionFor => "pipeline_position_for",
            Self::Obligation => "obligation",
            Self::Discharged => "discharged",
            Self::Undischarged => "undischarged",
            Self::CiteCount => "cite_count",
            Self::InDegree => "in_degree",
            Self::OutDegree => "out_degree",
            Self::DischargeCount => "discharge_count",
            Self::Freshness => "freshness",
            Self::Flux => "flux",
            Self::GitMtime => "git_mtime",
            Self::Recent => "recent",
            Self::TokenEstimate => "token_estimate",
            Self::Search => "search",
            Self::Read => "read",
            Self::ReadFull => "read_full",
            Self::Match => "match",
            Self::Schema => "schema",
            Self::Predicates => "predicates",
            Self::Verbs => "verbs",
            Self::Describe => "describe",
            Self::SourceOf => "source_of",
            Self::Examples => "examples",
            Self::Sources => "sources",
        }
    }

    pub(crate) fn signature(self) -> PrimitiveSignature {
        match self {
            Self::Upstream => PrimitiveSignature {
                parameters: &["h", "anc"],
                sealed: true,
            },
            Self::Downstream => PrimitiveSignature {
                parameters: &["h", "desc"],
                sealed: true,
            },
            Self::Impact => PrimitiveSignature {
                parameters: &["h", "x", "depth"],
                sealed: true,
            },
            Self::Neighborhood => PrimitiveSignature {
                parameters: &["h", "depth", "member"],
                sealed: true,
            },
            Self::Terminal
            | Self::Active
            | Self::Settled
            | Self::Obligation
            | Self::Discharged
            | Self::Undischarged => PrimitiveSignature {
                parameters: &["h"],
                sealed: false,
            },
            Self::PipelinePosition => PrimitiveSignature {
                parameters: &["h", "n"],
                sealed: false,
            },
            Self::PipelinePositionFor => PrimitiveSignature {
                parameters: &["s", "n"],
                sealed: false,
            },
            Self::CiteCount
            | Self::InDegree
            | Self::OutDegree
            | Self::DischargeCount
            | Self::TokenEstimate => PrimitiveSignature {
                parameters: &["h", "n"],
                sealed: true,
            },
            Self::Freshness | Self::Recent => PrimitiveSignature {
                parameters: &["h", "days"],
                sealed: true,
            },
            Self::Flux => PrimitiveSignature {
                parameters: &["h", "days", "delta"],
                sealed: true,
            },
            Self::GitMtime => PrimitiveSignature {
                parameters: &["file", "instant"],
                sealed: true,
            },
            Self::Search => PrimitiveSignature {
                parameters: &[
                    "query",
                    "handle",
                    "span_id",
                    "score",
                    "reason",
                    "field",
                    "low_confidence",
                ],
                sealed: true,
            },
            Self::Read => PrimitiveSignature {
                parameters: &[
                    "handle",
                    "budget",
                    "span_id",
                    "text",
                    "start_line",
                    "end_line",
                    "tokens",
                ],
                sealed: true,
            },
            Self::ReadFull => PrimitiveSignature {
                parameters: &["handle", "content"],
                sealed: true,
            },
            Self::Match => PrimitiveSignature {
                parameters: &["pattern", "handle", "line", "snippet"],
                sealed: true,
            },
            Self::Schema => PrimitiveSignature {
                parameters: &[
                    "name",
                    "kind",
                    "signature",
                    "determinism",
                    "source_provenance",
                ],
                sealed: true,
            },
            Self::Predicates => PrimitiveSignature {
                parameters: &["name", "doc", "source_file", "source_lines"],
                sealed: true,
            },
            Self::Verbs => PrimitiveSignature {
                parameters: &["name", "query", "doc", "output_schema"],
                sealed: true,
            },
            Self::Describe => PrimitiveSignature {
                parameters: &["name", "doc"],
                sealed: true,
            },
            Self::SourceOf => PrimitiveSignature {
                parameters: &["name", "file", "lines"],
                sealed: true,
            },
            Self::Examples => PrimitiveSignature {
                parameters: &["name", "example"],
                sealed: true,
            },
            Self::Sources => PrimitiveSignature {
                parameters: &["name", "recognizes", "capabilities", "doc"],
                sealed: true,
            },
        }
    }

    pub(crate) fn is_soft(self) -> bool {
        !self.signature().sealed
    }

    pub(crate) fn graph_anchor_positions(self) -> Option<&'static [usize]> {
        match self {
            Self::Upstream | Self::Downstream | Self::Impact => Some(&[0, 1]),
            Self::Neighborhood => Some(&[0, 2]),
            Self::Terminal
            | Self::Active
            | Self::Settled
            | Self::PipelinePosition
            | Self::PipelinePositionFor
            | Self::Obligation
            | Self::Discharged
            | Self::Undischarged
            | Self::CiteCount
            | Self::InDegree
            | Self::OutDegree
            | Self::DischargeCount
            | Self::Freshness
            | Self::Flux
            | Self::GitMtime
            | Self::Recent
            | Self::TokenEstimate
            | Self::Search
            | Self::Read
            | Self::ReadFull
            | Self::Match
            | Self::Schema
            | Self::Predicates
            | Self::Verbs
            | Self::Describe
            | Self::SourceOf
            | Self::Examples
            | Self::Sources => None,
        }
    }

    pub(crate) fn required_bound_inputs(self) -> &'static [RequiredPrimitiveInput] {
        match self {
            Self::Search => &[RequiredPrimitiveInput {
                position: 0,
                argument: "query",
            }],
            Self::Read => &[
                RequiredPrimitiveInput {
                    position: 0,
                    argument: "handle",
                },
                RequiredPrimitiveInput {
                    position: 1,
                    argument: "budget",
                },
            ],
            Self::ReadFull => &[RequiredPrimitiveInput {
                position: 0,
                argument: "handle",
            }],
            Self::Recent => &[RequiredPrimitiveInput {
                position: 1,
                argument: "days",
            }],
            Self::Match => &[
                RequiredPrimitiveInput {
                    position: 0,
                    argument: "pattern",
                },
                RequiredPrimitiveInput {
                    position: 1,
                    argument: "handle",
                },
            ],
            Self::Upstream
            | Self::Downstream
            | Self::Impact
            | Self::Neighborhood
            | Self::Terminal
            | Self::Active
            | Self::Settled
            | Self::PipelinePosition
            | Self::PipelinePositionFor
            | Self::Obligation
            | Self::Discharged
            | Self::Undischarged
            | Self::CiteCount
            | Self::InDegree
            | Self::OutDegree
            | Self::DischargeCount
            | Self::Freshness
            | Self::Flux
            | Self::TokenEstimate
            | Self::GitMtime
            | Self::Schema
            | Self::Predicates
            | Self::Verbs
            | Self::Describe
            | Self::SourceOf
            | Self::Examples
            | Self::Sources => &[],
        }
    }
}

pub(crate) fn primitive_signatures() -> impl Iterator<Item = (PredicateRef, PrimitiveSignature)> {
    PrimitivePredicate::ALL.iter().copied().map(|primitive| {
        (
            PredicateRef::new(Ident::new_unchecked(primitive.name())),
            primitive.signature(),
        )
    })
}
