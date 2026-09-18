//! Claude Code config-directory helpers shared by the Claude Code harness.

use std::path::PathBuf;

use anyhow::Result;

/// Resolve the Claude config directory.
///
/// Reads `$CLAUDE_CONFIG_DIR` if set, otherwise falls back to `~/.claude`.
//
/// TODO(REMOTE-1209): Use the transcript path reported by our hook.
pub(crate) fn claude_config_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        return Ok(PathBuf::from(dir));
    }
    home_dir_for_claude_config()
        .map(|h| h.join(".claude"))
        .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))
}

/// In tests on Windows, `dirs::home_dir()` ignores `HOME`, so we check it
/// manually so that tests can override the home directory.
pub(super) fn home_dir_for_claude_config() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(home) = std::env::var_os("HOME")
        && !home.is_empty()
    {
        return Some(PathBuf::from(home));
    }
    dirs::home_dir()
}
