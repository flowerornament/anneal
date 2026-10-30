//! Capture current Cargo membership and relocate it for immutable historical reads.
use super::Result;
use atlas_core::{
    discovery::{self, Snapshot},
    revision::Revision,
};
use std::path::Path;
pub(super) fn capture() -> Result<Snapshot> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(std::env::current_dir().map_err(|e| e.to_string())?)
        .no_deps()
        .exec()
        .map_err(|e| e.to_string())?;
    capture_metadata(metadata)
}
/// Preserve Anneal's per-package source-tree population before capture reads targets.
pub(super) fn capture_metadata(mut metadata: cargo_metadata::Metadata) -> Result<Snapshot> {
    let members = metadata.workspace_members.clone();
    for package in metadata
        .packages
        .iter_mut()
        .filter(|p| members.contains(&p.id))
    {
        let src = package
            .manifest_path
            .parent()
            .ok_or("manifest without parent")?
            .join("src");
        package
            .targets
            .retain(|target| target.src_path.starts_with(&src));
    }
    discovery::snapshot_metadata(&metadata).map_err(|e| e.to_string())
}

/// Diagnostics remain visible without changing Anneal's legacy stdout schemas.
pub(super) fn diagnostic_lines(snapshot: &Snapshot) -> Vec<String> {
    let mut lines: Vec<_> = snapshot
        .source_diagnostics()
        .iter()
        .map(|d| {
            format!(
                "atlas source: {}{}: {}",
                d.file,
                d.line.map_or_else(String::new, |line| format!(":{line}")),
                d.message
            )
        })
        .collect();
    lines.sort();
    lines.dedup();
    lines
}

pub(super) fn historical(current: &Snapshot, revision: &Revision) -> Result<Snapshot> {
    let mut metadata = current.metadata().clone();
    let old = metadata.workspace_root.clone();
    let destination = revision.path().canonicalize().map_err(|e| e.to_string())?;
    metadata.workspace_root =
        cargo_metadata::camino::Utf8PathBuf::from_path_buf(destination.clone())
            .map_err(|e| format!("non-UTF8 revision path: {}", e.display()))?;
    let members = metadata.workspace_members.clone();
    for package in metadata
        .packages
        .iter_mut()
        .filter(|p| members.contains(&p.id))
    {
        package.manifest_path = relocate(
            old.as_std_path(),
            &destination,
            package.manifest_path.as_std_path(),
        )?;
        for target in &mut package.targets {
            target.src_path = relocate(
                old.as_std_path(),
                &destination,
                target.src_path.as_std_path(),
            )?;
        }
    }
    // Native historical walks enumerate the old source tree using current membership;
    // a target introduced after that revision is not a required historical file.
    for package in metadata
        .packages
        .iter_mut()
        .filter(|p| members.contains(&p.id))
    {
        package.targets.retain(|target| target.src_path.is_file());
    }
    capture_metadata(metadata)
}
fn relocate(
    root: &Path,
    destination: &Path,
    path: &Path,
) -> Result<cargo_metadata::camino::Utf8PathBuf> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("historical source escapes workspace: {}", path.display()))?;
    cargo_metadata::camino::Utf8PathBuf::from_path_buf(destination.join(relative))
        .map_err(|e| format!("non-UTF8 historical path: {}", e.display()))
}
