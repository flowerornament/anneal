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
    TokenEstimate,
    Search,
    Read,
    ReadFull,
    Match,
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
            "token_estimate" => Some(Self::TokenEstimate),
            "search" => Some(Self::Search),
            "read" => Some(Self::Read),
            "read_full" => Some(Self::ReadFull),
            "match" => Some(Self::Match),
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
            Self::TokenEstimate => "token_estimate",
            Self::Search => "search",
            Self::Read => "read",
            Self::ReadFull => "read_full",
            Self::Match => "match",
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
            Self::Freshness => PrimitiveSignature {
                parameters: &["h", "days"],
                sealed: true,
            },
            Self::Flux => PrimitiveSignature {
                parameters: &["h", "days", "delta"],
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
            | Self::TokenEstimate
            | Self::Search
            | Self::Read
            | Self::ReadFull
            | Self::Match => None,
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
            | Self::TokenEstimate => &[],
        }
    }
}

pub(crate) fn primitive_signatures() -> impl Iterator<Item = (PredicateRef, PrimitiveSignature)> {
    [
        PrimitivePredicate::Upstream,
        PrimitivePredicate::Downstream,
        PrimitivePredicate::Impact,
        PrimitivePredicate::Neighborhood,
        PrimitivePredicate::Terminal,
        PrimitivePredicate::Active,
        PrimitivePredicate::Settled,
        PrimitivePredicate::PipelinePosition,
        PrimitivePredicate::PipelinePositionFor,
        PrimitivePredicate::Obligation,
        PrimitivePredicate::Discharged,
        PrimitivePredicate::Undischarged,
        PrimitivePredicate::CiteCount,
        PrimitivePredicate::InDegree,
        PrimitivePredicate::OutDegree,
        PrimitivePredicate::DischargeCount,
        PrimitivePredicate::Freshness,
        PrimitivePredicate::Flux,
        PrimitivePredicate::TokenEstimate,
        PrimitivePredicate::Search,
        PrimitivePredicate::Read,
        PrimitivePredicate::ReadFull,
        PrimitivePredicate::Match,
    ]
    .into_iter()
    .map(|primitive| {
        (
            PredicateRef::new(Ident::new_unchecked(primitive.name())),
            primitive.signature(),
        )
    })
}
