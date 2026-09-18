use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use tempfile::NamedTempFile;
use warp_cli::agent::Harness;
use warp_cli::{
    OZ_CLI_ENV, OZ_HARNESS_ENV, OZ_PARENT_RUN_ID_ENV, OZ_RUN_ID_ENV, SERVER_ROOT_URL_OVERRIDE_ENV,
    SESSION_SHARING_SERVER_URL_OVERRIDE_ENV, WS_SERVER_URL_OVERRIDE_ENV,
};
use warp_core::channel::ChannelState;
use warpui::{ModelHandle, ModelSpawner};

use super::terminal::{CommandHandle, TerminalDriver};
use super::{
    AgentDriver, AgentDriverError, LEGACY_OZ_PARENT_LISTENER_MANAGED_EXTERNALLY_ENV,
    LEGACY_OZ_PARENT_STATE_ROOT_ENV, OZ_MESSAGE_LISTENER_MANAGED_EXTERNALLY_ENV,
    OZ_MESSAGE_LISTENER_STATE_ROOT_ENV,
};
use crate::ai::ambient_agents::AmbientAgentTaskId;
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
mod telemetry;
pub(crate) use claude_code::ClaudeHarness;
use codex::CodexHarness;
use gemini::GeminiHarness;
pub(crate) use telemetry::ThirdPartyHarnessTelemetryEvent;

/// Trait for third-party agent harnesses that execute prompts via their own CLIs.
///
/// Each new external harness (e.g. Claude, Codex) implements this to be used with cloud agents.
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
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

    /// Whether this harness must verify its Oz platform plugin before launch.
    /// Codex opts into this because its unattended launch command bypasses hook
    /// trust globally, so we should fail setup instead of running without the
    /// Warp-installed orchestration hooks at the required version.
    fn requires_verified_platform_plugin(&self) -> bool {
        false
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

fn insert_non_empty_task_env_var(
    env_vars: &mut HashMap<OsString, OsString>,
    key: &'static str,
    value: String,
) {
    if value.is_empty() {
        return;
    }

    env_vars.insert(OsString::from(key), OsString::from(value));
}

fn insert_task_env_var_aliases(
    env_vars: &mut HashMap<OsString, OsString>,
    keys: &[&'static str],
    value: &str,
) {
    for key in keys {
        env_vars.insert(OsString::from(key), OsString::from(value));
    }
}

fn message_listener_state_root() -> Option<String> {
    [
        OZ_MESSAGE_LISTENER_STATE_ROOT_ENV,
        LEGACY_OZ_PARENT_STATE_ROOT_ENV,
    ]
    .into_iter()
    .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
}

fn task_env_vars_for_harness_name(
    task_id: Option<&AmbientAgentTaskId>,
    parent_run_id: Option<&str>,
    selected_harness: Harness,
) -> HashMap<OsString, OsString> {
    let mut env_vars = HashMap::with_capacity(7);

    if let Some(id) = task_id {
        env_vars.insert(
            OsString::from(OZ_RUN_ID_ENV),
            OsString::from(id.to_string()),
        );
    }

    if let Some(parent_run_id) = parent_run_id.filter(|id| !id.is_empty()) {
        env_vars.insert(
            OsString::from(OZ_PARENT_RUN_ID_ENV),
            OsString::from(parent_run_id),
        );
    }

    env_vars.insert(
        OsString::from(OZ_CLI_ENV),
        OsString::from(
            std::env::current_exe()
                .unwrap_or_else(|_| ChannelState::channel().cli_command_name().into()),
        ),
    );
    // `OZ_HARNESS` is only consumed by child orchestration telemetry when the child
    // CLI emits `run message *` events.
    env_vars.insert(
        OsString::from(OZ_HARNESS_ENV),
        OsString::from(selected_harness.to_string()),
    );
    if selected_harness == Harness::Claude && task_id.is_some() {
        insert_task_env_var_aliases(
            &mut env_vars,
            &[
                OZ_MESSAGE_LISTENER_MANAGED_EXTERNALLY_ENV,
                LEGACY_OZ_PARENT_LISTENER_MANAGED_EXTERNALLY_ENV,
            ],
            "1",
        );
        if let Some(state_root) = message_listener_state_root() {
            insert_task_env_var_aliases(
                &mut env_vars,
                &[
                    OZ_MESSAGE_LISTENER_STATE_ROOT_ENV,
                    LEGACY_OZ_PARENT_STATE_ROOT_ENV,
                ],
                &state_root,
            );
        }
    }
    // Server URL overrides are disabled on release channels, so there's no
    // override to propagate to child processes there.
    if ChannelState::channel().allows_server_url_overrides() {
        insert_non_empty_task_env_var(
            &mut env_vars,
            SERVER_ROOT_URL_OVERRIDE_ENV,
            ChannelState::server_root_url().into_owned(),
        );
        insert_non_empty_task_env_var(
            &mut env_vars,
            WS_SERVER_URL_OVERRIDE_ENV,
            ChannelState::ws_server_url().into_owned(),
        );
        if let Some(url) = ChannelState::session_sharing_server_url()
            .map(Cow::into_owned)
            .filter(|url| !url.is_empty())
        {
            env_vars.insert(
                OsString::from(SESSION_SHARING_SERVER_URL_OVERRIDE_ENV),
                OsString::from(url),
            );
        }
    }

    env_vars
}

pub(crate) fn remove_claude_externally_managed_listener_env_vars(
    env_vars: &mut HashMap<OsString, OsString>,
) {
    for env_name in [
        OZ_MESSAGE_LISTENER_MANAGED_EXTERNALLY_ENV,
        LEGACY_OZ_PARENT_LISTENER_MANAGED_EXTERNALLY_ENV,
    ] {
        env_vars.remove(OsStr::new(env_name));
    }
}

pub(crate) fn task_env_vars(
    task_id: Option<&AmbientAgentTaskId>,
    parent_run_id: Option<&str>,
    selected_harness: Harness,
) -> HashMap<OsString, OsString> {
    task_env_vars_for_harness_name(task_id, parent_run_id, selected_harness)
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
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
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
