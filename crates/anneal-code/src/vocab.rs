pub(crate) const SOURCE_NAME: &str = "code";
pub(crate) const DEFAULT_SOURCE_EXTENSIONS: &[&str] = &["rs", "ex", "exs"];
pub(crate) const DEFAULT_CONTENT_BUDGET_BYTES: usize = 1024 * 1024;
pub(crate) const DEFAULT_MEMBER_DOC_BUDGET_BYTES: usize = 16 * 1024;

pub(crate) mod config_key {
    pub(crate) const EEP48_BEAM: &str = "code.eep48_beam";
    pub(crate) const EEP48_BEAM_DIR: &str = "code.eep48_beam_dir";
    pub(crate) const EEP48_DOC_CHUNK: &str = "code.eep48_doc_chunk";
    pub(crate) const RUSTDOC_JSON: &str = "code.rustdoc_json";
    pub(crate) const SOURCE_ROOT: &str = "code.source_root";
    pub(crate) const SOURCE_EXTENSION: &str = "code.source_extension";
    pub(crate) const PACKAGE: &str = "code.package";
    pub(crate) const ARTIFACT_MANIFEST: &str = "code.artifact_manifest";
    pub(crate) const ARTIFACT_REVISION: &str = "code.artifact_revision";
    pub(crate) const CONTENT_BUDGET_BYTES: &str = "code.content_budget_bytes";
    pub(crate) const MEMBER_DOC_BUDGET_BYTES: &str = "code.member_doc_budget_bytes";
}

pub(crate) mod meta_key {
    pub(crate) const QUALIFIED_NAME: &str = "code.qualified_name";
    pub(crate) const KIND: &str = "code.kind";
    pub(crate) const VISIBILITY: &str = "code.visibility";
    pub(crate) const DEPRECATED_NOTE: &str = "code.deprecated.note";
    pub(crate) const DEPRECATED_SINCE: &str = "code.deprecated.since";
    pub(crate) const PACKAGE: &str = "code.package";
    pub(crate) const ARTIFACT_PATH: &str = "code.artifact.path";
    pub(crate) const ARTIFACT_FORMAT: &str = "code.artifact.format";
    pub(crate) const ARTIFACT_FORMAT_VERSION: &str = "code.artifact.format_version";
    pub(crate) const ARTIFACT_REVISION: &str = "code.artifact.revision";
    pub(crate) const ARTIFACT_REVISION_STATE: &str = "code.artifact.revision_state";
    pub(crate) const PACKAGE_VERSION: &str = "code.package.version";
    pub(crate) const IMPLEMENTS_KIND: &str = "code.implements.kind";
    pub(crate) const IMPLEMENTS_SIGNATURE: &str = "code.implements.signature";
    pub(crate) const CONTENT_TRUNCATED: &str = "code.content_truncated";
    pub(crate) const CONTENT_BUDGET_DISPOSITION: &str = "code.content_budget_disposition";
    pub(crate) const EXTERNAL_CLASS: &str = "code.external_class";
    pub(crate) const CONTENT_BUDGET_ROOT_DISPOSITION: &str = "code.content_budget.disposition";
    pub(crate) const CONTENT_BUDGET_BYTES: &str = "code.content_budget.bytes";
    pub(crate) const MEMBER_DOC_ITEM_BYTES: &str = "code.content_budget.member_doc_item_bytes";
    pub(crate) const DOC_BYTES: &str = "code.content.doc_bytes";
    pub(crate) const MEMBER_DOC_BYTES: &str = "code.content.member_doc_bytes";
    pub(crate) const STRUCTURAL_DOC_BYTES: &str = "code.content.structural_doc_bytes";
    pub(crate) const SIGNATURE_BYTES: &str = "code.content.signature_bytes";
    pub(crate) const CLASS: &str = "code.class";
    pub(crate) const OBLIGATION: &str = "code.obligation";
    pub(crate) const OBLIGATION_COUNT: &str = "code.obligation.count";
    pub(crate) const VERSION_TAG: &str = "code.version_tag";
    pub(crate) const DOCS_SOURCE: &str = "code.docs_source";
    pub(crate) const DOC_STATE: &str = "code.doc_state";
    pub(crate) const HIDDEN: &str = "code.hidden";
    pub(crate) const SINCE: &str = "code.since";
}

pub(crate) mod relation_value {
    pub(crate) const ARTIFACT_REVISION_UNKNOWN: &str = "artifact_revision_unknown";
    pub(crate) const BUDGET_COMPLETE: &str = "complete";
    pub(crate) const BUDGET_TRUNCATED: &str = "truncated";
    pub(crate) const FIRST_PARAGRAPH: &str = "first_paragraph";
    pub(crate) const PER_ITEM_CAP: &str = "per_item_cap";
    pub(crate) const CLASS_GENERATED: &str = "generated";
    pub(crate) const CLASS_PRIVATE: &str = "private";
    pub(crate) const CLASS_PUBLIC_API: &str = "public-api";
    pub(crate) const CLASS_TEST: &str = "test";
    pub(crate) const DOC_STATE_DOCUMENTED: &str = "documented";
    pub(crate) const DOC_STATE_HIDDEN: &str = "hidden";
    pub(crate) const DOC_STATE_MISSING: &str = "missing";
    pub(crate) const DOC_STATE_NONE: &str = "none";
    pub(crate) const DOC_SOURCE_BEAM: &str = "beam_docs_chunk";
    pub(crate) const DOC_SOURCE_EXTERNAL: &str = "external_doc_chunk";
    pub(crate) const DOC_SOURCE_MISSING: &str = "missing_or_stripped";
}

pub(crate) mod edge_kind {
    pub(crate) const CONTAINS: &str = "Contains";
    pub(crate) const CITES: &str = "Cites";
    pub(crate) const IMPLEMENTS: &str = "Implements";
    pub(crate) const USES_TYPE: &str = "UsesType";
}

pub(crate) mod concern_name {
    pub(crate) const CODE_FIXME: &str = "code.fixme";
    pub(crate) const CODE_TODO: &str = "code.todo";
}
