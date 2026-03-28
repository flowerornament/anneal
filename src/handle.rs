use camino::Utf8PathBuf;
use serde::Serialize;

/// Arena index into `DiGraph::nodes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct NodeId(u32);

impl NodeId {
    pub fn new(index: u32) -> Self {
        Self(index)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// The four kinds of handle per KB-D2.
///
/// Kind determines discovery, resolution, and valid states.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum HandleKind {
    /// A markdown file, identified by its path relative to root.
    File(Utf8PathBuf),
    /// A heading range within a parent file.
    Section { parent: NodeId, heading: String },
    /// A cross-reference label (e.g., OQ-64, A-10).
    Label { prefix: String, number: u32 },
    /// A version of a versioned artifact (e.g., v17 of formal-model).
    Version { artifact: NodeId, version: u32 },
}

/// A handle is a triple (identity, kind, state) per KB-D1.
///
/// Handles are the only objects in the system. Every question anneal
/// answers is about handles.
#[derive(Clone, Debug, Serialize)]
pub struct Handle {
    /// Unique identity string for this handle.
    pub id: String,
    /// What kind of handle this is.
    pub kind: HandleKind,
    /// Frontmatter `status` value (`None` if no frontmatter or no status field).
    pub status: Option<String>,
    /// Source file for non-File handles (the file where this handle was discovered).
    pub file_path: Option<Utf8PathBuf>,
    /// Additional metadata extracted from frontmatter.
    pub metadata: HandleMetadata,
}

/// Metadata extracted from YAML frontmatter fields.
#[derive(Clone, Debug, Default, Serialize)]
pub struct HandleMetadata {
    /// The `updated:` frontmatter field, parsed as a date.
    pub updated: Option<chrono::NaiveDate>,
    /// The `superseded-by:` frontmatter field.
    pub superseded_by: Option<String>,
    /// The `depends-on:` frontmatter field (list of handle identities).
    pub depends_on: Vec<String>,
    /// The `discharges:` frontmatter field (list of handle identities).
    pub discharges: Vec<String>,
    /// The `verifies:` frontmatter field (list of handle identities).
    pub verifies: Vec<String>,
}
