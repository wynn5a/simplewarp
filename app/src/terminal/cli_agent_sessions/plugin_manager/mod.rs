pub(crate) mod claude;

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::PathBuf;
use std::{fmt, io};

use async_trait::async_trait;
use claude::ClaudeCodePluginManager;

use crate::terminal::CLIAgent;
use crate::terminal::model::session::LocalCommandExecutor;
use crate::terminal::shell::ShellType;

/// Error returned when plugin installation fails.
/// Carries a short message and the detailed command log; `Display` prints
/// both so the app log shows what was attempted.
#[derive(Debug)]
pub(crate) struct PluginInstallError {
    /// Short description of the failure.
    pub message: String,
    /// Detailed log of every command/step that was attempted.
    pub log: String,
}

impl fmt::Display for PluginInstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if !self.log.is_empty() {
            write!(f, "\n{}", self.log)?;
        }
        Ok(())
    }
}

impl std::error::Error for PluginInstallError {}

impl From<io::Error> for PluginInstallError {
    fn from(err: io::Error) -> Self {
        Self {
            message: err.to_string(),
            log: String::new(),
        }
    }
}

/// Compares two `X.Y.Z` version strings.
/// Returns `Ordering::Less` if `a < b`, etc.
/// Unparseable components are treated as 0.
pub(crate) fn compare_versions(a: &str, b: &str) -> Ordering {
    let parse = |s: &str| -> [u64; 3] {
        let mut parts = s.splitn(3, '.');
        let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        [major, minor, patch]
    };
    parse(a).cmp(&parse(b))
}

/// Runs a CLI subcommand through [`LocalCommandExecutor`], appending the
/// command and its output to `log`.
pub(crate) async fn run_cli_command_logged(
    cli_name: &str,
    args: &[&str],
    executor: &LocalCommandExecutor,
    env_vars: Option<HashMap<String, String>>,
    log: &mut String,
) -> Result<(), PluginInstallError> {
    let display_cmd = format!("{cli_name} {}", args.join(" "));
    log.push_str(&format!("$ {display_cmd}\n"));
    let result = executor
        .execute_local_command_in_login_shell(&display_cmd, None, env_vars)
        .await;
    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            for stream in [&stdout, &stderr] {
                if stream.is_empty() {
                    continue;
                }
                log.push_str(stream);
                if !stream.ends_with('\n') {
                    log.push('\n');
                }
            }
            if output.success() {
                log.push('\n');
                return Ok(());
            }
            Err(PluginInstallError {
                message: format!("'{display_cmd}' failed"),
                log: log.to_owned(),
            })
        }
        Err(err) => {
            log.push_str(&format!("error: {err}\n"));
            Err(PluginInstallError {
                message: format!("failed to run '{display_cmd}'"),
                log: log.clone(),
            })
        }
    }
}

/// Manages the Warp notification plugin for a specific CLI agent.
///
/// Each supported CLI agent has its own implementation that knows how to
/// check installation state and perform install/update operations.
#[async_trait]
pub(crate) trait CliAgentPluginManager: Send + Sync {
    /// The minimum plugin version required by this Warp build.
    fn minimum_plugin_version(&self) -> &'static str;

    /// Whether this agent supports one-click auto-install/update.
    /// When `false`, the footer always opens the manual instructions modal.
    fn can_auto_install(&self) -> bool;

    /// Whether the Warp notification plugin is installed.
    /// Default returns `false` (no filesystem check).
    fn is_installed(&self) -> bool {
        false
    }

    /// Whether the on-disk plugin version is below the minimum required.
    /// Default returns `false` (no filesystem check).
    fn needs_update(&self) -> bool {
        false
    }

    /// Whether this agent's Oz platform plugin is already installed.
    /// Default returns `true` because most agents do not have a platform plugin.
    fn is_platform_plugin_installed(&self) -> bool {
        true
    }
    /// Whether this agent's Oz platform plugin is below the minimum required version.
    /// Default returns `false` because most agents do not have a platform plugin.
    fn platform_plugin_needs_update(&self) -> bool {
        false
    }

    /// Whether the agent's plugin marketplace is currently overridden to a
    /// local filesystem path. This is used by local test flows to avoid
    /// clobbering a developer's marketplace override while still preserving
    /// normal install/update behavior in staging and production.
    fn has_local_marketplace_override(&self) -> bool {
        false
    }
    /// Install the Warp notification plugin.
    /// Default returns an error — only agents with `can_auto_install() == true` should override.
    async fn install(&self) -> Result<(), PluginInstallError> {
        Err(PluginInstallError {
            message: "Auto-install not supported for this agent".to_owned(),
            log: String::new(),
        })
    }

    /// Update the Warp notification plugin to the latest version.
    /// Default returns an error — only agents with `can_auto_install() == true` should override.
    async fn update(&self) -> Result<(), PluginInstallError> {
        Err(PluginInstallError {
            message: "Auto-update not supported for this agent".to_owned(),
            log: String::new(),
        })
    }

    /// Install the Oz platform plugin for this CLI agent, if one exists,
    /// which provides skills that third-party harnesses can use to interact with
    /// the Oz platform.
    /// Default is a no-op — only agents with a platform plugin should override.
    async fn install_platform_plugin(&self) -> Result<(), PluginInstallError> {
        Ok(())
    }

    /// Update the Oz platform plugin for this CLI agent, if one exists.
    /// Default reuses the install path because most agents do not have a
    /// platform plugin or need distinct update behavior.
    async fn update_platform_plugin(&self) -> Result<(), PluginInstallError> {
        self.install_platform_plugin().await
    }
}

/// Returns a plugin manager for the given CLI agent, or `None` if the agent
/// doesn't have Warp notification plugin support.
pub(crate) fn plugin_manager_for(agent: CLIAgent) -> Option<Box<dyn CliAgentPluginManager>> {
    plugin_manager_for_with_shell(agent, None, None, None)
}
/// Returns a plugin manager for the given CLI agent, or `None` if the agent
/// doesn't have Warp notification plugin support.
///
/// When a shell path and type are provided, plugin commands run through that shell.
/// When `path_env_var` is provided, it is set as the PATH for plugin commands
/// (needed for nvm-installed tools that are only on PATH in interactive shells).
pub(crate) fn plugin_manager_for_with_shell(
    agent: CLIAgent,
    shell_path: Option<PathBuf>,
    shell_type: Option<ShellType>,
    path_env_var: Option<String>,
) -> Option<Box<dyn CliAgentPluginManager>> {
    match agent {
        CLIAgent::Claude => Some(Box::new(ClaudeCodePluginManager::new(
            shell_path,
            shell_type,
            path_env_var,
        ))),
        CLIAgent::OpenCode
        | CLIAgent::Codex
        | CLIAgent::Gemini
        | CLIAgent::Amp
        | CLIAgent::Droid
        | CLIAgent::Copilot
        | CLIAgent::Pi
        | CLIAgent::OhMyPi
        | CLIAgent::Auggie
        | CLIAgent::CursorCli
        | CLIAgent::Hermes
        | CLIAgent::Goose
        | CLIAgent::Vibe
        | CLIAgent::Antigravity
        | CLIAgent::WarpTui
        | CLIAgent::Unknown => None,
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
