use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;

use crate::graph::DiGraph;

/// Arena index into `DiGraph::nodes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub(crate) struct NodeId(u32);

impl NodeId {
    pub(crate) fn new(index: u32) -> Self {
        Self(index)
    }

    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

/// The five kinds of handle per KB-D2.
///
/// Kind determines discovery, resolution, and valid states.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub(crate) enum HandleKind {
    /// A markdown file, identified by its path relative to root.
    File(Utf8PathBuf),
    /// A heading range within a parent file.
    Section { parent: NodeId, heading: String },
    /// A cross-reference label (e.g., OQ-64, A-10).
    Label { prefix: String, number: u32 },
    /// A version of a versioned artifact (e.g., v17 of formal-model).
    Version { artifact: NodeId, version: u32 },
    /// An external URL referenced from frontmatter.
    External { url: String },
}

impl HandleKind {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::File(_) => "file",
            Self::Section { .. } => "section",
            Self::Label { .. } => "label",
            Self::Version { .. } => "version",
            Self::External { .. } => "external",
        }
    }
}

/// A handle is a triple (identity, kind, state) per KB-D1.
///
/// Handles are the only objects in the system. Every question anneal
/// answers is about handles.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Handle {
    /// Unique identity string for this handle.
    pub(crate) id: String,
    /// What kind of handle this is.
    pub(crate) kind: HandleKind,
    /// Frontmatter `status` value (`None` if no frontmatter or no status field).
    pub(crate) status: Option<String>,
    /// Source file for non-File handles (the file where this handle was discovered).
    pub(crate) file_path: Option<Utf8PathBuf>,
    /// Resolved file date: `updated:` > `date:` frontmatter > filename `YYYY-MM-DD` prefix.
    pub(crate) date: Option<chrono::NaiveDate>,
    /// Source-file size in bytes. Only populated for `HandleKind::File`.
    pub(crate) size_bytes: Option<u32>,
    /// Additional metadata extracted from frontmatter.
    pub(crate) metadata: HandleMetadata,
}

impl Handle {
    /// Whether this handle's status is in the terminal set.
    pub(crate) fn is_terminal(&self, lattice: &crate::lattice::Lattice) -> bool {
        self.status
            .as_ref()
            .is_some_and(|s| lattice.terminal.contains(s))
    }

    /// Create a File handle.
    pub(crate) fn file(
        path: Utf8PathBuf,
        status: Option<String>,
        date: Option<chrono::NaiveDate>,
        size_bytes: Option<u32>,
        metadata: HandleMetadata,
    ) -> Self {
        Self {
            id: path.to_string(),
            file_path: Some(path.clone()),
            kind: HandleKind::File(path),
            status,
            date,
            size_bytes,
            metadata,
        }
    }

    /// Create a Section handle.
    pub(crate) fn section(parent: NodeId, heading: String, file_path: Utf8PathBuf) -> Self {
        Self {
            id: format!("{}#{}", file_path, heading.to_lowercase().replace(' ', "-")),
            kind: HandleKind::Section { parent, heading },
            status: None,
            file_path: Some(file_path),
            date: None,
            size_bytes: None,
            metadata: HandleMetadata::default(),
        }
    }

    /// Create a Label handle.
    pub(crate) fn label(prefix: String, number: u32, file_path: Option<Utf8PathBuf>) -> Self {
        Self {
            id: format!("{prefix}-{number}"),
            kind: HandleKind::Label { prefix, number },
            status: None,
            file_path,
            date: None,
            size_bytes: None,
            metadata: HandleMetadata::default(),
        }
    }

    /// Create a Version handle.
    pub(crate) fn version(
        artifact: NodeId,
        version: u32,
        artifact_id: &str,
        status: Option<String>,
    ) -> Self {
        Self {
            id: format!("{artifact_id}-v{version}"),
            kind: HandleKind::Version { artifact, version },
            status,
            file_path: None,
            date: None,
            size_bytes: None,
            metadata: HandleMetadata::default(),
        }
    }

    /// Create an External (URL) handle.
    pub(crate) fn external(url: String, file_path: Option<Utf8PathBuf>) -> Self {
        Self {
            id: url.clone(),
            kind: HandleKind::External { url },
            status: None,
            file_path,
            date: None,
            size_bytes: None,
            metadata: HandleMetadata::default(),
        }
    }
}

pub(crate) fn resolved_file<'a>(handle: &'a Handle, graph: &'a DiGraph) -> Option<&'a Utf8Path> {
    handle.file_path.as_deref().or_else(|| match &handle.kind {
        HandleKind::Version { artifact, .. } => graph.node(*artifact).file_path.as_deref(),
        _ => None,
    })
}

/// Metadata extracted from YAML frontmatter fields.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct HandleMetadata {
    /// The `updated:` frontmatter field, parsed as a date.
    pub(crate) updated: Option<chrono::NaiveDate>,
    /// The `superseded-by:` frontmatter field.
    pub(crate) superseded_by: Option<String>,
    /// The `depends-on:` frontmatter field (list of handle identities).
    pub(crate) depends_on: Vec<String>,
    /// The `discharges:` frontmatter field (list of handle identities).
    pub(crate) discharges: Vec<String>,
    /// The `verifies:` frontmatter field (list of handle identities).
    pub(crate) verifies: Vec<String>,
    pub(crate) purpose: Option<String>,
    pub(crate) note: Option<String>,
}

impl Handle {
    /// Preferred orientation summary: purpose → note → body fallback.
    pub(crate) fn summary<'a>(&'a self, body_fallback: Option<&'a str>) -> Option<&'a str> {
        self.metadata
            .purpose
            .as_deref()
            .or(self.metadata.note.as_deref())
            .or(body_fallback)
    }
}

// ---------------------------------------------------------------------------
// Test factories
// ---------------------------------------------------------------------------

#[cfg(test)]
impl Handle {
    pub(crate) fn test_file(id: &str, status: Option<&str>) -> Self {
        Self::file(
            Utf8PathBuf::from(id),
            status.map(String::from),
            None,
            None,
            HandleMetadata::default(),
        )
    }

    pub(crate) fn test_file_with_date(
        id: &str,
        status: Option<&str>,
        date: chrono::NaiveDate,
    ) -> Self {
        Self::file(
            Utf8PathBuf::from(id),
            status.map(String::from),
            Some(date),
            None,
            HandleMetadata::default(),
        )
    }

    pub(crate) fn test_label(prefix: &str, number: u32, status: Option<&str>) -> Self {
        let mut h = Self::label(prefix.to_string(), number, None);
        h.status = status.map(String::from);
        h
    }
}

#[cfg(test)]
mod tests {
    use super::HandleKind;

    #[test]
    fn external_handle_kind_reports_external_tag() {
        let kind = HandleKind::External {
            url: "https://example.com/spec".to_string(),
        };

        assert_eq!(kind.as_str(), "external");
    }
}
