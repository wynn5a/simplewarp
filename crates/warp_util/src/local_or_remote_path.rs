use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::standardized_path::StandardizedPath;

/// Uniquely identifies where a file lives. Used across both the buffer model and the editor/view
/// layers as the canonical file-identity type.
///
/// Single-variant, but kept as an enum: its `{"Local": ...}` serde shape is persisted in restored
/// code-pane sources and skill references.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum LocalOrRemotePath {
    /// File on the local filesystem.
    Local(PathBuf),
}

impl LocalOrRemotePath {
    /// Returns `true` if this is a `Local` location.
    pub fn is_local(&self) -> bool {
        matches!(self, LocalOrRemotePath::Local(_))
    }

    /// Returns the standardized path component of the location.
    pub fn path_component(&self) -> StandardizedPath {
        match self {
            LocalOrRemotePath::Local(path) => StandardizedPath::from_local_absolute_unchecked(path),
        }
    }

    /// Returns the file name component for display (e.g. tab titles).
    pub fn display_name(&self) -> &str {
        match self {
            LocalOrRemotePath::Local(path) => path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default(),
        }
    }

    /// Returns a displayable path string.
    pub fn display_path(&self) -> String {
        match self {
            LocalOrRemotePath::Local(path) => path.to_string_lossy().to_string(),
        }
    }

    /// Returns this location's parent.
    pub fn parent(&self) -> Option<LocalOrRemotePath> {
        match self {
            LocalOrRemotePath::Local(path) => path
                .parent()
                .map(|parent| LocalOrRemotePath::Local(parent.to_path_buf())),
        }
    }

    /// Returns the file name component.
    pub fn file_name(&self) -> Option<&str> {
        match self {
            LocalOrRemotePath::Local(path) => path.file_name().and_then(|name| name.to_str()),
        }
    }

    /// Returns whether this location starts with `base`.
    pub fn starts_with(&self, base: &LocalOrRemotePath) -> bool {
        match (self, base) {
            (LocalOrRemotePath::Local(path), LocalOrRemotePath::Local(base)) => {
                path.starts_with(base)
            }
        }
    }

    /// Returns the local path.
    pub fn to_local_path(&self) -> Option<&Path> {
        match self {
            LocalOrRemotePath::Local(path) => Some(path.as_path()),
        }
    }

    /// Joins a (typically repo-relative) segment onto this location.
    ///
    /// If `segment` is itself absolute, the standard `Path::join` replacement semantics apply (the
    /// joined result is `segment`).
    pub fn join(&self, segment: &str) -> LocalOrRemotePath {
        match self {
            LocalOrRemotePath::Local(path) => LocalOrRemotePath::Local(path.join(segment)),
        }
    }

    /// If `file` starts with this location's path, returns the relative remainder as a `String`.
    /// Returns `None` when `file` is not under this location.
    pub fn strip_repo_prefix(&self, file: &LocalOrRemotePath) -> Option<String> {
        match (self, file) {
            (LocalOrRemotePath::Local(repo), LocalOrRemotePath::Local(f)) => f
                .strip_prefix(repo)
                .ok()
                .map(|p| p.to_string_lossy().into_owned()),
        }
    }
}

impl From<PathBuf> for LocalOrRemotePath {
    fn from(path: PathBuf) -> Self {
        LocalOrRemotePath::Local(path)
    }
}

impl From<LocalOrRemotePath> for PathBuf {
    fn from(location: LocalOrRemotePath) -> Self {
        match location {
            LocalOrRemotePath::Local(path) => path,
        }
    }
}

impl From<&LocalOrRemotePath> for PathBuf {
    fn from(location: &LocalOrRemotePath) -> Self {
        match location {
            LocalOrRemotePath::Local(path) => path.clone(),
        }
    }
}
#[cfg(test)]
#[path = "local_or_remote_path_tests.rs"]
mod tests;
