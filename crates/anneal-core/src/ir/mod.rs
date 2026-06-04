//! Internal logical identifiers and schema metadata for the physical runtime.

pub(crate) mod ids;
pub(crate) mod interner;
// Phase 1 of anneal-kftp proves lowering before the executor consumes it.
#[allow(dead_code)]
pub(crate) mod plan;
pub(crate) mod schema;
