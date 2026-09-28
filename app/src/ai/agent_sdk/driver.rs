use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::future::Future;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ai::agent::AgentTaskState;
use ai::skills::{parse_skills_dirs_env, read_skills_for_skills_dirs, resolve_skills_dirs};
use anyhow::Context as _;
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{self, Either};
use oneshot::{Canceled, Receiver};
use tracing::Instrument as _;
use uuid::Uuid;
use warp_cli::agent::{Harness, OutputFormat};
use warp_cli::mcp::MCPSpec;
use warp_core::features::FeatureFlag;
use warp_errors::{ErrorExt, register_error, report_error, report_if_error};
use warpui::r#async::{FutureExt, TimeoutError};
use warpui::{Entity, ModelContext, ModelHandle, ModelSpawner, SingletonEntity};

use crate::ai::agent::{
    AIAgentExchange, AIAgentInput, AIAgentOutput, AIAgentOutputStatus, CancellationReason,
    FinishedAIAgentOutput, RenderableAIError, TransientNetworkErrorKind,
};
use crate::ai::agent_sdk::driver::harness::{
    HarnessKind, HarnessRunner, ThirdPartyHarness, harness_model_env_vars, oz_cli_env_var,
};
use crate::ai::agent_sdk::setup_observability::{SetupClientEventReporter, SetupStep};
use crate::ai::ambient_agents::task::HarnessModelConfig;
use crate::ai::ambient_agents::{
    AmbientConversationStatus, conversation_output_status_from_conversation,
};
use crate::ai::blocklist::agent_view::AgentViewEntryOrigin;
use crate::ai::blocklist::{
    BlocklistAIHistoryEvent, BlocklistAIHistoryModel, BlocklistAIPermissions,
};
use crate::ai::execution_profiles::ProfileLookupError;
use crate::ai::execution_profiles::profiles::AIExecutionProfilesModel;
use crate::ai::llms::{LLMId, LLMPreferences};
use crate::ai::mcp::parsing::{ParsedTemplatableMCPServerResult, normalize_mcp_json, resolve_json};
use crate::ai::mcp::templatable_manager::TemplatableMCPServerManagerEvent;
use crate::ai::mcp::{
    JSONMCPServer, MCPServerState, TemplatableMCPServerInstallation, TemplatableMCPServerManager,
};
use crate::ai::skills::SkillManager;
use crate::terminal::cli_agent_sessions::plugin_manager::{
    CliAgentPluginManager, plugin_manager_for,
};
use crate::terminal::cli_agent_sessions::{
    CLIAgentSessionStatus, CLIAgentSessionsModel, CLIAgentSessionsModelEvent,
};
use crate::terminal::model::BlockId;

mod error_classification;
pub(crate) mod harness;
mod harness_output_monitor;
pub(super) mod output;
pub(crate) mod terminal;

use terminal::TerminalDriverEvent;

const MCP_SERVER_STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
/// Timeout for individual harness auth preflight commands.
const PREFLIGHT_CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum time to wait for an automatic error resume before propagating the error.
/// If no follow-up status arrives within this window, the driver terminates with the
/// original error so the CLI does not hang indefinitely.
const AUTO_RESUME_TIMEOUT: Duration = Duration::from_secs(120);
/// IdleTimeoutSender is wrapper around a sender that signals when a run is done after
/// an idle timeout. Used for both Oz runs and third-party harnesses.
///
/// We use a generation-based approach to cancel timers instead of storing timer handles:
///
/// - `tx_cell` holds the completion sender; taking it ensures we only complete once.
/// - `timer_generation` starts at 0 and is incremented each time we want to cancel
///   existing timers and potentially start a new one. When a timer fires, it checks
///   if its generation still matches the current generation. If not, the timer was
///   "cancelled" by a newer timer and should not complete the conversation.
///
/// This approach avoids the complexity of storing and cancelling timer handles,
/// while allowing multiple events to safely race without double-completion.
struct IdleTimeoutSender<T: Send + 'static> {
    tx_cell: Arc<Mutex<Option<oneshot::Sender<T>>>>,
    generation: Arc<AtomicUsize>,
}

// Hand-written so cloning does not require `T: Clone`. Every field is a shared handle, so
// clones drive the same completion.
impl<T: Send + 'static> Clone for IdleTimeoutSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx_cell: Arc::clone(&self.tx_cell),
            generation: Arc::clone(&self.generation),
        }
    }
}

impl<T: Send + 'static> IdleTimeoutSender<T> {
    fn new(tx: oneshot::Sender<T>) -> Self {
        Self {
            tx_cell: Arc::new(Mutex::new(Some(tx))),
            generation: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// End the run by sending `value` immediately.
    fn end_run_now(&self, value: T) {
        if let Ok(mut guard) = self.tx_cell.lock()
            && let Some(sender) = guard.take()
        {
            let _ = sender.send(value);
        }
    }

    /// End the run after `timeout` by sending `value`, unless cancelled before then.
    fn end_run_after(&self, timeout: Duration, value: T) {
        // Increment the generation counter to invalidate any existing timers,
        // then capture the new generation for our timer to check against.
        let current_gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let tx_cell = Arc::clone(&self.tx_cell);
        let generation = Arc::clone(&self.generation);

        // Spawn a background thread that will complete the oneshot after the idle timeout,
        // unless a follow-up query resets the timer (by bumping the generation counter).
        thread::spawn(move || {
            thread::sleep(timeout);

            // Check if our timer generation is still current. If not, a follow-up
            // query or other activity has "cancelled" this timer by bumping the generation.
            if generation.load(Ordering::SeqCst) != current_gen {
                return;
            }
            if let Ok(mut guard) = tx_cell.lock()
                && let Some(sender) = guard.take()
            {
                // Send the value after the idle timeout expires.
                let _ = sender.send(value);
            }
        });
    }

    /// Cancel any pending idle timers.
    fn cancel_idle_timeout(&self) {
        if self.generation.load(Ordering::SeqCst) > 0 {
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// End the run with `value`, deferring by `idle_timeout` when set and completing immediately
    /// when it is `None`.
    fn complete_with_optional_idle(&self, idle_timeout: Option<Duration>, value: T) {
        if let Some(idle_timeout) = idle_timeout {
            self.end_run_after(idle_timeout, value);
        } else {
            self.end_run_now(value);
        }
    }
}

/// How long the driver should stay alive after the conversation reaches `status`. `None` exits
/// immediately. A terminal error always exits immediately.
fn idle_window_for_terminal_status(
    status: &SDKConversationOutputStatus,
    idle_on_complete: Option<Duration>,
) -> Option<Duration> {
    match status {
        SDKConversationOutputStatus::Success
        | SDKConversationOutputStatus::Blocked { .. }
        | SDKConversationOutputStatus::Cancelled { .. } => idle_on_complete,
        SDKConversationOutputStatus::Error { .. } => None,
    }
}

/// [`idle_window_for_terminal_status`] for a third-party CLI harness session.
fn idle_window_for_cli_session_status(
    status: &CLIAgentSessionStatus,
    idle_on_complete: Option<Duration>,
) -> Option<Duration> {
    match status {
        CLIAgentSessionStatus::Success | CLIAgentSessionStatus::Blocked { .. } => idle_on_complete,
        CLIAgentSessionStatus::Failed { .. } | CLIAgentSessionStatus::InProgress => None,
    }
}

/// Low-cardinality `outcome=` label for the ambient agent idle lifecycle logs.
fn terminal_status_log_outcome(status: &SDKConversationOutputStatus) -> &'static str {
    match status {
        SDKConversationOutputStatus::Success
        | SDKConversationOutputStatus::Blocked { .. }
        | SDKConversationOutputStatus::Cancelled { .. } => "non_error_completion",
        SDKConversationOutputStatus::Error { .. } => "error",
    }
}

/// [`terminal_status_log_outcome`] for a third-party CLI harness session.
fn cli_session_status_log_outcome(status: &CLIAgentSessionStatus) -> &'static str {
    match status {
        CLIAgentSessionStatus::Success | CLIAgentSessionStatus::Blocked { .. } => {
            "non_error_completion"
        }
        CLIAgentSessionStatus::Failed { .. } => "error",
        CLIAgentSessionStatus::InProgress => "in_progress",
    }
}

/// Options for initializing the agent driver.
pub struct AgentDriverOptions {
    /// Initial working directory for the agent's terminal session.
    pub working_dir: PathBuf,
    /// How long to keep the session alive after the agent run completes, if at all.
    pub idle_on_complete: Option<Duration>,
    /// Selected execution harness for this run.
    pub selected_harness: Harness,
    /// Model config for the selected harness. Only used for non-Oz harnesses.
    pub third_party_harness_model_config: Option<HarnessModelConfig>,
    /// Fail the run when MCP servers fail to start, instead of continuing
    /// without the unavailable servers.
    pub strict_mcp_startup: bool,
    /// MCP server startup timeout override.
    pub mcp_startup_timeout: Option<Duration>,
}

/// `AgentDriver` is a model for driving an ambient Warp agent to completion.
///
/// Its primary responsibility is to configure a headless terminal pane and execute an AI query within it.
pub struct AgentDriver {
    terminal_driver: ModelHandle<terminal::TerminalDriver>,
    working_dir: PathBuf,

    /// Env vars passed to the terminal session, including cloud provider
    /// vars, task vars, and sandbox flags. Passed to `build_runner` so
    /// harnesses can look up resolved values without re-deriving precedence.
    resolved_env_vars: Arc<HashMap<OsString, OsString>>,

    output_format: OutputFormat,

    /// Harness adapter for the running agent. This is only set if:
    /// - The harness has started successfully.
    /// - We're using a third-party harness.
    /// In the future, we _may_ use the harness abstraction for the Oz agent as well.
    harness: Option<Arc<dyn HarnessRunner>>,

    // Optional idle timeout after completion. If set, the process will stay alive for follow-ups
    // and exit after this period of inactivity.
    idle_on_complete: Option<Duration>,

    third_party_harness_model_config: Option<HarnessModelConfig>,

    /// Whether MCP server startup failures are fatal for the run.
    strict_mcp_startup: bool,
    /// How long to wait for MCP servers to start before degrading (or failing,
    /// in strict mode).
    mcp_startup_timeout: Duration,
}

#[derive(Clone)]
pub(crate) enum SDKConversationOutputStatus {
    Success,
    Error { error: RenderableAIError },
    Cancelled { reason: CancellationReason },
    Blocked { blocked_action: String },
}

impl SDKConversationOutputStatus {
    pub fn into_result(self) -> Result<(), AgentDriverError> {
        match self {
            SDKConversationOutputStatus::Success => Ok(()),
            SDKConversationOutputStatus::Error { error } => {
                Err(AgentDriverError::ConversationError { error })
            }
            // NOTE: this doesn't happen in the SDK (yet) because CTRL+C kills the whole program.
            SDKConversationOutputStatus::Cancelled { reason } => {
                Err(AgentDriverError::ConversationCancelled { reason })
            }
            SDKConversationOutputStatus::Blocked { blocked_action } => {
                Err(AgentDriverError::ConversationBlocked { blocked_action })
            }
        }
    }
}

/// Task configuration for running an agent.
#[derive(Debug)]
pub struct Task {
    /// The prompt for the agent.
    pub prompt: String,
    pub model: Option<LLMId>,
    /// Local profile to run as, by ID or unique name. If None, use the CLI default profile.
    pub profile: Option<String>,
    /// MCP server specifications to start prior to execution.
    pub mcp_specs: Vec<MCPSpec>,
    /// Which harness to use for executing the agent run.
    pub harness: HarnessKind,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentDriverError {
    #[error("Terminal session is not available.")]
    TerminalUnavailable,
    #[error("Invalid runtime state - please file a bug report.")]
    InvalidRuntimeState,
    #[error("Requested MCP server not found: {0}")]
    MCPServerNotFound(uuid::Uuid),
    #[error("Failed to resolve managed MCP server {uid}: {message}")]
    ManagedMcpResolutionFailed { uid: Uuid, message: String },
    #[error("Failed to start MCP servers: {}", .details.join("; "))]
    MCPStartupFailed {
        /// One line per unavailable server (e.g. "'datadog' failed to start:
        /// connection refused").
        details: Vec<String>,
    },
    #[error("Failed to parse MCP server JSON: {0}")]
    MCPJsonParseError(String),
    #[error("MCP server configuration is missing required variables")]
    MCPMissingVariables,
    #[error(transparent)]
    ProfileError(#[from] ProfileLookupError),
    #[error("Saved prompt not found for id {0}")]
    AIWorkflowNotFound(String),
    #[error("Terminal bootstrap failed")]
    BootstrapFailed {
        #[source]
        error: terminal::BootstrapError,
    },
    #[error("Could not resolve working directory {}", path.display())]
    InvalidWorkingDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{error}")]
    ConversationError { error: RenderableAIError },
    #[error("Conversation was canceled: {reason}")]
    ConversationCancelled { reason: CancellationReason },
    #[error("The agent got stuck waiting for user confirmation on the action: {blocked_action}")]
    ConversationBlocked { blocked_action: String },
    /// The shell process exited while an environment setup command was
    /// running (e.g. the command ran `exit`), so the run cannot continue.
    /// `command` is the (secret-redacted) command that was in flight (or
    /// most recently submitted) when the shell died.
    #[error(
        "The shell exited during setup command `{command}`, so the run could not continue. \
         Check the setup commands for this environment."
    )]
    SetupCommandExitedShell { command: String },
    #[error("{0}")]
    SkillResolutionFailed(String),
    #[error("Failed to build agent configuration")]
    ConfigBuildFailed(#[source] anyhow::Error),
    #[error("Harness command exited with code {exit_code}")]
    HarnessCommandFailed { exit_code: i32 },
    #[error("Harness '{harness}' setup failed: {reason}")]
    HarnessSetupFailed { harness: String, reason: String },
    #[error("Harness '{harness}' config setup failed")]
    HarnessConfigSetupFailed {
        harness: String,
        #[source]
        error: anyhow::Error,
    },
    #[error("Harness '{harness}' auth preflight failed")]
    HarnessAuthCheckFailed {
        harness: String,
        /// Stderr/stdout captured from the failing command, for logs.
        detail: String,
    },
    #[error("Harness '{harness}' reported a runtime failure matching '{pattern}'")]
    HarnessRuntimeFailureDetected {
        harness: String,
        /// The originating needle from `runtime_error_patterns` that hit.
        pattern: String,
        /// Matching row(s) from the harness block, trimmed and capped.
        excerpt: String,
    },
}

impl ErrorExt for AgentDriverError {
    fn is_actionable(&self) -> bool {
        error_classification::classify_driver_error(self) == AgentTaskState::Error
    }
}
register_error!(AgentDriverError);

#[derive(Debug, Default)]
struct ResolvedMcpSpecs {
    local_uuids: Vec<Uuid>,
    ephemeral_installations: Vec<TemplatableMCPServerInstallation>,
}

impl From<warpui::ModelDropped> for AgentDriverError {
    fn from(_: warpui::ModelDropped) -> Self {
        AgentDriverError::InvalidRuntimeState
    }
}

impl AgentDriver {
    #[tracing::instrument(name = "AgentDriver::new", skip_all, err, fields(
        is_sandbox = tracing::field::Empty,
    ))]
    pub fn new(
        options: AgentDriverOptions,
        ctx: &mut ModelContext<Self>,
    ) -> Result<Self, AgentDriverError> {
        let AgentDriverOptions {
            working_dir,
            idle_on_complete,
            selected_harness,
            third_party_harness_model_config,
            strict_mcp_startup,
            mcp_startup_timeout,
        } = options;

        log::info!("Initializing agent driver: idle_on_complete={idle_on_complete:?}");

        let mut env_vars = HashMap::from([oz_cli_env_var()]);
        env_vars.extend(harness_model_env_vars(
            selected_harness,
            third_party_harness_model_config.as_ref(),
        ));

        // Signal to third-party harnesses (e.g. Claude Code) that we're in a sandbox
        // so they allow root execution with permissive flags.
        if warp_isolation_platform::detect().is_some() {
            env_vars.insert(OsString::from("IS_SANDBOX"), OsString::from("1"));
            tracing::Span::current().record("is_sandbox", true);
        }

        let resolved_env_vars = Arc::new(env_vars);

        let terminal_driver = terminal::TerminalDriver::create(
            terminal::TerminalDriverOptions {
                working_dir: working_dir.clone(),
                env_vars: HashMap::clone(&resolved_env_vars),
            },
            ctx,
        )?;

        // Subscribe to TerminalDriver events for task-specific handling.
        ctx.subscribe_to_model(&terminal_driver, |me, _, event, ctx| {
            me.handle_terminal_driver_event(event, ctx);
        });

        Ok(Self {
            terminal_driver,
            working_dir,
            resolved_env_vars,
            output_format: OutputFormat::default(),
            harness: None,
            idle_on_complete,
            third_party_harness_model_config,
            strict_mcp_startup,
            mcp_startup_timeout: mcp_startup_timeout.unwrap_or(MCP_SERVER_STARTUP_TIMEOUT),
        })
    }

    /// Minimal constructor for unit tests that need a live `AgentDriver` model to call
    /// methods on (e.g. `load_skills_dirs`) without
    /// bootstrapping a full agent run.
    ///
    /// The caller is responsible for creating the `TerminalDriver` handle beforehand
    /// (e.g. via `TerminalDriver::create_from_existing_view`) and for registering all
    /// required singleton models before constructing the driver.
    #[cfg(test)]
    pub(crate) fn new_for_test(
        working_dir: PathBuf,
        terminal_driver: ModelHandle<terminal::TerminalDriver>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        ctx.subscribe_to_model(&terminal_driver, |me, _, event, ctx| {
            me.handle_terminal_driver_event(event, ctx);
        });
        Self {
            terminal_driver,
            working_dir,
            resolved_env_vars: Arc::new(HashMap::new()),
            output_format: OutputFormat::default(),
            harness: None,
            idle_on_complete: None,
            third_party_harness_model_config: None,
            strict_mcp_startup: false,
            mcp_startup_timeout: MCP_SERVER_STARTUP_TIMEOUT,
        }
    }

    pub fn set_output_format(&mut self, output_format: OutputFormat) {
        self.output_format = output_format;
    }

    pub fn run(
        &mut self,
        task: Task,
        ctx: &mut ModelContext<Self>,
    ) -> impl Future<Output = Result<(), AgentDriverError>> + use<> {
        let (tx, rx) = oneshot::channel();
        let foreground = ctx.spawner();

        ctx.spawn(
            async move {
                let result = Self::run_internal(task, foreground).await;
                if tx.send(result).is_err() {
                    report_error!("Caller did not wait for agent driver to finish");
                }
                log::info!(
                    "Ambient agent lifecycle: event=driver_cleanup_started next=terminal_process_exit"
                );
            },
            |_, _, _| {},
        );

        async move {
            let result = match rx.await {
                Ok(result) => result,
                Err(Canceled) => {
                    log::error!("Agent driver exited abruptly");
                    Err(AgentDriverError::InvalidRuntimeState)
                }
            };

            if let Err(err) = &result {
                report_error!(err);
            }

            result
        }
    }

    /// Check that the working directory exists. Since it's user-specified, we don't automatically
    /// create the directory (in case they made a typo).
    fn check_working_dir(&self) -> impl Future<Output = Result<(), AgentDriverError>> + use<> {
        let working_dir = self.working_dir.clone();
        async move {
            match async_fs::metadata(&working_dir).await {
                Ok(metadata) => {
                    if metadata.is_dir() {
                        Ok(())
                    } else {
                        Err(AgentDriverError::InvalidWorkingDirectory {
                            path: working_dir.to_owned(),
                            source: io::ErrorKind::NotADirectory.into(),
                        })
                    }
                }
                Err(err) => Err(AgentDriverError::InvalidWorkingDirectory {
                    path: working_dir.to_owned(),
                    source: err,
                }),
            }
        }
    }

    /// Resolve MCP specs into a map of MCP name to `JSONMCPServer` for use in
    /// third-party harnesses. Each spec is fully resolved (templates rendered)
    /// so harnesses can serialize directly into their native config format.
    async fn resolve_mcp_specs_to_json(
        specs: &[MCPSpec],
        foreground: &ModelSpawner<Self>,
    ) -> Result<HashMap<String, JSONMCPServer>, AgentDriverError> {
        let resolved_specs = Self::resolve_mcp_specs(specs, foreground).await?;

        let local_uuids = resolved_specs.local_uuids;
        let mut installations = foreground
            .spawn(move |_, ctx| -> Result<Vec<_>, AgentDriverError> {
                let manager = TemplatableMCPServerManager::as_ref(ctx);
                local_uuids
                    .iter()
                    .map(|uuid| {
                        manager
                            .get_installed_server(uuid)
                            .cloned()
                            .ok_or(AgentDriverError::MCPServerNotFound(*uuid))
                    })
                    .collect()
            })
            .await??;
        installations.extend(resolved_specs.ephemeral_installations);

        Self::mcp_installations_to_json(installations)
    }

    fn mcp_installations_to_json(
        mut installations: Vec<TemplatableMCPServerInstallation>,
    ) -> Result<HashMap<String, JSONMCPServer>, AgentDriverError> {
        let mut result = HashMap::new();

        for installation in installations.iter_mut() {
            let resolved = resolve_json(installation);
            let servers: HashMap<String, JSONMCPServer> = serde_json::from_str(&resolved)
                .map_err(|e| AgentDriverError::MCPJsonParseError(e.to_string()))?;
            result.extend(servers);
        }

        Ok(result)
    }

    /// Resolve MCP specs into local UUIDs and ephemeral installations. UUIDs
    /// are local-first; only non-local UUIDs call managed MCP GraphQL.
    async fn resolve_mcp_specs(
        specs: &[MCPSpec],
        foreground: &ModelSpawner<Self>,
    ) -> Result<ResolvedMcpSpecs, AgentDriverError> {
        let local_installed_uuids = foreground
            .spawn(|_, ctx| {
                TemplatableMCPServerManager::as_ref(ctx)
                    .get_installed_templatable_servers()
                    .keys()
                    .copied()
                    .collect::<HashSet<_>>()
            })
            .await?;

        Self::resolve_mcp_specs_with_local_uuids(specs, &local_installed_uuids).await
    }

    async fn resolve_mcp_specs_with_local_uuids(
        specs: &[MCPSpec],
        local_installed_uuids: &HashSet<Uuid>,
    ) -> Result<ResolvedMcpSpecs, AgentDriverError> {
        let mut resolved = ResolvedMcpSpecs::default();

        for spec in specs {
            match spec {
                MCPSpec::Uuid(uuid) if local_installed_uuids.contains(uuid) => {
                    resolved.local_uuids.push(*uuid);
                }
                MCPSpec::Uuid(uuid) => {
                    // A uuid that is not installed locally could only be resolved by asking the
                    // server for its managed client config. This build has no server, so the
                    // spec names a server that cannot be reached.
                    return Err(AgentDriverError::ManagedMcpResolutionFailed {
                        uid: *uuid,
                        message: "managed MCP servers are not available in this build".to_string(),
                    });
                }
                MCPSpec::WellKnown(id) => {
                    // Well-known ids (e.g. "linear") were resolved by the server, which owned the
                    // set of recognized ids. Resolution was already best-effort — a disconnected
                    // integration skipped the server rather than failing the run — so with no
                    // server to ask, every one of them skips.
                    log::warn!(
                        "Skipping well-known MCP server '{id}': managed MCP servers are not available in this build"
                    );
                }
                MCPSpec::Json(json_str) => {
                    resolved
                        .ephemeral_installations
                        .extend(Self::installations_from_user_mcp_json(json_str)?);
                }
            }
        }

        Ok(resolved)
    }

    fn installations_from_user_mcp_json(
        json_str: &str,
    ) -> Result<Vec<TemplatableMCPServerInstallation>, AgentDriverError> {
        let normalized_json = normalize_mcp_json(json_str)
            .map_err(|e| AgentDriverError::MCPJsonParseError(e.to_string()))?;
        let parsed_results = ParsedTemplatableMCPServerResult::from_user_json(&normalized_json)
            .map_err(|e| AgentDriverError::MCPJsonParseError(e.to_string()))?;

        parsed_results
            .into_iter()
            .map(|result| {
                result
                    .templatable_mcp_server_installation
                    .ok_or(AgentDriverError::MCPMissingVariables)
            })
            .collect()
    }

    /// Start MCP servers from profile allowlist for the terminal.
    fn start_profile_mcp_servers(
        &self,
        ctx: &mut ModelContext<Self>,
    ) -> impl Future<Output = Result<(), AgentDriverError>> + use<> {
        let terminal_id = self.terminal_driver.as_ref(ctx).terminal_view().id();
        let permissions = BlocklistAIPermissions::as_ref(ctx);
        let profile_allowlist = permissions.get_mcp_allowlist(ctx, Some(terminal_id));

        if !profile_allowlist.is_empty() {
            log::info!(
                "Starting {} MCP servers allowlisted in profile",
                profile_allowlist.len()
            );
        }
        self.start_mcp_servers(&profile_allowlist, ctx)
    }

    fn get_mcp_servers_to_start(
        &self,
        uuids: &[uuid::Uuid],
        ctx: &mut ModelContext<Self>,
    ) -> Result<HashSet<Uuid>, AgentDriverError> {
        let templatable_mcp_manager = TemplatableMCPServerManager::handle(ctx);

        let mut servers_to_start: HashSet<Uuid> = HashSet::new();

        for uuid in uuids.iter() {
            if templatable_mcp_manager
                .as_ref(ctx)
                .is_server_active_or_pending(*uuid)
            {
                log::debug!("MCP server {uuid} is already active or pending; skipping");
                continue;
            } else if templatable_mcp_manager
                .as_ref(ctx)
                .get_installed_server(uuid)
                .is_some()
            {
                servers_to_start.insert(*uuid);
            } else {
                return Err(AgentDriverError::MCPServerNotFound(*uuid));
            }
        }

        Ok(servers_to_start)
    }

    /// Subscribe to MCP server state changes and wait for every server in
    /// `servers` (keyed by installation UUID, valued by display name) to reach
    /// a terminal state (`Running` or `FailedToStart`), up to the configured
    /// startup timeout.
    ///
    /// Returns [`AgentDriverError::MCPStartupFailed`] naming the servers that
    /// failed to start or were still starting at the deadline. Callers decide
    /// whether that is fatal (see strict MCP startup handling in
    /// `run_internal`).
    ///
    /// Must be called before the servers are spawned so no state changes are
    /// missed, and never concurrently with another MCP wait: the driver keeps
    /// at most one subscription to [`TemplatableMCPServerManager`].
    fn wait_for_mcp_servers_started(
        &self,
        servers: HashMap<Uuid, String>,
        ctx: &mut ModelContext<Self>,
    ) -> impl Future<Output = Result<(), AgentDriverError>> + use<> {
        // If no servers to wait for, complete immediately.
        if servers.is_empty() {
            return Either::Right(future::ready(Ok(())));
        }

        // Stall for user-configured timeout, else 20 seconds (configured in [`AgentDriverOptions`]).
        let timeout = self.mcp_startup_timeout;
        let (tx, rx) = oneshot::channel::<()>();
        let mut tx = Some(tx);

        let pending = Arc::new(Mutex::new(servers));
        let failed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let pending_for_subscription = Arc::clone(&pending);
        let failed_for_subscription = Arc::clone(&failed);

        let templatable_mcp_manager = TemplatableMCPServerManager::handle(ctx);

        // Clear any stale subscription left behind by a previous wait that
        // timed out, so it can't tear down this wait's subscription.
        ctx.unsubscribe_from_model(&templatable_mcp_manager);
        ctx.subscribe_to_model(&templatable_mcp_manager, move |_me, manager, event, ctx| {
            let TemplatableMCPServerManagerEvent::StateChanged { uuid, state } = event else {
                return;
            };
            let Ok(mut pending_servers) = pending_for_subscription.lock() else {
                return;
            };
            let Some(name) = pending_servers.get(uuid).cloned() else {
                // If we receive a state change for a server that we're not waiting for, ignore it.
                return;
            };
            match state {
                MCPServerState::Running => {
                    pending_servers.remove(uuid);
                }
                MCPServerState::FailedToStart => {
                    pending_servers.remove(uuid);
                    let error = TemplatableMCPServerManager::as_ref(ctx)
                        .get_server_error_message(*uuid)
                        .map(|message| format!(": {message}"))
                        .unwrap_or_default();
                    let detail = format!("'{name}' failed to start{error}");
                    log::warn!("MCP server {detail}");
                    if let Ok(mut failed_servers) = failed_for_subscription.lock() {
                        failed_servers.push(detail);
                    }
                }
                MCPServerState::NotRunning
                | MCPServerState::Starting
                | MCPServerState::Authenticating
                | MCPServerState::ShuttingDown => return,
            }
            if pending_servers.is_empty() {
                log::info!("All requested MCP servers reached a terminal state");
                if let Some(sender) = tx.take() {
                    let _ = sender.send(());
                }
                ctx.unsubscribe_from_model(&manager);
            }
        });

        let spawner = ctx.spawner();
        Either::Left(async move {
            let wait_result = rx.with_timeout(timeout).await;

            let mut still_starting: Vec<String> = Vec::new();
            match wait_result {
                Ok(Ok(())) => {}
                Ok(Err(Canceled)) => {
                    log::error!("Subscription dropped before MCP servers started");
                    return Err(AgentDriverError::InvalidRuntimeState);
                }
                Err(TimeoutError) => {
                    still_starting = pending
                        .lock()
                        .map(|pending_servers| pending_servers.values().cloned().collect())
                        .unwrap_or_default();
                    still_starting.sort();
                    // The subscription is now stale; remove it so it can't
                    // tear down a later wait's subscription. This completes
                    // before this future resolves, so it cannot race with a
                    // subsequent wait.
                    let _ = spawner
                        .spawn(|_, ctx| {
                            let manager = TemplatableMCPServerManager::handle(ctx);
                            ctx.unsubscribe_from_model(&manager);
                        })
                        .await;
                }
            }

            let mut details = failed
                .lock()
                .map(|failed_servers| failed_servers.clone())
                .unwrap_or_default();
            details.sort();
            details.extend(
                still_starting
                    .iter()
                    .map(|name| format!("'{name}' did not start within {}s", timeout.as_secs())),
            );

            if details.is_empty() {
                Ok(())
            } else {
                Err(AgentDriverError::MCPStartupFailed { details })
            }
        })
    }

    /// Fold an MCP startup result into `degraded`, propagating any error that
    /// is fatal regardless of the strict MCP startup setting.
    fn collect_mcp_degradation(
        result: Result<(), AgentDriverError>,
        degraded: &mut Vec<String>,
    ) -> Result<(), AgentDriverError> {
        match result {
            Ok(()) => Ok(()),
            Err(AgentDriverError::MCPStartupFailed { details }) => {
                degraded.extend(details);
                Ok(())
            }
            Err(other) => Err(other),
        }
    }

    /// Apply strict MCP startup handling to a recorded startup result.
    ///
    /// Degraded startup (`MCPStartupFailed`) is fatal only in strict mode.
    /// Otherwise the run continues without the unavailable servers and the
    /// degradation is logged.
    fn handle_mcp_startup_result(
        result: Result<(), AgentDriverError>,
        strict: bool,
    ) -> Result<(), AgentDriverError> {
        let Err(error) = result else {
            return Ok(());
        };
        let AgentDriverError::MCPStartupFailed { details } = &error else {
            return Err(error);
        };
        let details = details.join("; ");

        if strict {
            return Err(error);
        }

        log::warn!(
            "MCP startup degraded ({details}); continuing without the unavailable MCP servers"
        );
        Ok(())
    }

    fn spawn_inactive_servers(
        &self,
        servers_to_start: HashSet<Uuid>,
        ctx: &mut ModelContext<Self>,
    ) {
        let templatable_mcp_manager = TemplatableMCPServerManager::handle(ctx);
        templatable_mcp_manager.update(ctx, |manager, ctx| {
            for uuid in servers_to_start {
                manager.spawn_server(uuid, ctx);
            }
        });
    }

    fn start_mcp_servers(
        &self,
        uuids: &[uuid::Uuid],
        ctx: &mut ModelContext<Self>,
    ) -> impl Future<Output = Result<(), AgentDriverError>> + use<> {
        let servers_to_start = match self.get_mcp_servers_to_start(uuids, ctx) {
            Ok(val) => val,
            Err(e) => {
                return Either::Right(future::ready(Err(e)));
            }
        };

        // If we don't need to start any servers, complete immediately.
        if servers_to_start.is_empty() {
            return Either::Right(future::ready(Ok(())));
        }

        log::info!("Starting {} MCP servers...", servers_to_start.len());

        let named_servers: HashMap<Uuid, String> = {
            let manager = TemplatableMCPServerManager::as_ref(ctx);
            servers_to_start
                .iter()
                .map(|uuid| {
                    let name = manager
                        .get_installed_server(uuid)
                        .map(|installation| installation.templatable_mcp_server().name.clone())
                        .unwrap_or_else(|| uuid.to_string());
                    (*uuid, name)
                })
                .collect()
        };
        let wait = self.wait_for_mcp_servers_started(named_servers, ctx);

        self.spawn_inactive_servers(servers_to_start, ctx);

        Either::Left(wait)
    }

    /// Start ephemeral MCP servers from inline JSON specifications.
    /// These servers are not persisted and exist only for the duration of the agent run.
    fn start_ephemeral_mcp_servers(
        &self,
        installations: Vec<TemplatableMCPServerInstallation>,
        ctx: &mut ModelContext<Self>,
    ) -> impl Future<Output = Result<(), AgentDriverError>> + use<> {
        if installations.is_empty() {
            return Either::Right(future::ready(Ok(())));
        }

        log::info!("Starting {} ephemeral MCP servers...", installations.len());

        let named_servers: HashMap<Uuid, String> = installations
            .iter()
            .map(|installation| {
                (
                    installation.uuid(),
                    installation.templatable_mcp_server().name.clone(),
                )
            })
            .collect();
        let wait = self.wait_for_mcp_servers_started(named_servers, ctx);

        // Spawn the ephemeral servers.
        let templatable_mcp_manager = TemplatableMCPServerManager::handle(ctx);
        templatable_mcp_manager.update(ctx, move |manager, ctx| {
            for installation in installations {
                manager.spawn_cli_ephemeral_server(installation, ctx);
            }
        });

        Either::Left(wait)
    }

    /// Load skills from the `WARP_SKILL_DIRS` environment variable as personal (home) tier skills.
    ///
    /// `WARP_SKILL_DIRS` is a comma-separated list of paths; each entry is itself a skills directory
    /// whose **direct children** are expected to be skill folders containing `SKILL.md`. Relative
    /// entries are resolved against the driver's working directory, not the process's current
    /// working directory. Skills loaded this way behave identically to `~/.agents/skills` personal
    /// skills—always in scope, regardless of the current working directory.
    ///
    /// Invalid, missing, or unreadable entries are skipped with a warning; an unset or empty
    /// variable is a no-op.
    async fn load_skills_dirs(foreground: &ModelSpawner<Self>) {
        let dirs = parse_skills_dirs_env();
        if dirs.is_empty() {
            return;
        }
        log::info!(
            "WARP_SKILL_DIRS: loading skills from {} directories",
            dirs.len()
        );
        let load_result = foreground
            .spawn(move |me, ctx| {
                let dirs = resolve_skills_dirs(&me.working_dir, dirs);
                let skills = read_skills_for_skills_dirs(&dirs);
                if skills.is_empty() {
                    log::info!("WARP_SKILL_DIRS: no skills found");
                } else {
                    log::info!("WARP_SKILL_DIRS: loaded {} skill(s)", skills.len());
                }
                SkillManager::handle(ctx).update(ctx, |manager, _| {
                    manager.add_skills_dirs_skills(skills);
                });
            })
            .await;
        if let Err(err) = load_result {
            log::warn!("Failed to load WARP_SKILL_DIRS skills: {err}");
        }
    }

    /// Runs the agent to completion.
    /// Driving the agent mostly requires main-thread UI framework updates, but using `async` and
    /// a `ModelSpawner` lets us express the high-level process linearly rather than in a
    /// series of callbacks and state machine updates.
    #[tracing::instrument(name = "AgentDriver::run_internal", skip_all, err)]
    async fn run_internal(
        task: Task,
        foreground: ModelSpawner<Self>,
    ) -> Result<(), AgentDriverError> {
        log::debug!("Running agent driver");

        let setup_span = tracing::info_span!("agent_run_setup");
        let setup_events = async {
            let setup_events = foreground
                .spawn(|_me, _ctx| SetupClientEventReporter::new())
                .await?;

            foreground
                .spawn(|me, _| me.check_working_dir())
                .await?
                .await?;

            // IMPORTANT: Wait for the terminal session to bootstrap before starting MCP servers.
            // Some of the initializations are necessary for the MCP servers to start correctly.
            //
            // Why: MCP server startup can happen before we actually execute the agent prompt. For
            // `TransportType::CLIServer` MCPs we currently depend on `AISettings.mcp_execution_path`,
            // which is populated as part of terminal bootstrap. Waiting for the session bootstrap
            // here avoids a subtle race where MCP spawn runs with an unset PATH and then the driver
            // only fails via a timeout.
            setup_events
                .record_result(SetupStep::TerminalBootstrap, async {
                    foreground
                        .spawn(|me, ctx| {
                            me.terminal_driver
                                .update(ctx, |driver, _| driver.wait_for_session_bootstrapped())
                        })
                        .await?
                        .await
                        .map_err(|error| AgentDriverError::BootstrapFailed { error })
                })
                .await?;

            // For the Oz harness only: set up MCP servers, model overrides, and profile information.
            if matches!(&task.harness, HarnessKind::Oz) {
                let mcp_specs = task.mcp_specs.clone();

                let mcp_startup_result = setup_events
                    .record_result(SetupStep::McpServerStartup, async {
                        let resolved_mcp_specs =
                            Self::resolve_mcp_specs(&mcp_specs, &foreground).await?;
                        let existing_uuids = resolved_mcp_specs.local_uuids;
                        let ephemeral_installations = resolved_mcp_specs.ephemeral_installations;

                        log::info!(
                            "Starting {} existing and {} ephemeral MCP servers",
                            existing_uuids.len(),
                            ephemeral_installations.len()
                        );

                        // Run both startup phases even when one degrades, collecting
                        // degradation details so non-strict runs can continue with
                        // whichever servers did start.
                        let mut degraded = Vec::new();
                        if !existing_uuids.is_empty() {
                            let result = foreground
                                .spawn(move |me, ctx| me.start_mcp_servers(&existing_uuids, ctx))
                                .await?
                                .await;
                            Self::collect_mcp_degradation(result, &mut degraded)?;
                        }
                        // Start ephemeral MCP servers from inline JSON specs.
                        if !ephemeral_installations.is_empty() {
                            let result = foreground
                                .spawn(move |me, ctx| {
                                    me.start_ephemeral_mcp_servers(ephemeral_installations, ctx)
                                })
                                .await?
                                .await;
                            Self::collect_mcp_degradation(result, &mut degraded)?;
                        }
                        if degraded.is_empty() {
                            Ok(())
                        } else {
                            Err(AgentDriverError::MCPStartupFailed { details: degraded })
                        }
                    })
                    .await;
                let strict = foreground.spawn(|me, _| me.strict_mcp_startup).await?;
                Self::handle_mcp_startup_result(mcp_startup_result, strict)?;
                let profile = task.profile.clone();
                setup_events
                    .record_result(SetupStep::AgentProfileConfiguration, async {
                        foreground
                            .spawn(move |me, ctx| me.configure_terminal(profile, ctx))
                            .await?
                    })
                    .await?;

                if let Some(model_id) = task.model.clone() {
                    foreground
                        .spawn(move |me, ctx| me.set_base_model_override(model_id, ctx))
                        .await??;
                }

                let profile_mcp_startup_result = setup_events
                    .record_result(SetupStep::ProfileMcpServerStartup, async {
                        foreground
                            .spawn(|me, ctx| me.start_profile_mcp_servers(ctx))
                            .await?
                            .await
                    })
                    .await;
                let strict = foreground.spawn(|me, _| me.strict_mcp_startup).await?;
                Self::handle_mcp_startup_result(profile_mcp_startup_result, strict)?;
            }

            // Skill loading is Oz-only; third-party harnesses have their own skill systems.
            if matches!(&task.harness, HarnessKind::Oz) {
                setup_events
                    .record_value(
                        SetupStep::SkillsDirsLoading,
                        Self::load_skills_dirs(&foreground),
                    )
                    .await;
            }

            Ok::<_, AgentDriverError>(setup_events)
        }
        .instrument(setup_span)
        .await?;

        match task.harness {
            HarnessKind::Oz => {
                let status_rx = foreground
                    .spawn(move |me, ctx| me.execute_run(task.prompt, ctx))
                    .await?;

                let conversation_status = async move {
                    status_rx.await.map_err(|_| {
                        report_error!("Subscription dropped before agent finished");
                        AgentDriverError::InvalidRuntimeState
                    })
                }
                .await?;
                log::info!(
                    "Ambient agent Oz lifecycle: event=run_exit_received idle_on_complete_elapsed_or_not_configured=true next=terminal_teardown_after_flush"
                );

                // Pause before returning to make sure that all conversation events are transmitted before the session is closed.
                // TODO: This is a bit of a bandaid fix, and it would be better if we explicitly waited for the session to end before terminating.
                // The way we could do that is through having the driver wait for all in-flight streams to be finished before terminating
                // and then call stop_sharing_session when they're done. To know when streams are finished, we would need to modify start_ordered_terminal_events_listener
                // to send a message when the streams are finished, flushed, and the websocket is disconnected. For now, we'll just sleep for a second, as this seems
                // to be enough time for the streams to be finished and the events to be flushed.
                warpui::r#async::Timer::after(Duration::from_secs(1)).await;

                conversation_status.into_result()
            }
            HarnessKind::ThirdParty(harness) => {
                let harness_setup_events = setup_events.clone();
                let (harness_exit_rx, runner) = setup_events
                    .record_result(SetupStep::ThirdPartyHarnessPreparation, async {
                        let harness_exit_rx = Self::setup_harness(
                            harness.as_ref(),
                            &foreground,
                            &harness_setup_events,
                        )
                        .await?;
                        let runner = Self::prepare_harness(
                            &task.prompt,
                            &task.mcp_specs,
                            harness.as_ref(),
                            &foreground,
                        )
                        .await?;

                        Self::run_preflight_checks(harness.as_ref(), &foreground).await?;
                        Ok::<_, AgentDriverError>((harness_exit_rx, runner))
                    })
                    .await?;
                let runtime_error_patterns = harness.runtime_error_patterns();

                Self::run_harness(runner, runtime_error_patterns, &foreground, harness_exit_rx)
                    .await
            }
            HarnessKind::Unsupported(harness) => Err(AgentDriverError::HarnessSetupFailed {
                harness: harness.to_string(),
                reason: format!(
                    "The {harness} harness is only supported for local child agent launches."
                ),
            }),
        }
    }

    /// Run the authentication preflight check for a third-party harness.
    ///
    /// Uses `execute_command` so the check appears as a collapsible block in
    /// the terminal, mirroring how environment setup commands
    /// surface.
    async fn run_preflight_checks(
        harness: &dyn ThirdPartyHarness,
        foreground: &ModelSpawner<Self>,
    ) -> Result<(), AgentDriverError> {
        let harness_name = harness.cli_agent().command_prefix().to_owned();

        if let Some(cmd) = harness.auth_check_command() {
            log::info!("Running auth check for {harness_name}: {cmd}");
            Self::run_single_preflight(&cmd, &harness_name, foreground).await?;
        }

        Ok(())
    }

    /// Run a single preflight check command and return an error if it fails.
    async fn run_single_preflight(
        command: &str,
        harness_name: &str,
        foreground: &ModelSpawner<Self>,
    ) -> Result<(), AgentDriverError> {
        let cmd = command.to_owned();
        let start_future = foreground
            .spawn(move |me, ctx| {
                me.terminal_driver
                    .update(ctx, |driver, ctx| driver.execute_command(&cmd, ctx))
            })
            .await??;

        let command_handle = start_future.await?;
        let block_id = command_handle.block_id().clone();

        let exit_code = match command_handle.with_timeout(PREFLIGHT_CHECK_TIMEOUT).await {
            Err(TimeoutError) => {
                log::error!("Preflight auth check timed out for {harness_name}");
                return Err(AgentDriverError::HarnessAuthCheckFailed {
                    harness: harness_name.to_owned(),
                    detail: "command timed out".to_owned(),
                });
            }
            Ok(result) => result?,
        };

        if !exit_code.was_successful() {
            let output_text = Self::fetch_preflight_block_output(&block_id, foreground).await;
            let detail = if output_text.is_empty() {
                format!("exit code {}", exit_code.value())
            } else {
                format!("exit code {}: {}", exit_code.value(), output_text)
            };
            log::error!(
                "Preflight auth check failed for {harness_name} (exit code {})",
                exit_code.value()
            );
            return Err(AgentDriverError::HarnessAuthCheckFailed {
                harness: harness_name.to_owned(),
                detail,
            });
        }

        log::info!("Preflight auth check passed for {harness_name}");
        Ok(())
    }

    async fn fetch_preflight_block_output(
        block_id: &BlockId,
        foreground: &ModelSpawner<Self>,
    ) -> String {
        let block_id = block_id.clone();
        let plaintext = foreground
            .spawn(move |me, ctx| {
                me.terminal_driver
                    .as_ref(ctx)
                    .block_output_plaintext(&block_id, ctx)
            })
            .await;
        match plaintext {
            Ok(Some(text)) => text.trim().to_owned(),
            Ok(None) | Err(_) => String::new(),
        }
    }

    /// Sets up the third-party harness by subscribing to CLI session events and
    /// installing the Warp plugin and platform plugin, if applicable.
    ///
    /// Returns a oneshot receiver that fires when the harness should exit
    /// (either immediately on completion or after the idle-on-complete timeout).
    async fn setup_harness(
        harness: &dyn ThirdPartyHarness,
        foreground: &ModelSpawner<Self>,
        events: &SetupClientEventReporter,
    ) -> Result<oneshot::Receiver<()>, AgentDriverError> {
        let (exit_tx, exit_rx) = oneshot::channel();
        let harness_exit = IdleTimeoutSender::new(exit_tx);

        // Subscribe to CLI agent session events so we can update the task
        // state as the harness emits stop/blocked notifications.
        foreground
            .spawn(move |me, ctx| me.subscribe_to_cli_agent_session_events(harness_exit, ctx))
            .await?;

        // Install plugins before running the harness command.
        Self::setup_harness_plugins(harness, events).await?;

        Ok(exit_rx)
    }

    async fn setup_harness_plugins(
        harness: &dyn ThirdPartyHarness,
        events: &SetupClientEventReporter,
    ) -> Result<(), AgentDriverError> {
        let harness_name = harness.cli_agent().command_prefix();
        let requires_platform_plugin = harness.requires_verified_platform_plugin();
        let Some(manager) = plugin_manager_for(harness.cli_agent()) else {
            if requires_platform_plugin {
                return Err(Self::required_platform_plugin_error(
                    harness_name,
                    "Required platform plugin manager is unavailable",
                ));
            }
            return Ok(());
        };

        Self::setup_notification_plugin(manager.as_ref(), events).await;
        Self::setup_platform_plugin(
            harness_name,
            manager.as_ref(),
            requires_platform_plugin,
            events,
        )
        .await
    }

    async fn setup_notification_plugin(
        manager: &dyn CliAgentPluginManager,
        events: &SetupClientEventReporter,
    ) {
        if !manager.can_auto_install() {
            return;
        }
        if manager.needs_update() {
            if let Err(e) = events
                .record_result(
                    SetupStep::ThirdPartyHarnessPreparationNotificationPluginUpdate,
                    manager.update(),
                )
                .await
            {
                log::warn!("Plugin update failed (continuing): {e}");
            }
        } else if !manager.is_installed()
            && let Err(e) = events
                .record_result(
                    SetupStep::ThirdPartyHarnessPreparationNotificationPluginInstall,
                    manager.install(),
                )
                .await
        {
            log::warn!("Plugin installation failed (continuing): {e}");
        }
    }

    async fn setup_platform_plugin(
        harness_name: &str,
        manager: &dyn CliAgentPluginManager,
        required: bool,
        events: &SetupClientEventReporter,
    ) -> Result<(), AgentDriverError> {
        if manager.platform_plugin_needs_update() {
            if let Err(e) = events
                .record_result(
                    SetupStep::ThirdPartyHarnessPreparationPlatformPluginUpdate,
                    manager.update_platform_plugin(),
                )
                .await
            {
                if required {
                    return Err(Self::required_platform_plugin_error(
                        harness_name,
                        format!("Required platform plugin update failed: {e}"),
                    ));
                }
                log::warn!("Platform plugin update failed (continuing): {e}");
            }
        } else if !manager.is_platform_plugin_installed()
            && let Err(e) = events
                .record_result(
                    SetupStep::ThirdPartyHarnessPreparationPlatformPluginInstall,
                    manager.install_platform_plugin(),
                )
                .await
        {
            if required {
                return Err(Self::required_platform_plugin_error(
                    harness_name,
                    format!("Required platform plugin installation failed: {e}"),
                ));
            }
            log::warn!("Platform plugin installation failed (continuing): {e}");
        }

        if required {
            Self::verify_required_platform_plugin(harness_name, manager)?;
        }
        Ok(())
    }

    fn verify_required_platform_plugin(
        harness_name: &str,
        manager: &dyn CliAgentPluginManager,
    ) -> Result<(), AgentDriverError> {
        if !manager.is_platform_plugin_installed() {
            return Err(Self::required_platform_plugin_error(
                harness_name,
                "Required platform plugin is not installed",
            ));
        }
        if manager.platform_plugin_needs_update() {
            return Err(Self::required_platform_plugin_error(
                harness_name,
                "Required platform plugin is below the minimum supported version",
            ));
        }
        Ok(())
    }

    fn required_platform_plugin_error(
        harness: &str,
        reason: impl Into<String>,
    ) -> AgentDriverError {
        AgentDriverError::HarnessSetupFailed {
            harness: harness.to_owned(),
            reason: reason.into(),
        }
    }

    /// Configure a third-party harness for execution. This will set `self.harness` and
    /// return a handle to the harness runner.
    async fn prepare_harness(
        prompt: &str,
        mcp_specs: &[MCPSpec],
        harness: &dyn ThirdPartyHarness,
        foreground: &ModelSpawner<Self>,
    ) -> Result<Arc<dyn harness::HarnessRunner>, AgentDriverError> {
        let (working_dir, terminal_driver) = foreground
            .spawn(|me, _| {
                if me.harness.is_some() {
                    log::error!(
                        "Attempted to prepare a third-party harness, but one was already configured"
                    );
                    return Err(AgentDriverError::InvalidRuntimeState);
                }

                Ok((me.working_dir.clone(), me.terminal_driver.clone()))
            })
            .await
            .map_err(|_| AgentDriverError::InvalidRuntimeState)
            .flatten()?;

        let (prompt_text, system_prompt, resumption_prompt, server_context): (
            Cow<'_, str>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = (Cow::Borrowed(prompt), None, None, None);

        let third_party_harness_model_config = foreground
            .spawn(|me, _| me.third_party_harness_model_config.clone())
            .await
            .map_err(|_| AgentDriverError::InvalidRuntimeState)?;

        // Resolve MCP specs into harness-native JSON format.
        let mcp_specs = mcp_specs.to_vec();
        let resolved_mcp_servers = Self::resolve_mcp_specs_to_json(&mcp_specs, foreground).await?;
        if !resolved_mcp_servers.is_empty() {
            log::info!(
                "Resolved {} MCP server(s) for third-party harness",
                resolved_mcp_servers.len()
            );
        }

        let resolved_env_vars = foreground
            .spawn(|me, _| Arc::clone(&me.resolved_env_vars))
            .await
            .map_err(|_| AgentDriverError::InvalidRuntimeState)?;

        let runner: Arc<dyn HarnessRunner> = harness
            .build_runner(
                prompt_text.as_ref(),
                system_prompt.as_deref(),
                resumption_prompt.as_deref(),
                server_context.as_deref(),
                &working_dir,
                terminal_driver,
                &resolved_env_vars,
                &resolved_mcp_servers,
                third_party_harness_model_config.as_ref(),
            )?
            .into();

        let stored_runner = runner.clone();
        foreground
            .spawn(move |me, _| me.harness = Some(stored_runner))
            .await?;

        Ok(runner)
    }

    /// Execute a configured external harness in the terminal.
    ///
    /// The `harness_exit_rx` oneshot fires when the subscription determines it's
    /// time to exit (either immediately on completion or after the idle timeout).
    ///
    /// While the harness runs, a background scanner watches its block for
    /// known runtime failure substrings (e.g. invalid API key, exhausted
    /// credits). If one is detected we send `/exit` to the harness and
    /// synthesize a [`AgentDriverError::HarnessRuntimeFailureDetected`]
    /// failure, which `report_driver_error` reports to the server with the
    /// same `AuthenticationRequired` error code used by the auth preflight.
    async fn run_harness(
        runner: Arc<dyn harness::HarnessRunner>,
        runtime_error_patterns: &'static [&'static str],
        foreground: &ModelSpawner<Self>,
        harness_exit_rx: oneshot::Receiver<()>,
    ) -> Result<(), AgentDriverError> {
        let harness_name = runner.harness_name().to_owned();

        // Start the third-party harness.
        let command_handle = runner.start(foreground).await?;
        let block_id = command_handle.block_id().clone();
        let mut command_handle = command_handle.fuse();
        let mut harness_exit_rx = harness_exit_rx.fuse();

        let scanner_fut = harness_output_monitor::watch_block_for_errors(
            block_id,
            runtime_error_patterns,
            foreground,
        )
        .fuse();
        futures::pin_mut!(scanner_fut);

        // Detected runtime error, if any. Promoted to the final return value below
        // after the select loop ends.
        let mut detected_runtime_failure: Option<harness_output_monitor::DetectedHarnessError> =
            None;

        // Handle exiting gracefully once the idle timeout elapses.
        let command_result = loop {
            futures::select! {
                exit_code = command_handle => break exit_code,
                _ = harness_exit_rx => {
                    log::debug!("Requesting harness exit");
                    report_if_error!(runner
                        .exit(foreground)
                        .await
                        .context("Failed to exit harness"));
                }
                detected = scanner_fut => {
                    if let Some(error) = detected {
                        log::warn!(
                            "Runtime failure detected for {harness_name}: pattern={}, excerpt={}",
                            error.pattern,
                            error.excerpt,
                        );
                        let session_status = foreground
                            .spawn(|me, ctx| {
                                let view_id =
                                    me.terminal_driver.as_ref(ctx).terminal_view().id();
                                CLIAgentSessionsModel::handle(ctx)
                                    .as_ref(ctx)
                                    .session(view_id)
                                    .map(|session| session.status.clone())
                            })
                            .await
                            .ok()
                            .flatten();
                        if harness_output_monitor::should_suppress_runtime_failure(
                            session_status.as_ref(),
                        ) {
                            log::info!(
                                "Ignoring runtime failure for {harness_name}: \
                                 session already marked Success or Failed via plugin \
                                 (pattern={}, excerpt={})",
                                error.pattern,
                                error.excerpt,
                            );
                        } else {
                            report_if_error!(runner
                                .exit(foreground)
                                .await
                                .context(
                                    "Failed to exit harness after runtime failure detection",
                                ));
                            detected_runtime_failure = Some(error);
                        }
                    }
                    // When the schedule exhausts without a hit, the `Fuse`
                    // wrapper makes this branch stay Pending forever, so
                    // we don't busy-loop.
                }
            }
        };

        // A runtime failure detected mid-run takes precedence over the
        // harness's own exit code: surface the actionable detail rather
        // than a generic "exit code N".
        if let Some(error) = detected_runtime_failure {
            return Err(AgentDriverError::HarnessRuntimeFailureDetected {
                harness: harness_name,
                pattern: error.pattern,
                excerpt: error.excerpt,
            });
        }

        let exit_code = command_result?;
        log::debug!("Agent harness exited with status {exit_code}");

        if exit_code.was_successful() {
            Ok(())
        } else {
            Err(AgentDriverError::HarnessCommandFailed {
                exit_code: exit_code.value(),
            })
        }
    }

    /// Selects the local profile that `profile` (an ID or unique name) names for the terminal.
    fn configure_terminal(
        &self,
        profile: Option<String>,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), AgentDriverError> {
        let Some(profile) = profile else {
            return Ok(());
        };
        let terminal_id = self.terminal_driver.as_ref(ctx).terminal_view().id();
        AIExecutionProfilesModel::handle(ctx).update(ctx, |model, ctx| {
            let profile_id = model.local_profiles(ctx).resolve(&profile)?;
            model.set_active_profile(terminal_id, profile_id, ctx);
            Ok(())
        })
    }

    fn set_base_model_override(
        &self,
        model_id: LLMId,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), AgentDriverError> {
        let terminal_view_id = self.terminal_driver.as_ref(ctx).terminal_view().id();
        log::info!("Selecting base agent model {model_id} (from agent driver)");

        LLMPreferences::handle(ctx).update(ctx, |preferences, ctx| {
            preferences.update_preferred_agent_mode_llm(&model_id, terminal_view_id, ctx);
        });
        Ok(())
    }

    /// Execute an AI run in the terminal session and wait for it to complete.
    ///
    /// Conversation output is streamed as it's available.
    fn execute_run(
        &self,
        task_prompt: String,
        ctx: &mut ModelContext<Self>,
    ) -> Receiver<SDKConversationOutputStatus> {
        // Create a oneshot channel to signal task completion.
        let (tx, rx) = oneshot::channel();
        let run_exit = IdleTimeoutSender::new(tx);

        // Subscribe before the conversation starts.
        let history_model_handle = BlocklistAIHistoryModel::handle(ctx);
        let terminal_id = self.terminal_driver.as_ref(ctx).terminal_view().id();
        let mut written_conversation_id = false;

        ctx.subscribe_to_model(&history_model_handle, move |me, _, event, ctx| {
            if event.terminal_surface_id().is_some_and(|id| id != terminal_id) {
                return;
            }

            match event {
                BlocklistAIHistoryEvent::UpdatedTodoList { .. } => {
                    // TODO: Log TODO list updates.
                }
                BlocklistAIHistoryEvent::AppendedExchange {
                    exchange_id,
                    conversation_id,
                    ..
                } => {
                    let Some(conversation) = BlocklistAIHistoryModel::as_ref(ctx)
                        .conversation(conversation_id)
                    else {
                        log::warn!("Invalid conversation ID: {conversation_id:?}");
                        return;
                    };

                    let Some(exchange) = conversation.exchange_with_id(*exchange_id) else {
                        log::warn!("Invalid exchange ID: {exchange_id:?}");
                        return;
                    };

                    // When a new exchange is appended, we should already have its inputs available.
                    report_if_error!(me
                        .write_exchange_inputs(exchange)
                        .context("Failed to write exchange inputs"));

                    // Reset the idle timer only if we've already scheduled one.
                    // This handles the case where a follow-up query creates new exchanges after
                    // the conversation has finished and an idle timer was set.
                    run_exit.cancel_idle_timeout();
                }
                BlocklistAIHistoryEvent::UpdatedStreamingExchange {
                    exchange_id,
                    conversation_id,
                    ..
                } => {
                    // Get conversation data first to avoid borrowing conflicts
                    let history_model = BlocklistAIHistoryModel::handle(ctx);
                    let conversation_data = history_model.as_ref(ctx).conversation(conversation_id)
                        .and_then(|conv| {
                            let token = conv.server_conversation_token().map(|t| t.as_str().to_string());
                            let exchange = conv.exchange_with_id(*exchange_id)?;
                            Some((token, exchange))
                        });
                    let Some((token_opt, exchange)) = conversation_data else {
                        log::warn!("Invalid conversation or exchange ID: {conversation_id:?}, {exchange_id:?}");
                        return;
                    };

                    if !written_conversation_id
                        && let Some(token) = token_opt {
                            report_if_error!(output::with_stdout_buffered(|buf| match me.output_format {
                                OutputFormat::Json | OutputFormat::Ndjson => output::json::conversation_started(&token, buf),
                                OutputFormat::Text | OutputFormat::Pretty => output::text::conversation_started(&token, buf),
                            }).context("Failed to write conversation ID"));
                            written_conversation_id = true;
                        }

                    // Once the outputs are fully streamed from the server, write them to stdout.
                    if exchange.output_status.is_finished() {
                        report_if_error!(me
                            .write_exchange_output(exchange)
                            .context("Failed to write exchange output"));
                    }

                }

                BlocklistAIHistoryEvent::UpdatedConversationStatus { terminal_surface_id: conversation_terminal_id, conversation_id, .. } => {
                    if *conversation_terminal_id != terminal_id {
                        return;
                    }
                    let history_model = BlocklistAIHistoryModel::as_ref(ctx);
                    let Some(conversation) = history_model.conversation(conversation_id) else {
                        log::warn!("No active conversation for terminal view {conversation_terminal_id} with id {conversation_id}");
                        return;
                    };

                    if conversation.status().is_in_progress() {
                        // Conversation resumed or a new one started; cancel any
                        // pending idle timeout.
                        log::info!(
                            "Ambient agent idle lifecycle: event=idle_timeout_cancel_requested terminal_view_id={terminal_id:?} trigger=conversation_in_progress",
                        );
                        run_exit.cancel_idle_timeout();
                        return;
                    }

                    // wait_for_events keeps the run alive via the
                    // action_model's running_actions; the executor owns
                    // the watchdog. Don't resolve run_exit.
                    if conversation.status().is_waiting_for_events() {
                        return;
                    }

                    if conversation.status().is_transient_error() {
                        // An automatic recovery is in flight. Don't terminate yet, but bound
                        // the wait so the CLI doesn't hang if it never completes; a successful
                        // recovery returns to InProgress, which cancels this deadline.
                        log::info!(
                            "Ambient agent idle lifecycle: event=idle_timeout_scheduled terminal_view_id={terminal_id:?} timeout={AUTO_RESUME_TIMEOUT:?} outcome=automatic_resume_pending",
                        );
                        let error = conversation
                            .root_task_exchanges()
                            .last()
                            .and_then(|exchange| match &exchange.output_status {
                                AIAgentOutputStatus::Finished {
                                    finished_output: FinishedAIAgentOutput::Error { error, .. },
                                } => Some(error.clone()),
                                _ => None,
                            })
                            .unwrap_or_else(|| {
                                RenderableAIError::transient_network_error(
                                    false,
                                    false,
                                    TransientNetworkErrorKind::MissingExchangeError,
                                )
                            });
                        run_exit.end_run_after(
                            AUTO_RESUME_TIMEOUT,
                            SDKConversationOutputStatus::Error { error },
                        );
                        return;
                    }

                    // Conversation is no longer in progress. Handle completion based on the result.
                    if let Some(conversation_status) =
                         conversation_output_status_from_conversation(conversation)
                    {
                        let output_status = match conversation_status {
                            AmbientConversationStatus::Success => {
                                SDKConversationOutputStatus::Success
                            }
                            AmbientConversationStatus::Cancelled { reason } => {
                                SDKConversationOutputStatus::Cancelled { reason }
                            }
                            AmbientConversationStatus::Error { error } => {
                                SDKConversationOutputStatus::Error { error }
                            }
                            AmbientConversationStatus::Blocked { blocked_action } => {
                                SDKConversationOutputStatus::Blocked { blocked_action }
                            }
                        };

                        // Errors here are terminal: in-flight recoveries surface as
                        // TransientError (handled above).
                        let idle_window =
                            idle_window_for_terminal_status(&output_status, me.idle_on_complete);
                        let outcome = terminal_status_log_outcome(&output_status);
                        if let Some(idle_timeout) = idle_window {
                            log::info!(
                                "Ambient agent idle lifecycle: event=idle_timeout_scheduled terminal_view_id={terminal_id:?} timeout={idle_timeout:?} outcome={outcome}",
                            );
                        } else {
                            log::info!(
                                "Ambient agent idle lifecycle: event=run_completion_immediate terminal_view_id={terminal_id:?} outcome={outcome}",
                            );
                        }
                        run_exit.complete_with_optional_idle(idle_window, output_status);
                    }
                }

                BlocklistAIHistoryEvent::SetActiveConversation { .. } => {
                    // Continuing an existing conversation should reset the idle timer.
                    run_exit.cancel_idle_timeout();
                }
                BlocklistAIHistoryEvent::StartedNewConversation { .. }
                | BlocklistAIHistoryEvent::ReassignedExchange { .. }
                | BlocklistAIHistoryEvent::ClearedConversationsForTerminalSurface { .. }
                | BlocklistAIHistoryEvent::UpdatedAutoexecuteOverride { .. }
                | BlocklistAIHistoryEvent::SplitConversation { .. }
                | BlocklistAIHistoryEvent::RemoveConversation { .. }
                | BlocklistAIHistoryEvent::DeletedConversation { .. }
                | BlocklistAIHistoryEvent::RestoredConversations { .. }
                | BlocklistAIHistoryEvent::CreatedSubtask { .. }
                | BlocklistAIHistoryEvent::UpgradedTask { .. }
                | BlocklistAIHistoryEvent::UpdatedConversationTitle { .. }
                | BlocklistAIHistoryEvent::UpdatedConversationMetadata { .. }
                | BlocklistAIHistoryEvent::ClearedActiveConversation { .. }
                | BlocklistAIHistoryEvent::UpdatedConversationArtifacts { .. }
                | BlocklistAIHistoryEvent::ConversationServerTokenAssigned { .. }
                | BlocklistAIHistoryEvent::ConversationTransferredBetweenTerminalSurfaces { .. }
                | BlocklistAIHistoryEvent::NewConversationRequestComplete { .. }
                | BlocklistAIHistoryEvent::OrchestrationConfigUpdated { .. }
                | BlocklistAIHistoryEvent::ConversationUsageMetadataUpdated { .. } => (),
            }
        });

        // Submit the AI query.
        tracing::info!("Submitting initial AI query");

        self.terminal_driver.update(ctx, |td, ctx| {
            td.with_terminal_view(ctx, |terminal, ctx| {
                if FeatureFlag::AgentView.is_enabled() {
                    terminal.enter_agent_view(
                        Some(task_prompt.clone()),
                        None,
                        AgentViewEntryOrigin::Cli,
                        ctx,
                    );
                } else {
                    terminal.set_ai_input_mode_with_query(Some(&task_prompt), ctx);
                    terminal
                        .input()
                        .update(ctx, |input, ctx| input.input_enter(ctx));
                }
            });
        });

        rx
    }

    /// Write the inputs to an exchange to stdout.
    fn write_exchange_inputs(&self, exchange: &AIAgentExchange) -> io::Result<()> {
        output::with_stdout_buffered(|buf| {
            for input in &exchange.input {
                self.write_input(buf, input)?;
            }
            Ok(())
        })
    }

    /// Write the outputs of an exchange to stdout.
    fn write_exchange_output(&self, exchange: &AIAgentExchange) -> io::Result<()> {
        let Some(shared) = exchange.output_status.output() else {
            return Ok(());
        };
        let output = shared.get();

        output::with_stdout_buffered(|buf| self.write_output(buf, &output))
    }

    /// Format an agent input for display.
    fn write_input<W: Write>(&self, w: &mut W, input: &AIAgentInput) -> io::Result<()> {
        match self.output_format {
            OutputFormat::Json | OutputFormat::Ndjson => output::json::format_input(input, w),
            OutputFormat::Text | OutputFormat::Pretty => output::text::format_input(input, w),
        }
    }

    /// Format an agent output for display.
    fn write_output<W: Write>(&self, w: &mut W, output: &AIAgentOutput) -> io::Result<()> {
        match self.output_format {
            OutputFormat::Json | OutputFormat::Ndjson => output::json::format_output(output, w),
            OutputFormat::Text | OutputFormat::Pretty => output::text::format_output(output, w),
        }
    }

    /// Subscribe to the singleton `CLIAgentSessionsModel` so that idle-on-complete
    /// timers are driven by CLI agent session status changes.
    fn subscribe_to_cli_agent_session_events(
        &self,
        harness_exit: IdleTimeoutSender<()>,
        ctx: &mut ModelContext<Self>,
    ) {
        let terminal_view_id = self.terminal_driver.as_ref(ctx).terminal_view().id();

        ctx.subscribe_to_model(&CLIAgentSessionsModel::handle(ctx), move |me, _, event, _| match event {
                CLIAgentSessionsModelEvent::StatusChanged {
                    terminal_view_id: event_tid,
                    status,
                    ..
                } => {
                    if *event_tid != terminal_view_id {
                        return;
                    }

                    // Drive the idle timer for the harness exit signal.
                    match status {
                        CLIAgentSessionStatus::Success
                        | CLIAgentSessionStatus::Failed { .. }
                        | CLIAgentSessionStatus::Blocked { .. } => {
                            let idle_window =
                                idle_window_for_cli_session_status(status, me.idle_on_complete);
                            let outcome = cli_session_status_log_outcome(status);
                            if let Some(idle_timeout) = idle_window {
                                log::info!(
                                    "Ambient agent CLI lifecycle: event=idle_timeout_scheduled terminal_view_id={terminal_view_id:?} timeout={idle_timeout:?} outcome={outcome}",
                                );
                            } else {
                                log::info!(
                                    "Ambient agent CLI lifecycle: event=run_completion_immediate terminal_view_id={terminal_view_id:?} outcome={outcome}",
                                );
                            }
                            harness_exit.complete_with_optional_idle(idle_window, ());
                        }
                        CLIAgentSessionStatus::InProgress => {
                            log::info!(
                                "Ambient agent CLI lifecycle: event=idle_timeout_cancel_requested terminal_view_id={terminal_view_id:?} trigger=session_in_progress",
                            );
                            harness_exit.cancel_idle_timeout();
                        }
                    }
                }
                CLIAgentSessionsModelEvent::SessionUpdated { .. }
                | CLIAgentSessionsModelEvent::Started { .. }
                | CLIAgentSessionsModelEvent::Ended { .. } => {}
            });
    }

    /// Handle events re-emitted by the `TerminalDriver`.
    fn handle_terminal_driver_event(
        &mut self,
        event: &TerminalDriverEvent,
        _ctx: &mut ModelContext<Self>,
    ) {
        match event {
            TerminalDriverEvent::SlowBootstrap => {
                tracing::event!(tracing::Level::WARN, "slow bootstrap");
                eprintln!(
                    "Warning: Terminal session is slow to bootstrap. See https://docs.warp.dev/support-and-community/troubleshooting-and-support/known-issues#shells to troubleshoot."
                );
            }
        }
    }
}

impl Entity for AgentDriver {
    type Event = ();
}

/// The only reason that `AgentDriver` is a singleton entity is to ensure the UI framework
/// doesn't drop it. Generally, we should not assume there's only one running agent.
impl SingletonEntity for AgentDriver {}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
