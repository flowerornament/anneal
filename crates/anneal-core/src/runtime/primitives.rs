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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PrimitiveSignature {
    pub(crate) parameters: &'static [&'static str],
    pub(crate) sealed: bool,
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
        }
    }

    pub(crate) fn is_soft(self) -> bool {
        !self.signature().sealed
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
    ]
    .into_iter()
    .map(|primitive| {
        (
            PredicateRef::new(Ident::new_unchecked(primitive.name())),
            primitive.signature(),
        )
    })
}
