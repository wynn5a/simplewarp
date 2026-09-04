//! Touched-workspace derivation for local-to-cloud handoff (REMOTE-1486).
//!
//! Given the flat list of filesystem paths an agent run has touched, this module
//! produces a [`TouchedWorkspace`] enumerating the distinct git repos and orphan
//! files the local agent has touched.
//!
//! Path extraction is sync and pure (no I/O), and the workspace derivation is async
//! (one filesystem walk-up per unique repo). Callers run them in sequence
//! off the main thread; see `app/src/workspace/view.rs::start_local_to_cloud_handoff`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tokio::fs as tokio_fs;

/// The collection of git repos and orphan files the local agent has touched in the
/// active conversation. Drives both the snapshot upload plan and the modal's env-
/// overlap status row.
#[derive(Clone, Debug, Default)]
pub(crate) struct TouchedWorkspace {
    pub repos: Vec<TouchedRepo>,
    /// Files touched outside any `.git` directory.
    /// They're captured as raw file contents in the snapshot manifest.
    pub orphan_files: Vec<PathBuf>,
}

/// A single git repo touched by the local agent.
#[derive(Clone, Debug)]
pub(crate) struct TouchedRepo {
    /// Absolute path to the working tree root (the directory containing `.git`).
    pub git_root: PathBuf,
}

/// Derive the `TouchedWorkspace` from a flat list of absolute paths.
///
/// Walks each path up to the nearest `.git` directory; paths whose walk-up doesn't
/// find one go into `orphan_files`.
///
/// `paths` must already be absolute and must come from paths the agent
/// actually wrote to. That gate is what makes the orphan-file branch safe —
/// we never stage a read-only path like `~/.ssh/id_rsa` for upload.
pub(crate) async fn derive_touched_workspace(paths: Vec<PathBuf>) -> TouchedWorkspace {
    if paths.is_empty() {
        return TouchedWorkspace::default();
    }

    let mut git_roots: Vec<PathBuf> = Vec::new();
    let mut orphan_files: Vec<PathBuf> = Vec::new();
    let mut seen_roots: HashSet<PathBuf> = HashSet::new();

    for path in paths {
        match find_git_root(&path).await {
            Some(root) => {
                if seen_roots.insert(root.clone()) {
                    git_roots.push(root);
                }
            }
            None => {
                if tokio_fs::metadata(&path).await.is_ok_and(|m| m.is_file()) {
                    orphan_files.push(path);
                }
            }
        }
    }

    let repos: Vec<TouchedRepo> = git_roots
        .into_iter()
        .map(|git_root| TouchedRepo { git_root })
        .collect();

    TouchedWorkspace {
        repos,
        orphan_files,
    }
}

/// Walk `path` up to find the nearest enclosing `.git` directory and return its parent
/// (the working-tree root). Returns `None` if no `.git` is found.
async fn find_git_root(path: &Path) -> Option<PathBuf> {
    let mut cursor: Option<&Path> = if tokio_fs::metadata(path).await.is_ok_and(|m| m.is_dir()) {
        Some(path)
    } else {
        path.parent()
    };
    while let Some(dir) = cursor {
        let candidate = dir.join(".git");
        if tokio_fs::try_exists(&candidate).await.unwrap_or(false) {
            return Some(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    None
}

#[cfg(test)]
#[path = "touched_repos_tests.rs"]
mod tests;
