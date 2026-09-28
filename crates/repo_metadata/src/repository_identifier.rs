use std::path::Path;

use warp_util::local_or_remote_path::LocalOrRemotePath;
use warp_util::standardized_path::StandardizedPath;

/// Identifies a repository on the local filesystem by its standardized path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepositoryIdentifier(pub StandardizedPath);

impl RepositoryIdentifier {
    /// Convenience constructor for a repository identifier.
    pub fn local(path: StandardizedPath) -> Self {
        Self(path)
    }

    /// Convenience constructor that creates an identifier from a
    /// `std::path::Path`. Returns `None` if the path is not absolute or
    /// contains non-UTF-8 characters.
    pub fn try_local(path: &Path) -> Option<Self> {
        StandardizedPath::try_from_local(path).ok().map(Self)
    }

    /// Converts this identifier to a `LocalOrRemotePath`.
    ///
    /// Returns `None` when the `StandardizedPath` cannot be converted to a local `PathBuf`
    /// (cross-platform edge case).
    pub fn to_local_or_remote_path(&self) -> Option<LocalOrRemotePath> {
        self.0.to_local_path().map(LocalOrRemotePath::Local)
    }
}
