//! Source-adapter projections for runtime introspection.

use crate::source::{SourceCapabilities, SourceInfo};

use super::{Tuple, list_value, string_value};

/// Enumerates adapter capabilities in the stable order exposed by `sources`.
pub(super) fn source_capability_names(
    capabilities: &SourceCapabilities,
    supports_search: bool,
) -> impl Iterator<Item = &'static str> {
    [
        (capabilities.supports_git_ref, "supports_git_ref"),
        (
            capabilities.supports_time_snapshot,
            "supports_time_snapshot",
        ),
        (capabilities.supports_incremental, "supports_incremental"),
        (capabilities.live_only, "live_only"),
        (supports_search, "search"),
    ]
    .into_iter()
    .filter_map(|(enabled, name)| enabled.then_some(name))
}

/// Projects one adapter descriptor into the `sources` primitive's tuple shape.
pub(super) fn source_tuple(source: &SourceInfo) -> Tuple {
    Tuple(vec![
        string_value(source.name),
        list_value(source.recognizes.iter().map(|pattern| pattern.0.as_str())),
        list_value(source_capability_names(
            &source.capabilities,
            source.search.is_some(),
        )),
        string_value(source.doc),
    ])
}
