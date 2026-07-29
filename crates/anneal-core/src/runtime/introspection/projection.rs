//! Stable tuple and signature formatting shared by introspection projections.

use super::super::ast::SourceLocation;
use super::{Tuple, string_value};

/// Renders a source location's line component for `source_of`.
pub(super) fn source_line_text(location: &SourceLocation) -> String {
    if location.line == 0 {
        "unknown".to_string()
    } else {
        location.line.to_string()
    }
}

/// Builds one stable `schema` tuple from already-derived catalog fields.
pub(super) fn schema_tuple(
    name: &str,
    kind: &str,
    signature: &str,
    determinism: &str,
    source_provenance: &str,
) -> Tuple {
    Tuple(vec![
        string_value(name),
        string_value(kind),
        string_value(signature),
        string_value(determinism),
        string_value(source_provenance),
    ])
}

/// Renders a callable signature from its effective parameter order.
pub(super) fn call_signature(name: &str, parameters: &[impl AsRef<str>]) -> String {
    let params = parameters
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name}({params})")
}
