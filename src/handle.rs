use camino::Utf8PathBuf;
use serde::Serialize;

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
}

// ---------------------------------------------------------------------------
// Test factories
// ---------------------------------------------------------------------------

#[cfg(test)]
impl Handle {
    pub(crate) fn test_file(id: &str, status: Option<&str>) -> Self {
        Self {
            id: id.to_string(),
            kind: HandleKind::File(Utf8PathBuf::from(id)),
            status: status.map(String::from),
            file_path: Some(Utf8PathBuf::from(id)),
            metadata: HandleMetadata::default(),
        }
    }

    pub(crate) fn test_label(prefix: &str, number: u32, status: Option<&str>) -> Self {
        Self {
            id: format!("{prefix}-{number}"),
            kind: HandleKind::Label {
                prefix: prefix.to_string(),
                number,
            },
            status: status.map(String::from),
            file_path: None,
            metadata: HandleMetadata::default(),
        }
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
