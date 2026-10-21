//! Immutable regular-file trees, read through the owning repository's validated command.

use std::collections::BTreeMap;
use std::io::{Seek, Write};
use std::process::{Command, Stdio};

use super::{RepositoryContext, git_command};

/// An exact commit tree. Symlinks and submodules are deliberately not materialized.
#[derive(Debug)]
pub struct RepositorySnapshot {
    pub commit: String,
    pub files: BTreeMap<String, String>,
    pub skipped_non_utf8_paths: usize,
}

impl RepositoryContext {
    /// Resolve a Git commit id once and inventory its regular blobs without changing a checkout.
    /// Symbolic refs and jj change ids are not accepted. Nothing is fetched.
    pub fn read_snapshot(&self, revision: &str) -> Result<RepositorySnapshot, String> {
        if !(4..=64).contains(&revision.len()) || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("revision must be a Git commit id (full or unambiguous prefix), not a symbolic ref or jj change id".into());
        }
        let output = self
            .snapshot_command()?
            .args(["rev-parse", "--verify", &format!("{revision}^{{commit}}")])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "absent or ambiguous Git commit id {revision}: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let commit = String::from_utf8(output.stdout)
            .map_err(|e| e.to_string())?
            .trim()
            .to_string();
        let tree = self
            .snapshot_command()?
            .args(["ls-tree", "-r", "-z", &commit])
            .output()
            .map_err(|e| e.to_string())?;
        if !tree.status.success() {
            return Err("failed to read pinned repository tree".into());
        }
        let mut files = BTreeMap::new();
        let mut skipped_non_utf8_paths = 0;
        for record in tree.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let tab = record
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or("invalid Git tree entry")?;
            let header = std::str::from_utf8(&record[..tab]).map_err(|e| e.to_string())?;
            let fields = header.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 3 {
                return Err("invalid Git tree header".into());
            }
            if !matches!(fields[0], "100644" | "100755") {
                continue;
            }
            if fields[1] != "blob" {
                return Err("regular tree entry is not a blob".into());
            }
            let Ok(path) = std::str::from_utf8(&record[tab + 1..]) else {
                skipped_non_utf8_paths += 1;
                continue;
            };
            if path
                .split('/')
                .any(|p| p.is_empty() || matches!(p, "." | ".." | ".git" | ".jj"))
                || path.starts_with('/')
            {
                return Err("unsafe path in repository tree".into());
            }
            files.insert(path.to_string(), fields[2].to_string());
        }
        Ok(RepositorySnapshot {
            commit,
            files,
            skipped_non_utf8_paths,
        })
    }

    fn snapshot_command(&self) -> Result<Command, String> {
        self.direct_git_root
            .as_deref()
            .map(git_command)
            .or_else(|| self.jj_git_command())
            .ok_or_else(|| "mount has no validated Git backing".into())
    }

    /// Read only requested regular blobs from an already resolved tree, in one batch.
    pub fn read_snapshot_files(
        &self,
        snapshot: &RepositorySnapshot,
        paths: &[String],
    ) -> Result<BTreeMap<String, Vec<u8>>, String> {
        let mut entries = BTreeMap::new();
        let mut input = tempfile::tempfile().map_err(|e| e.to_string())?;
        for path in paths {
            if let Some(oid) = snapshot.files.get(path) {
                entries.insert(path.clone(), oid.clone());
            }
        }
        if entries.is_empty() {
            return Ok(BTreeMap::new());
        }
        for oid in entries.values() {
            writeln!(input, "{oid}").map_err(|e| e.to_string())?;
        }
        input.rewind().map_err(|e| e.to_string())?;
        let blobs = self
            .snapshot_command()?
            .args(["cat-file", "--batch"])
            .stdin(Stdio::from(input))
            .output()
            .map_err(|e| e.to_string())?;
        if !blobs.status.success() {
            return Err("failed to read pinned repository blobs".into());
        }
        let mut remaining = blobs.stdout.as_slice();
        let mut files = BTreeMap::new();
        for (path, oid) in entries {
            let newline = remaining
                .iter()
                .position(|b| *b == b'\n')
                .ok_or("missing Git blob header")?;
            let header = std::str::from_utf8(&remaining[..newline]).map_err(|e| e.to_string())?;
            let fields = header.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 3 || fields[0] != oid || fields[1] != "blob" {
                return Err("mismatched Git blob response".into());
            }
            let size = fields[2].parse::<usize>().map_err(|e| e.to_string())?;
            remaining = &remaining[newline + 1..];
            let body = remaining.get(..size).ok_or("truncated Git blob")?;
            if remaining.get(size) != Some(&b'\n') {
                return Err("missing Git blob terminator".into());
            }
            files.insert(path, body.to_vec());
            remaining = &remaining[size + 1..];
        }
        if !remaining.is_empty() {
            return Err("unexpected trailing Git blobs".into());
        }
        Ok(files)
    }
}
