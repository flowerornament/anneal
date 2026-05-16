//! Type-driven model of the anneal handle graph for engine-spike use.
//!
//! Each domain concept gets a distinct type so swapped arguments fail at
//! compile time. Enums are closed over the values the spec defines
//! (§4.1 / §8 of `.design/2026-05-03-language-redesign.md`) with one
//! `Other` extension variant where corpora extend the vocabulary.
//!
//! The terminal-status classification mirrors `anneal/src/lattice.rs`'s
//! canonical heuristic family (`TERMINAL_STATUS_HEURISTICS`); keeping the
//! mirror keyed to that source is `anneal-aj8`-adjacent debt that will
//! collapse when v2.0 ships and the lattice lives in `convergence.dl`.

use serde::Serialize;

// ---------------------------------------------------------------------------
// Identity newtypes
// ---------------------------------------------------------------------------

/// Stable handle identity. Files use their path; labels use `PREFIX-N`.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(transparent)]
pub struct HandleId(pub &'static str);

/// Area in the corpus tree — the top-level directory of a file handle.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(transparent)]
pub struct Area(pub &'static str);

/// Label namespace (e.g. `"OQ"`). Empty for non-label handles.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(transparent)]
pub struct Namespace(pub &'static str);

impl Namespace {
    pub const NONE: Self = Self("");
    pub const fn is_empty(self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(transparent)]
pub struct FilePath(pub &'static str);

/// ISO-8601 date string, when known. Optional because some fixture handles
/// lack a date (labels declared in undated files, etc.).
pub type IsoDate = &'static str;

/// The five kinds from spec §4.1.

#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HandleKind {
    File,
    Section,
    Label,
    Version,
    External,
}

impl HandleKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "file" => Some(Self::File),
            "section" => Some(Self::Section),
            "label" => Some(Self::Label),
            "version" => Some(Self::Version),
            "external" => Some(Self::External),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// EdgeKind — the kinds the parser emits and the impact traversal consumes
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    DependsOn,
    Supersedes,
    Cites,
    Discharges,
    Verifies,
    Affects,
    /// Extension point for corpora that define custom edge kinds.
    Other(&'static str),
}

impl EdgeKind {
    /// Parse an edge-kind string from anneal's JSON output. Falls back
    /// to [`EdgeKind::Other`] for unrecognized kinds; the string must
    /// be `'static`-leaked by the caller.
    pub fn parse(s: &'static str) -> Self {
        match s {
            "DependsOn" | "depends_on" => Self::DependsOn,
            "Supersedes" | "supersedes" => Self::Supersedes,
            "Cites" | "cites" => Self::Cites,
            "Discharges" | "discharges" => Self::Discharges,
            "Verifies" | "verifies" => Self::Verifies,
            "Affects" | "affects" => Self::Affects,
            _ => Self::Other(s),
        }
    }
}

// ---------------------------------------------------------------------------
// Status — closed enum over corpus statuses we model, with extension variant
// ---------------------------------------------------------------------------

/// Lifecycle status of a handle.
///
/// The variants below the `Other` boundary are recognized terminal
/// statuses per `anneal/src/lattice.rs::TERMINAL_STATUS_HEURISTICS`.
/// Above are common active statuses. Corpora with bespoke vocabulary
/// can use [`Status::Other`] without losing type safety on the closed set.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    // ---- Active statuses (common across anneal/large-corpus/host-corpus corpora) ----
    Raw,
    Draft,
    Research,
    Plan,
    Current,
    Active,
    Stable,
    Authoritative,
    Open,
    Living,
    // ---- Terminal heuristics (mirror src/lattice.rs:16) ----
    Superseded,
    Archived,
    Historical,
    Prior,
    Retired,
    Deprecated,
    Obsolete,
    Withdrawn,
    Cancelled,
    Closed,
    Resolved,
    Done,
    Completed,
    Incorporated,
    Digested,
    // ---- Extension for corpus-specific statuses ----
    Other(&'static str),
}

impl Status {
    /// True if this status is terminal under anneal's canonical lattice
    /// heuristic. Mirrors `anneal::lattice::is_terminal_status`.
    pub const fn is_terminal(self) -> bool {
        use Status::{
            Archived, Cancelled, Closed, Completed, Deprecated, Digested, Done, Historical,
            Incorporated, Obsolete, Prior, Resolved, Retired, Superseded, Withdrawn,
        };
        matches!(
            self,
            Superseded
                | Archived
                | Historical
                | Prior
                | Retired
                | Deprecated
                | Obsolete
                | Withdrawn
                | Cancelled
                | Closed
                | Resolved
                | Done
                | Completed
                | Incorporated
                | Digested
        )
    }

    /// Active is "not terminal" — the lattice's complement.
    pub const fn is_active(self) -> bool {
        !self.is_terminal()
    }

    /// Statuses considered "settled" — used by project predicates like
    /// `release_blocker` and downstream-pressure aggregation. Distinct from
    /// terminal: settled handles are alive but the work has crystallized.
    pub const fn is_settled(self) -> bool {
        matches!(
            self,
            Self::Authoritative | Self::Current | Self::Active | Self::Stable | Self::Living
        )
    }

    /// Position in a canonical pipeline ordering. `None` for terminal
    /// statuses and for any [`Status::Other`] variant. Matches the spec's
    /// `pipeline_position_for(s, n)` predicate from §8.
    pub fn pipeline_position(self) -> Option<usize> {
        PIPELINE_ORDERING.iter().position(|&s| s == self)
    }

    /// Parse a status string from frontmatter or anneal's JSON output.
    /// Unrecognized strings become [`Status::Other`]; the caller must
    /// pass a `'static`-leaked string.
    pub fn parse(s: &'static str) -> Self {
        match s {
            "raw" => Self::Raw,
            "draft" => Self::Draft,
            "research" => Self::Research,
            "plan" => Self::Plan,
            "current" => Self::Current,
            "active" => Self::Active,
            "stable" => Self::Stable,
            "authoritative" => Self::Authoritative,
            "open" => Self::Open,
            "living" => Self::Living,
            "superseded" => Self::Superseded,
            "archived" => Self::Archived,
            "historical" => Self::Historical,
            "prior" => Self::Prior,
            "retired" => Self::Retired,
            "deprecated" => Self::Deprecated,
            "obsolete" => Self::Obsolete,
            "withdrawn" => Self::Withdrawn,
            "cancelled" | "canceled" => Self::Cancelled,
            "closed" => Self::Closed,
            "resolved" => Self::Resolved,
            "done" => Self::Done,
            "completed" => Self::Completed,
            "incorporated" => Self::Incorporated,
            "digested" => Self::Digested,
            _ => Self::Other(s),
        }
    }
}

/// Canonical active-status ordering. Earlier = less settled.
pub const PIPELINE_ORDERING: &[Status] = &[
    Status::Raw,
    Status::Draft,
    Status::Research,
    Status::Plan,
    Status::Current,
    Status::Active,
    Status::Stable,
    Status::Authoritative,
];

/// Identifier for a point-in-time snapshot of corpus state. Spec's
/// `at(<ref>) { ... }` block translates at the engine level to "join on
/// `snapshot_handle(<ref>, ...)`"; the spike validates the underlying
/// relational primitive without modeling the surface syntax.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(transparent)]
pub struct SnapshotId(pub &'static str);

// ---------------------------------------------------------------------------
// DiagnosticCode + Severity — diagnostic IDs per §17 / LR-D8
// ---------------------------------------------------------------------------

/// Diagnostic codes from `.design/2026-05-03-language-redesign.md` §17.
///
/// Closed over the prelude-owned `E*`, `W*`, `I*`, `S*` prefixes per LR-R3.
/// User-defined diagnostics from `anneal.dl` would carry a project prefix —
/// the spike doesn't model those yet.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
pub enum DiagnosticCode {
    E001, // broken reference
    E002, // undischarged obligation
    W001, // stale dependency
    W002, // confidence gap
    W003, // index-file orphan
    W004, // implausible reference
    I001, // section-notation info
    S001, // candidate orphan
    S002, // candidate namespace
    S003, // pipeline stall
    S004, // sparse area
    S005, // concern group candidate
}

impl DiagnosticCode {
    pub const fn severity(self) -> Severity {
        use DiagnosticCode::{
            E001, E002, I001, S001, S002, S003, S004, S005, W001, W002, W003, W004,
        };
        match self {
            E001 | E002 => Severity::Error,
            W001 | W002 | W003 | W004 => Severity::Warning,
            I001 => Severity::Info,
            S001 | S002 | S003 | S004 | S005 => Severity::Suggestion,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
    Suggestion,
}

// ---------------------------------------------------------------------------
// Tests — type-level invariants
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_set_matches_anneal_lattice_heuristic() {
        // Mirrors src/lattice.rs::TERMINAL_STATUS_HEURISTICS (16 entries).
        // If anneal adds a new terminal heuristic, this list must grow with it.
        let terminals: Vec<Status> = vec![
            Status::Superseded,
            Status::Archived,
            Status::Historical,
            Status::Prior,
            Status::Retired,
            Status::Deprecated,
            Status::Obsolete,
            Status::Withdrawn,
            Status::Cancelled,
            Status::Closed,
            Status::Resolved,
            Status::Done,
            Status::Completed,
            Status::Incorporated,
            Status::Digested,
        ];
        for s in terminals {
            assert!(s.is_terminal(), "{s:?} should be terminal");
        }
    }

    #[test]
    fn common_active_statuses_are_not_terminal() {
        for s in [
            Status::Draft,
            Status::Current,
            Status::Authoritative,
            Status::Research,
            Status::Plan,
            Status::Open,
            Status::Active,
            Status::Stable,
            Status::Living,
        ] {
            assert!(s.is_active(), "{s:?} should be active");
        }
    }

    #[test]
    fn other_status_defaults_to_active() {
        assert!(Status::Other("provisional").is_active());
    }

    #[test]
    fn pipeline_position_advances_monotonically() {
        let positions: Vec<_> = PIPELINE_ORDERING
            .iter()
            .map(|s| {
                s.pipeline_position()
                    .expect("status in ordering must have a position")
            })
            .collect();
        assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "PIPELINE_ORDERING positions must be strictly increasing"
        );
    }

    #[test]
    fn terminal_statuses_have_no_pipeline_position() {
        for s in [
            Status::Superseded,
            Status::Archived,
            Status::Resolved,
            Status::Done,
        ] {
            assert_eq!(s.pipeline_position(), None);
        }
    }

    #[test]
    fn other_status_has_no_pipeline_position() {
        assert_eq!(Status::Other("provisional").pipeline_position(), None);
    }

    #[test]
    fn severity_dispatch_covers_every_code() {
        for (code, expected) in [
            (DiagnosticCode::E001, Severity::Error),
            (DiagnosticCode::E002, Severity::Error),
            (DiagnosticCode::W001, Severity::Warning),
            (DiagnosticCode::W004, Severity::Warning),
            (DiagnosticCode::I001, Severity::Info),
            (DiagnosticCode::S003, Severity::Suggestion),
        ] {
            assert_eq!(code.severity(), expected);
        }
    }
}
