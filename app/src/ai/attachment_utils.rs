//! Shared utilities for file-attachment handling.
use std::path::{Path, PathBuf};

/// Returns the per-session directory for downloading file attachments,
/// based on the agent's working directory.
pub(crate) fn attachments_download_dir(working_dir: &Path) -> PathBuf {
    working_dir.join(".warp").join("attachments")
}
