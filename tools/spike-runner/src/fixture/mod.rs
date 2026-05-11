//! Hand-built large-corpus-shaped fixture for engine spike testing.
//!
//! Realistic enough to exercise MVS-1..9 without depending on the live large-corpus
//! corpus. Models v17 formal-model supersedes-chain, OQ labels with
//! settled-status downstream pressure, and stale dependencies.

#[derive(Clone, Debug, serde::Serialize)]
pub struct Handle {
    pub id: &'static str,
    pub kind: &'static str,
    pub status: &'static str,
    pub namespace: &'static str,
    pub area: &'static str,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Edge {
    pub from: &'static str,
    pub to: &'static str,
    pub kind: &'static str,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: &'static str,
    pub handle: &'static str,
    pub file: &'static str,
    pub line: u32,
}

/// Statuses considered terminal in this fixture (mirrors large-corpus's lattice).
pub const TERMINAL_STATUSES: &[&str] = &[
    "superseded",
    "archived",
    "historical",
    "incorporated",
    "decision",
    "resolved",
];

pub fn handles() -> Vec<Handle> {
    vec![
        // formal-model: v17 authoritative, history chain
        Handle { id: "formal-model/v17.md", kind: "file", status: "authoritative",
                 namespace: "", area: "formal-model" },
        Handle { id: "formal-model/v16.md", kind: "file", status: "superseded",
                 namespace: "", area: "formal-model" },
        Handle { id: "formal-model/v15.md", kind: "file", status: "superseded",
                 namespace: "", area: "formal-model" },
        Handle { id: "formal-model/v14.md", kind: "file", status: "superseded",
                 namespace: "", area: "formal-model" },
        // compiler: active and stale
        Handle { id: "compiler/jit-spec.md", kind: "file", status: "draft",
                 namespace: "", area: "compiler" },
        Handle { id: "compiler/jit-stale.md", kind: "file", status: "superseded",
                 namespace: "", area: "compiler" },
        Handle { id: "compiler/exec.md", kind: "file", status: "current",
                 namespace: "", area: "compiler" },
        // research-log: active reference
        Handle { id: "research-log/2026-04-jit.md", kind: "file", status: "research",
                 namespace: "", area: "research-log" },
        // OQ labels: open and resolved
        Handle { id: "OQ-22", kind: "label", status: "open",
                 namespace: "OQ", area: "formal-model" },
        Handle { id: "OQ-23", kind: "label", status: "open",
                 namespace: "OQ", area: "formal-model" },
        Handle { id: "OQ-99", kind: "label", status: "resolved",
                 namespace: "OQ", area: "formal-model" },
        Handle { id: "OQ-60", kind: "label", status: "open",
                 namespace: "OQ", area: "compiler" },
    ]
}

pub fn edges() -> Vec<Edge> {
    vec![
        // v17 depends on OQs
        Edge { from: "formal-model/v17.md", to: "OQ-22", kind: "depends_on" },
        Edge { from: "formal-model/v17.md", to: "OQ-23", kind: "depends_on" },
        Edge { from: "formal-model/v17.md", to: "OQ-60", kind: "depends_on" },
        // supersedes chain
        Edge { from: "formal-model/v17.md", to: "formal-model/v16.md", kind: "supersedes" },
        Edge { from: "formal-model/v16.md", to: "formal-model/v15.md", kind: "supersedes" },
        Edge { from: "formal-model/v15.md", to: "formal-model/v14.md", kind: "supersedes" },
        // jit-spec depends on OQ-22 (settled-status downstream) and on stale file
        Edge { from: "compiler/jit-spec.md", to: "OQ-22", kind: "depends_on" },
        Edge { from: "compiler/jit-spec.md", to: "compiler/jit-stale.md", kind: "depends_on" },
        // exec depends on jit-spec
        Edge { from: "compiler/exec.md", to: "compiler/jit-spec.md", kind: "depends_on" },
        // research-log cites v17
        Edge { from: "research-log/2026-04-jit.md", to: "formal-model/v17.md", kind: "cites" },
    ]
}

pub fn diagnostics() -> Vec<Diagnostic> {
    vec![
        Diagnostic { code: "W001", severity: "warning",
                     handle: "compiler/jit-spec.md", file: "compiler/jit-spec.md", line: 12 },
    ]
}
