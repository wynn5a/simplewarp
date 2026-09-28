use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use tempfile::NamedTempFile;
use warp_cli::agent::Harness;
use warpui::{ModelHandle, ModelSpawner};

use super::terminal::{CommandHandle, TerminalDriver};
use super::{AgentDriver, AgentDriverError};
use crate::ai::ambient_agents::task::HarnessModelConfig;
use crate::ai::mcp::JSONMCPServer;
use crate::terminal::CLIAgent;
use crate::util::path::resolve_executable;

pub(crate) mod claude_code;
pub(crate) mod claude_transcript;
mod codex;
mod gemini;
mod json_utils;
mod skill_dirs_publish;
pub(crate) use claude_code::ClaudeHarness;
use codex::CodexHarness;
use gemini::GeminiHarness;

/// Trait for third-party agent harnesses that execute prompts via their own CLIs.
///
/// Each new external harness (e.g. Claude, Codex) implements this to be used with cloud agents.
#[async_trait]
pub(crate) trait ThirdPartyHarness: Send + Sync {
    /// Returns the [`Harness`] variant this implementation corresponds to.
    fn harness(&self) -> Harness;

    /// Returns the CLIAgent type associated with this harness.
    fn cli_agent(&self) -> CLIAgent;

    /// URL to install instructions for this harness's CLI, surfaced in the
    /// default [`validate`] impl when the CLI is not on `PATH`.
    fn install_docs_url(&self) -> Option<&'static str> {
        None
    }

    /// Validate that the harness is ready to run. Default impl checks that the
    /// CLI is installed on `PATH`; override for additional checks.
    fn validate(&self) -> Result<(), AgentDriverError> {
        validate_cli_installed(self.cli_agent().command_prefix(), self.install_docs_url())
    }

    /// Shell command to verify authentication credentials are valid.
    /// Exit code 0 = pass; non-zero = fail.
    fn auth_check_command(&self) -> Option<String> {
        None
    }

    /// Substrings to scan for in the running harness block's output. A hit
    /// indicates the harness can't make a successful API request (e.g.
    /// invalid key, no billing, quota exhausted). The driver matches
    /// case-insensitively against the block's plaintext via the same DFA
    /// machinery used by the find feature.
    fn runtime_error_patterns(&self) -> &'static [&'static str] {
        &[]
    }

    /// Build a runner for executing this harness with the given prompt.
    ///
    /// Responsible for all harness-specific setup: writing config files (auth,
    /// trust, system prompt, MCP, etc.) and constructing the runner that will
    /// execute the CLI command.
    ///
    /// `resolved_env_vars` contains the env vars resolved for the terminal
    /// session (worker env, task vars, harness model vars).
    #[allow(clippy::too_many_arguments)]
    fn build_runner(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        resumption_prompt: Option<&str>,
        context: Option<&str>,
        working_dir: &Path,
        terminal_driver: ModelHandle<TerminalDriver>,
        resolved_env_vars: &HashMap<OsString, OsString>,
        resolved_mcp_servers: &HashMap<String, JSONMCPServer>,
        third_party_harness_model_config: Option<&HarnessModelConfig>,
    ) -> Result<Box<dyn HarnessRunner>, AgentDriverError>;
}

/// Harness type for driver dispatch.
pub(crate) enum HarnessKind {
    Oz,
    /// Third-party CLI-backed harness (e.g. Claude, Gemini).
    ThirdParty(Box<dyn ThirdPartyHarness>),
    /// Harnesses that exist in the shared CLI enum but are not supported by the
    /// standalone agent driver.
    Unsupported(Harness),
}

impl HarnessKind {
    /// Corresponding [`Harness`] enum value.
    pub(crate) fn harness(&self) -> Harness {
        match self {
            HarnessKind::Oz => Harness::Oz,
            HarnessKind::ThirdParty(h) => h.harness(),
            HarnessKind::Unsupported(harness) => *harness,
        }
    }
}

impl fmt::Debug for HarnessKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Use the `Display` method on the [`Harness`] enum.
        write!(f, "{}", self.harness())
    }
}

/// Build a [`HarnessKind`] for the given [`Harness`].
///
/// We shouldn't ever get a `--harness unknown` here because clap should handle
/// it.
pub(crate) fn harness_kind(harness: Harness) -> Result<HarnessKind, AgentDriverError> {
    match harness {
        Harness::Oz => Ok(HarnessKind::Oz),
        Harness::Claude => Ok(HarnessKind::ThirdParty(Box::new(ClaudeHarness))),
        Harness::Codex => Ok(HarnessKind::ThirdParty(Box::new(CodexHarness))),
        Harness::OpenCode => Ok(HarnessKind::Unsupported(Harness::OpenCode)),
        Harness::Gemini => Ok(HarnessKind::ThirdParty(Box::new(GeminiHarness))),
        Harness::Unknown => Err(AgentDriverError::InvalidRuntimeState),
    }
}

/// Check that `cli` is installed and on PATH, returning a `HarnessSetupFailed`
/// error with an optional install-docs link when it isn't.
pub(crate) fn validate_cli_installed(
    cli: &str,
    install_docs_url: Option<&str>,
) -> Result<(), AgentDriverError> {
    if resolve_executable(cli).is_none() {
        let mut reason = format!("'{cli}' CLI not found on your machine.");
        if let Some(url) = install_docs_url {
            reason.push_str(&format!(" Install it first: {url}"));
        }
        return Err(AgentDriverError::HarnessSetupFailed {
            harness: cli.into(),
            reason,
        });
    }
    Ok(())
}

/// Returns environment variables that configure the model for a third-party harness.
/// Returns an empty map for Oz or when no model is specified.
///
/// We use the `ANTHROPIC_MODEL` env var rather than the `--model` CLI flag because
/// the env var is the most reliable mechanism and avoids precedence conflicts with
/// Claude Code's `settings.json`.
pub(crate) fn harness_model_env_vars(
    selected_harness: Harness,
    third_party_harness_model_config: Option<&HarnessModelConfig>,
) -> HashMap<OsString, OsString> {
    let mut env_vars = HashMap::new();
    let Some(model_id) = third_party_harness_model_config
        .map(|config| config.model_id.as_str())
        .filter(|id| !id.is_empty())
    else {
        return env_vars;
    };

    match selected_harness {
        Harness::Claude => {
            env_vars.insert(OsString::from("ANTHROPIC_MODEL"), OsString::from(model_id));
        }
        Harness::Oz | Harness::OpenCode | Harness::Gemini | Harness::Codex | Harness::Unknown => {}
    }

    env_vars
}

/// Stateful per-run representation of an external harness produced
/// by [`ThirdPartyHarness::build_runner`].
///
/// All `HarnessRunner` methods take `&self` as a parameter, but may mutate internal
/// state. There are no `&mut self` methods, as this would require that the `AgentDriver`
/// store the runner in a mutex and lock it across `await` points.
///
/// The driver uses this to manage the lifecycle of a particular third-party harness.
#[async_trait]
pub(crate) trait HarnessRunner: Send + Sync {
    fn harness_name(&self) -> &str;

    /// Start the harness command in the terminal.
    ///
    /// Returns a [`CommandHandle`] that resolves to the exit code.
    async fn start(
        &self,
        foreground: &ModelSpawner<AgentDriver>,
    ) -> Result<CommandHandle, AgentDriverError>;

    /// Gracefully ask the harness to exit.
    async fn exit(&self, foreground: &ModelSpawner<AgentDriver>) -> Result<()>;
}

/// Create a [`NamedTempFile`] with the given prefix and write `content` into it.
///
/// Used by third-party harnesses to stage prompts / system prompts on disk
/// before launching the CLI, avoiding shell-quoting issues with complex input.
pub(super) fn write_temp_file(
    prefix: &str,
    content: &str,
    suffix: &str,
) -> Result<NamedTempFile, AgentDriverError> {
    let mut file = tempfile::Builder::new()
        .prefix(prefix)
        .suffix(suffix)
        .tempfile()
        .map_err(|e| {
            AgentDriverError::ConfigBuildFailed(anyhow::anyhow!(
                "Failed to create temp file '{prefix}': {e}"
            ))
        })?;
    file.write_all(content.as_bytes()).map_err(|e| {
        AgentDriverError::ConfigBuildFailed(anyhow::anyhow!(
            "Failed to write temp file '{prefix}': {e}"
        ))
    })?;
    Ok(file)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
