//! Engine-spike harness for anneal v2.0 language redesign.
//!
//! Phase 0 of `.design/2026-05-03-language-redesign.md`. Provides typed
//! domain primitives ([`types`]), a hand-built corpus shape ([`fixture`]),
//! and an NDJSON-streaming reporting harness ([`capability`]) used by
//! engine binaries (`src/bin/ascent_spike.rs`, future `crepe_spike.rs`).
//!
//! See `.design/2026-05-07-engine-spike-and-parity-protocol.md` for the
//! MVS test catalog and pass/fail criteria.

pub mod capability;
pub mod fixture;
pub mod loader;
pub mod program;
pub mod types;

pub use capability::{emit, CapabilityReport, Verdict};
pub use fixture::{
    Edge, Handle, PendingEdge, Snapshot, EDGES, HANDLES, LINEAR_NAMESPACES, PENDING_EDGES,
    SNAPSHOTS,
};
pub use types::{
    Area, DiagnosticCode, EdgeKind, FilePath, HandleId, HandleKind, Namespace, Severity,
    SnapshotId, Status, PIPELINE_ORDERING,
};
