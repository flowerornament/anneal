//! Markdown adapter for anneal v2.
//!
//! The first Phase 1 slice keeps this adapter intentionally small: it
//! proves the `Source` boundary by emitting source-qualified facts for
//! markdown files. Full v1.x parity remains tracked separately.

use std::fs;

use anneal_core::{
    ConfigKey, ContentFact, EdgeFact, FactBatch, FactBatchMode, FactIdentity, HandleFact, MetaFact,
    NativeId, OriginUri, Pattern, Revision, SearchInfo, Source, SourceCapabilities, SourceContext,
    SourceError, SourceInfo, SourceName, SpanFact,
};
use camino::{Utf8Path, Utf8PathBuf};
use serde_yaml_ng::Value;
use walkdir::WalkDir;

const SOURCE_NAME: &str = "markdown";

/// Markdown `Source` implementation.
#[derive(Clone, Debug, Default)]
pub struct MarkdownSource;

impl Source for MarkdownSource {
    fn describe(&self) -> SourceInfo {
        SourceInfo {
            name: SOURCE_NAME,
            recognizes: vec![Pattern::new("**/*.md")],
            doc: "Extracts markdown files, frontmatter metadata, content spans, and frontmatter edges.",
            config_keys: vec![
                ConfigKey::required("md.file_extension"),
                ConfigKey::required("md.scan_root"),
                ConfigKey::optional("md.scan_exclude"),
                ConfigKey::optional("md.label_pattern"),
                ConfigKey::optional("md.linear_namespace"),
                ConfigKey::optional("md.version_pattern"),
                ConfigKey::optional("md.section_min_depth"),
                ConfigKey::optional("md.section_max_depth"),
            ],
            capabilities: SourceCapabilities {
                supports_git_ref: true,
                supports_time_snapshot: false,
                supports_incremental: false,
                live_only: false,
            },
            search: Some(SearchInfo {
                reason_vocabulary: vec!["title-match", "frontmatter-key-match", "body-substring"],
                fields: vec!["title", "body", "frontmatter"],
                low_confidence_threshold: 0.5,
            }),
        }
    }

    fn extract(&self, cx: &SourceContext<'_>) -> Result<FactBatch, SourceError> {
        if cx.time_ref.is_some() {
            return Err(SourceError::UnsupportedTimeRef(
                cx.time_ref.clone().expect("checked above"),
            ));
        }

        let generation = cx.next_generation();
        let mut batch = FactBatch::new(
            cx.corpus.clone(),
            SourceName::from(SOURCE_NAME),
            FactBatchMode::FullSnapshot,
            generation,
        );
        let extensions: Vec<&str> = cx
            .config_facts
            .values("md.file_extension")
            .chain(std::iter::once(".md"))
            .collect();

        for root in cx.roots {
            cx.cancellation.check()?;
            let scan_root = cx
                .config_facts
                .first("md.scan_root")
                .map_or_else(|| root.clone(), |configured| root.join(configured));
            scan_directory(cx, &scan_root, root, &extensions, &mut batch)?;
        }

        Ok(batch)
    }
}

fn scan_directory(
    cx: &SourceContext<'_>,
    scan_root: &Utf8Path,
    corpus_root: &Utf8Path,
    extensions: &[&str],
    batch: &mut FactBatch,
) -> Result<(), SourceError> {
    for entry in WalkDir::new(scan_root).sort_by_file_name() {
        cx.cancellation.check()?;
        let entry = entry.map_err(|err| SourceError::Other(err.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = Utf8PathBuf::from_path_buf(entry.path().to_path_buf())
            .map_err(|path| SourceError::Other(format!("non-UTF-8 path: {}", path.display())))?;
        if !extensions
            .iter()
            .any(|extension| path.as_str().ends_with(extension))
        {
            continue;
        }
        extract_file(cx, corpus_root, &path, batch)?;
    }
    Ok(())
}

fn extract_file(
    cx: &SourceContext<'_>,
    corpus_root: &Utf8Path,
    path: &Utf8Path,
    batch: &mut FactBatch,
) -> Result<(), SourceError> {
    let content = fs::read_to_string(path).map_err(|err| SourceError::io(path, err))?;
    let relative = path.strip_prefix(corpus_root).unwrap_or(path);
    let file_id = relative.to_string();
    let (frontmatter, body, body_start_line) = split_frontmatter(&content);
    let parsed = frontmatter.and_then(parse_frontmatter);
    let revision = Revision::from(format!("{:016x}", fnv1a64(content.as_bytes())));
    let identity = identity(cx, &file_id, path, revision.clone());
    let status = parsed
        .as_ref()
        .and_then(|value| value_string(value, "status"));

    batch.handles.push(HandleFact {
        identity: identity.clone(),
        id: file_id.clone(),
        kind: "file".to_string(),
        status,
        namespace: String::new(),
        file: file_id.clone(),
        line: 1,
        date: parsed.as_ref().and_then(|value| {
            value_string(value, "updated").or_else(|| value_string(value, "date"))
        }),
        area: area_for(relative),
        summary: summary_for(parsed.as_ref(), body),
    });

    emit_frontmatter(cx, &file_id, path, parsed.as_ref(), &revision, batch);
    emit_content(cx, &file_id, path, body, body_start_line, &revision, batch);
    Ok(())
}

fn emit_frontmatter(
    cx: &SourceContext<'_>,
    file_id: &str,
    path: &Utf8Path,
    parsed: Option<&Value>,
    revision: &Revision,
    batch: &mut FactBatch,
) {
    let Some(Value::Mapping(mapping)) = parsed else {
        return;
    };
    for (key, value) in mapping {
        let Some(key) = key.as_str() else {
            continue;
        };
        for scalar in scalar_values(value) {
            let fact_identity = identity(
                cx,
                &format!("{file_id}:meta:{key}:{scalar}"),
                path,
                revision.clone(),
            );
            batch.meta.push(MetaFact {
                identity: fact_identity,
                handle: file_id.to_string(),
                key: key.to_string(),
                value: scalar.clone(),
            });
            if let Some(kind) = edge_kind_for_frontmatter(key) {
                batch.edges.push(EdgeFact {
                    identity: identity(
                        cx,
                        &format!("{file_id}:edge:{kind}:{scalar}"),
                        path,
                        revision.clone(),
                    ),
                    from: file_id.to_string(),
                    to: scalar,
                    kind: kind.to_string(),
                    file: file_id.to_string(),
                    line: 1,
                });
            }
        }
    }
}

fn emit_content(
    cx: &SourceContext<'_>,
    file_id: &str,
    path: &Utf8Path,
    body: &str,
    body_start_line: u32,
    revision: &Revision,
    batch: &mut FactBatch,
) {
    if body.trim().is_empty() {
        return;
    }
    let line_count = u32::try_from(body.lines().count()).unwrap_or(u32::MAX);
    let token_count = u32::try_from(body.split_whitespace().count()).unwrap_or(u32::MAX);
    let span_id = format!("{file_id}#full");
    let identity = identity(cx, &format!("{file_id}:content"), path, revision.clone());
    batch.content.push(ContentFact {
        identity: identity.clone(),
        handle: file_id.to_string(),
        span_id: span_id.clone(),
        lines: line_count,
        text: body.to_string(),
        tokens: token_count,
    });
    batch.spans.push(SpanFact {
        identity,
        id: span_id,
        handle: file_id.to_string(),
        start_line: body_start_line,
        end_line: body_start_line.saturating_add(line_count.saturating_sub(1)),
        summary: first_non_heading_line(body),
    });
}

fn identity(
    cx: &SourceContext<'_>,
    native_id: &str,
    path: &Utf8Path,
    revision: Revision,
) -> FactIdentity {
    FactIdentity::new(
        cx.corpus.clone(),
        SourceName::from(SOURCE_NAME),
        NativeId::from(native_id),
        OriginUri::from(format!("file://{path}")),
        revision,
        cx.next_generation(),
    )
}

fn split_frontmatter(content: &str) -> (Option<&str>, &str, u32) {
    let Some(rest) = content.strip_prefix("---\n") else {
        return (None, content, 1);
    };
    if let Some(pos) = rest.find("\n---\n") {
        let yaml = &rest[..pos];
        let body = &rest[pos + 5..];
        let yaml_lines = u32::try_from(yaml.lines().count()).unwrap_or(u32::MAX);
        (Some(yaml), body, yaml_lines.saturating_add(3))
    } else {
        (None, content, 1)
    }
}

fn parse_frontmatter(yaml: &str) -> Option<Value> {
    serde_yaml_ng::from_str::<Value>(yaml).ok()
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    let Value::Mapping(mapping) = value else {
        return None;
    };
    mapping
        .get(Value::String(key.to_string()))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn scalar_values(value: &Value) -> Vec<String> {
    match value {
        Value::String(value) => vec![value.clone()],
        Value::Number(value) => vec![value.to_string()],
        Value::Bool(value) => vec![value.to_string()],
        Value::Sequence(values) => values.iter().flat_map(scalar_values).collect(),
        _ => Vec::new(),
    }
}

fn edge_kind_for_frontmatter(key: &str) -> Option<&'static str> {
    match key {
        "depends-on" => Some("depends_on"),
        "superseded-by" => Some("supersedes"),
        "verifies" => Some("verifies"),
        "discharges" => Some("discharges"),
        _ => None,
    }
}

fn summary_for(parsed: Option<&Value>, body: &str) -> String {
    parsed
        .and_then(|value| value_string(value, "purpose").or_else(|| value_string(value, "note")))
        .unwrap_or_else(|| first_non_heading_line(body))
}

fn first_non_heading_line(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or_default()
        .chars()
        .take(240)
        .collect()
}

fn area_for(path: &Utf8Path) -> String {
    path.components()
        .next()
        .map_or_else(String::new, |component| component.as_str().to_string())
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use anneal_core::{
        ActorContext, CancellationToken, ConfigFacts, CorpusId, Generation, SourceContext,
    };
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn markdown_source_extracts_file_handle_content_and_frontmatter_edge() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 tempdir");
        fs::write(
            root.join("a.md"),
            "---\nstatus: draft\ndepends-on: b.md\npurpose: Test file\n---\n# A\nBody text.\n",
        )
        .expect("write fixture");
        let roots = vec![root.clone()];
        let config = ConfigFacts::new(vec![
            ("md.file_extension".to_string(), ".md".to_string()),
            ("md.scan_root".to_string(), ".".to_string()),
        ]);
        let cx = SourceContext {
            corpus: CorpusId::from("test"),
            roots: &roots,
            config_facts: &config,
            time_ref: None,
            previous_generation: Some(Generation::new(7)),
            actor: ActorContext {
                actor: "test".to_string(),
                capabilities: BTreeSet::new(),
            },
            cancellation: CancellationToken::new(),
        };

        let batch = MarkdownSource.extract(&cx).expect("extract");

        assert_eq!(batch.generation, Generation::new(8));
        assert_eq!(batch.handles.len(), 1);
        assert_eq!(batch.handles[0].id, "a.md");
        assert_eq!(batch.handles[0].status.as_deref(), Some("draft"));
        assert_eq!(batch.content.len(), 1);
        assert_eq!(batch.spans.len(), 1);
        assert!(batch.edges.iter().any(|edge| edge.to == "b.md"));
        assert!(batch.meta.iter().any(|meta| meta.key == "purpose"));
    }
}
