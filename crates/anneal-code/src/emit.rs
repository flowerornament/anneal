use super::{
    BTreeMap, Command, DEFAULT_CONTENT_BUDGET_BYTES, DEFAULT_MEMBER_DOC_BUDGET_BYTES, FactBatch,
    FactIdentity, HandleFact, ItemKind, MetaFact, NativeId, OriginUri, Revision, SOURCE_NAME,
    Utf8Path, Utf8PathBuf, Visibility, meta_key, normalize_path_inside_root,
    normalize_relative_path, relation_value,
};

#[derive(Clone, Debug)]
pub(super) struct ContentBudgetReport {
    pub(super) disposition: String,
    pub(super) content_budget_bytes: usize,
    pub(super) member_doc_budget_bytes: usize,
    pub(super) doc_bytes: usize,
    pub(super) member_doc_bytes: usize,
    pub(super) structural_doc_bytes: usize,
    pub(super) signature_bytes: usize,
}

impl Default for ContentBudgetReport {
    fn default() -> Self {
        Self {
            disposition: relation_value::BUDGET_COMPLETE.to_string(),
            content_budget_bytes: DEFAULT_CONTENT_BUDGET_BYTES,
            member_doc_budget_bytes: DEFAULT_MEMBER_DOC_BUDGET_BYTES,
            doc_bytes: 0,
            member_doc_bytes: 0,
            structural_doc_bytes: 0,
            signature_bytes: 0,
        }
    }
}

pub(super) fn package_root_file(root: &Utf8Path, source_root: &Utf8Path) -> String {
    if root.join("mix.exs").is_file() {
        return "mix.exs".to_string();
    }
    if root.join(source_root).join("mix.exs").is_file() {
        let path = source_root.join("mix.exs");
        return normalize_relative_path(
            path.as_str(),
            anneal_core::RelativePathPolicy::ALLOW_EMPTY,
        )
        .map_or_else(|| path.to_string(), |path| path.to_string());
    }
    if root.join("Cargo.toml").is_file() {
        return "Cargo.toml".to_string();
    }
    if root.join(source_root).join("Cargo.toml").is_file() {
        let path = source_root.join("Cargo.toml");
        return normalize_relative_path(
            path.as_str(),
            anneal_core::RelativePathPolicy::ALLOW_EMPTY,
        )
        .map_or_else(|| path.to_string(), |path| path.to_string());
    }
    "Cargo.toml".to_string()
}

pub(super) fn git_version_tags(source_abs: &Utf8Path) -> Vec<String> {
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(source_abs)
        .args(["tag", "--points-at", "HEAD", "--sort=refname"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(super) fn version_handle_id(tag: &str) -> String {
    format!("code-version:{tag}")
}

pub(super) fn meta_values(batch: &FactBatch, key: &str) -> BTreeMap<String, String> {
    batch
        .meta
        .iter()
        .filter(|meta| meta.key == key)
        .map(|meta| (meta.handle.clone(), meta.value.clone()))
        .collect()
}

pub(super) fn push_code_meta(
    batch: &mut FactBatch,
    root: &Utf8Path,
    revision: &Revision,
    handle: &str,
    file: &str,
    key: &str,
    value: &str,
) {
    let identity = code_identity(batch, root, revision, handle, file);
    push_meta_fact(batch, &identity, handle, key, value);
}

pub(super) fn push_meta_fact(
    batch: &mut FactBatch,
    identity: &FactIdentity,
    handle: &str,
    key: &str,
    value: &str,
) {
    batch.meta.push(MetaFact {
        identity: identity.clone(),
        handle: handle.to_string(),
        key: key.to_string(),
        value: value.to_string(),
    });
}

pub(super) fn ensure_external_code_handle(
    batch: &mut FactBatch,
    root: &Utf8Path,
    revision: &Revision,
    file: &str,
    handle: &str,
    qualified: &str,
) {
    if batch.handles.iter().any(|existing| existing.id == handle) {
        return;
    }
    let identity = code_identity(batch, root, revision, handle, file);
    batch.handles.push(HandleFact {
        identity: identity.clone(),
        id: handle.to_string(),
        kind: "external".to_string(),
        status: None,
        namespace: "code".to_string(),
        file: String::new(),
        line: 0,
        date: None,
        area: String::new(),
        summary: qualified.to_string(),
    });
    push_meta_fact(
        batch,
        &identity,
        handle,
        meta_key::QUALIFIED_NAME,
        qualified,
    );
    push_meta_fact(
        batch,
        &identity,
        handle,
        meta_key::EXTERNAL_CLASS,
        SOURCE_NAME,
    );
}

pub(super) fn emit_content_budget_meta(
    batch: &mut FactBatch,
    root: &Utf8Path,
    revision: &Revision,
    package_handle: &str,
    budget: &ContentBudgetReport,
) {
    let identity = code_identity(batch, root, revision, package_handle, package_handle);
    let values = [
        (
            meta_key::CONTENT_BUDGET_ROOT_DISPOSITION,
            budget.disposition.as_str(),
        ),
        (
            meta_key::CONTENT_BUDGET_BYTES,
            &budget.content_budget_bytes.to_string(),
        ),
        (
            meta_key::MEMBER_DOC_ITEM_BYTES,
            &budget.member_doc_budget_bytes.to_string(),
        ),
        (meta_key::DOC_BYTES, &budget.doc_bytes.to_string()),
        (
            meta_key::MEMBER_DOC_BYTES,
            &budget.member_doc_bytes.to_string(),
        ),
        (
            meta_key::STRUCTURAL_DOC_BYTES,
            &budget.structural_doc_bytes.to_string(),
        ),
        (
            meta_key::SIGNATURE_BYTES,
            &budget.signature_bytes.to_string(),
        ),
    ];
    for (key, value) in values {
        push_meta_fact(batch, &identity, package_handle, key, value);
    }
}

pub(super) fn code_identity(
    batch: &FactBatch,
    root: &Utf8Path,
    revision: &Revision,
    native_id: &str,
    file: &str,
) -> FactIdentity {
    let origin_uri = if file.is_empty() {
        format!("rustdoc://{native_id}")
    } else {
        format!("file://{}", root.join(file))
    };
    FactIdentity::new(
        batch.corpus.clone(),
        batch.source.clone(),
        NativeId::from(native_id.to_string()),
        OriginUri::from(origin_uri),
        revision.clone(),
        batch.generation,
    )
}

pub(super) fn normalize_code_source_path(
    root: &Utf8Path,
    source_root: &Utf8Path,
    raw_path: &str,
) -> Option<String> {
    let path = Utf8PathBuf::from(raw_path);
    let relative = if path.is_absolute() {
        normalize_path_inside_root(root, &path)?
    } else if path.starts_with(source_root) {
        path
    } else {
        source_root.join(path)
    };
    normalize_relative_path(
        relative.as_str(),
        anneal_core::RelativePathPolicy::STRICT_NON_EMPTY,
    )
    .map(|path| path.to_string())
}

pub(super) fn first_paragraph(text: &str) -> &str {
    text.split("\n\n").next().unwrap_or(text).trim_end()
}

pub(super) fn truncate_at_char_boundary(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].trim_end()
}

pub(super) fn item_kind_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Module => "module",
        ItemKind::ExternCrate => "extern_crate",
        ItemKind::Use => "use",
        ItemKind::Struct => "struct",
        ItemKind::StructField => "struct_field",
        ItemKind::Union => "union",
        ItemKind::Enum => "enum",
        ItemKind::Variant => "variant",
        ItemKind::Function => "function",
        ItemKind::TypeAlias => "type_alias",
        ItemKind::Constant => "constant",
        ItemKind::Trait => "trait",
        ItemKind::TraitAlias => "trait_alias",
        ItemKind::Impl => "impl",
        ItemKind::Static => "static",
        ItemKind::ExternType => "extern_type",
        ItemKind::Macro => "macro",
        ItemKind::ProcAttribute => "proc_attribute",
        ItemKind::ProcDerive => "proc_derive",
        ItemKind::AssocConst => "assoc_const",
        ItemKind::AssocType => "assoc_type",
        ItemKind::Primitive => "primitive",
        ItemKind::Keyword => "keyword",
        ItemKind::Attribute => "attribute",
    }
}

pub(super) fn visibility_name(visibility: &Visibility) -> &'static str {
    match visibility {
        Visibility::Public => "public",
        Visibility::Default => "default",
        Visibility::Crate => "crate",
        Visibility::Restricted { .. } => "restricted",
    }
}

pub(super) fn area_for(file: &str) -> String {
    Utf8Path::new(file)
        .parent()
        .map_or_else(String::new, ToString::to_string)
}

pub(super) fn token_count(text: &str) -> u32 {
    u32::try_from(text.split_whitespace().count()).unwrap_or(u32::MAX)
}
