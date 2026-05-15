use crate::runtime::ast::{Ident, PredicateRef};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrimitivePredicate {
    Upstream,
    Downstream,
    Impact,
    Neighborhood,
    CiteCount,
    InDegree,
    OutDegree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PrimitiveSignature {
    pub(crate) parameters: &'static [&'static str],
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
            "cite_count" => Some(Self::CiteCount),
            "in_degree" => Some(Self::InDegree),
            "out_degree" => Some(Self::OutDegree),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Upstream => "upstream",
            Self::Downstream => "downstream",
            Self::Impact => "impact",
            Self::Neighborhood => "neighborhood",
            Self::CiteCount => "cite_count",
            Self::InDegree => "in_degree",
            Self::OutDegree => "out_degree",
        }
    }

    pub(crate) fn signature(self) -> PrimitiveSignature {
        match self {
            Self::Upstream => PrimitiveSignature {
                parameters: &["h", "anc"],
            },
            Self::Downstream => PrimitiveSignature {
                parameters: &["h", "desc"],
            },
            Self::Impact => PrimitiveSignature {
                parameters: &["h", "x", "depth"],
            },
            Self::Neighborhood => PrimitiveSignature {
                parameters: &["h", "depth", "member"],
            },
            Self::CiteCount | Self::InDegree | Self::OutDegree => PrimitiveSignature {
                parameters: &["h", "n"],
            },
        }
    }
}

pub(crate) fn primitive_signatures() -> impl Iterator<Item = (PredicateRef, PrimitiveSignature)> {
    [
        PrimitivePredicate::Upstream,
        PrimitivePredicate::Downstream,
        PrimitivePredicate::Impact,
        PrimitivePredicate::Neighborhood,
        PrimitivePredicate::CiteCount,
        PrimitivePredicate::InDegree,
        PrimitivePredicate::OutDegree,
    ]
    .into_iter()
    .map(|primitive| {
        (
            PredicateRef::new(Ident::new_unchecked(primitive.name())),
            primitive.signature(),
        )
    })
}
