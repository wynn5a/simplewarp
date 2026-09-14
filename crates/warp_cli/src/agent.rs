use std::fmt;
use std::path::PathBuf;

use clap::builder::PossibleValue;
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use crate::config_file::ConfigFileArgs;
use crate::mcp::MCPSpec;
use crate::model::ModelArgs;
use crate::share::ShareArgs;
use crate::skill::SkillSpec;

/// Output format for agent results.
#[derive(Debug, Copy, Clone, ValueEnum, Eq, PartialEq, Default)]
pub enum OutputFormat {
    /// Output as JSON.
    #[value(name = "json")]
    Json,
    /// Output as newline-delimited JSON.
    #[value(name = "ndjson")]
    Ndjson,
    /// Output as human-readable text.
    #[default]
    #[value(name = "pretty")]
    Pretty,
    /// Output as plain text.
    #[value(name = "text")]
    Text,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.to_possible_value().expect("no values are skipped");
        f.write_str(value.get_name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    PlainText(String),
    SavedPrompt(String),
}

impl fmt::Display for Prompt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Prompt::PlainText(text) => write!(f, "Prompt: {text}"),
            Prompt::SavedPrompt(id) => write!(f, "Saved Prompt ID: {id}"),
        }
    }
}

/// Prompt arguments - mutually exclusive prompt or saved-prompt.
/// The required constraint is enforced at the command level via ArgGroup.
#[derive(Debug, Clone, Args)]
#[group(multiple = false)]
pub struct PromptArg {
    /// Prompt for the agent to carry out.
    #[arg(long = "prompt", short = 'p')]
    pub prompt: Option<String>,
    /// The saved AI prompt to run, identified by id.
    #[arg(long = "saved-prompt")]
    pub saved_prompt: Option<String>,
}

impl PromptArg {
    pub fn to_prompt(&self) -> Option<Prompt> {
        match (self.prompt.as_ref(), self.saved_prompt.as_ref()) {
            (Some(prompt), None) => Some(Prompt::PlainText(prompt.clone())),
            (None, Some(saved_prompt)) => Some(Prompt::SavedPrompt(saved_prompt.clone())),
            _ => None,
        }
    }
}

/// Hidden variant of [`ComputerUseArgs`] for commands where computer use flags
/// should be accepted but not shown in help output.
#[derive(Debug, Clone, Args, Default)]
pub struct HiddenComputerUseArgs {
    /// Enable computer use capabilities for this agent run.
    #[arg(long = "computer-use", conflicts_with = "no_computer_use", hide = true)]
    pub computer_use: bool,

    /// Disable computer use capabilities for this agent run.
    #[arg(long = "no-computer-use", conflicts_with = "computer_use", hide = true)]
    pub no_computer_use: bool,
}

impl HiddenComputerUseArgs {
    pub fn computer_use_override(&self) -> Option<bool> {
        match (self.computer_use, self.no_computer_use) {
            (true, false) => Some(true),
            (false, true) => Some(false),
            _ => None,
        }
    }
}
const HARNESS_VALUE_VARIANTS: [Harness; 5] = [
    Harness::Oz,
    Harness::Claude,
    Harness::OpenCode,
    Harness::Gemini,
    Harness::Codex,
];

/// The execution harness for an agent run.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Harness {
    /// Use Warp's built-in MAA infrastructure (default).
    #[default]
    Oz,
    /// Delegate to the `claude` CLI.
    Claude,
    /// Delegate to the `opencode` CLI.
    OpenCode,
    /// Delegate to the `gemini` CLI.
    Gemini,
    /// Delegate to the `codex` CLI.
    Codex,
    /// A harness produced by a newer client/server that this client doesn't
    /// recognize. Surfaced via deserialization fallbacks (e.g. unknown GraphQL
    /// enum values, unknown `harness_type` strings); never selectable from the
    /// CLI or harness dropdown.
    #[serde(other)]
    Unknown,
}

impl ValueEnum for Harness {
    fn value_variants<'a>() -> &'a [Self] {
        &HARNESS_VALUE_VARIANTS
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        let mut pv = match self {
            Harness::Oz => {
                PossibleValue::new("oz").help("Use Warp's built-in MAA infrastructure (default)")
            }
            Harness::Claude => PossibleValue::new("claude")
                .alias("claude-code")
                .help("Delegate to the `claude` CLI"),
            Harness::OpenCode => PossibleValue::new("opencode")
                .alias("open-code")
                .help("Delegate to the `opencode` CLI"),
            Harness::Gemini => PossibleValue::new("gemini").help("Delegate to the `gemini` CLI"),
            Harness::Codex => PossibleValue::new("codex").help("Delegate to the `codex` CLI"),
            Harness::Unknown => return None,
        };
        if !self.should_display_in_help_text() {
            pv = pv.hide(true);
        }
        Some(pv)
    }
}

impl Harness {
    pub fn parse_orchestration_harness(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
        <Self as ValueEnum>::from_str(&normalized, true).ok()
    }

    pub fn parse_local_child_harness(value: &str) -> Option<Self> {
        match Self::parse_orchestration_harness(value) {
            Some(harness @ (Self::Claude | Self::OpenCode | Self::Codex)) => Some(harness),
            Some(Self::Oz) | Some(Self::Gemini) | Some(Self::Unknown) | None => None,
        }
    }

    /// Whether this harness is surfaced to users in CLI `--help` for agent runs.
    /// Only the harnesses that are generally available are shown; gemini and
    /// opencode aren't available yet, so they're hidden from help. Update this
    /// when a harness becomes GA.
    ///
    /// This is the single source of truth for the `ValueEnum` help text; the
    /// per-variant `#[value(hide = ...)]` attributes are no longer used. It does
    /// not affect runtime acceptance — the server decides which harnesses are
    /// actually runnable.
    pub fn should_display_in_help_text(self) -> bool {
        match self {
            Self::Oz | Self::Claude | Self::Codex => true,
            Self::OpenCode | Self::Gemini | Self::Unknown => false,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Oz => "Warp Agent",
            Self::Claude => "Claude Code",
            Self::OpenCode => "OpenCode",
            Self::Gemini => "Gemini CLI",
            Self::Codex => "Codex",
            Self::Unknown => "Unknown",
        }
    }

    /// Parses a harness config-name string (the lowercase name written into
    /// `HarnessConfig::harness_type` by the spawner, e.g. `"claude"`, `"gemini"`, `"oz"`)
    /// into a [`Harness`] variant. Inverse of [`Harness::config_name`]. Returns `None` for
    /// unrecognized names so callers can distinguish a future-server harness from a
    /// round-tripped [`Harness::Unknown`]; callers that want to fall back to `Unknown`
    /// should `.unwrap_or(Harness::Unknown)`. UI surfaces should treat `Unknown` as a
    /// non-Oz, non-runnable harness.
    pub fn from_config_name(name: &str) -> Option<Self> {
        match name {
            "oz" => Some(Harness::Oz),
            "claude" => Some(Harness::Claude),
            "opencode" => Some(Harness::OpenCode),
            "gemini" => Some(Harness::Gemini),
            "codex" => Some(Harness::Codex),
            "unknown" => Some(Harness::Unknown),
            _ => None,
        }
    }

    /// Canonical config name for this harness (the lowercase string written into
    /// `HarnessConfig::harness_type`). Inverse of [`Harness::from_config_name`].
    /// The exhaustive match here forces every new [`Harness`] variant to declare a
    /// canonical name, which prevents `from_config_name` from silently falling back to
    /// `Unknown` when a new variant is added.
    pub fn config_name(self) -> &'static str {
        match self {
            Harness::Oz => "oz",
            Harness::Claude => "claude",
            Harness::OpenCode => "opencode",
            Harness::Gemini => "gemini",
            Harness::Codex => "codex",
            Harness::Unknown => "unknown",
        }
    }
}

impl fmt::Display for Harness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.config_name())
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;

/// Profile subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum AgentProfileCommand {
    /// List available agent profiles.
    List,
}

/// Agent-related subcommands.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Subcommand)]
pub enum AgentCommand {
    /// Run a new Warp Agent.
    Run(RunAgentArgs),
    /// Manage agent profiles.
    #[command(subcommand)]
    Profile(AgentProfileCommand),
}

impl AgentCommand {
    pub(crate) fn as_str_for_tracing(&self) -> &'static str {
        match self {
            AgentCommand::Run(_) => "agent run",
            AgentCommand::Profile(_) => "agent profile",
        }
    }
}

#[derive(Debug, Clone, Args)]
#[command(
    visible_alias = "r",
    group(
        clap::ArgGroup::new("prompt_group")
            .required(true)
            .multiple(true)
            .args(["prompt", "saved_prompt", "task_id", "skill"])
    )
)]
pub struct RunAgentArgs {
    #[command(flatten)]
    pub prompt_arg: PromptArg,

    #[command(flatten)]
    pub model: ModelArgs,

    #[command(flatten)]
    pub config_file: ConfigFileArgs,

    /// Use a skill as the base prompt for the agent.
    ///
    /// Format: `skill_name`, `repo:skill_name`, or `org/repo:skill_name`
    ///
    /// Skills are searched in `.agents/skills/`, `.warp/skills/`, `.claude/skills/`, and `.codex/skills/` directories.
    /// If a repo is specified, searches only that repo. If org is also specified,
    /// validates the repo's git remote matches the expected org.
    ///
    /// When used with --prompt, the skill provides the base context and the prompt is the task.
    #[arg(long = "skill", value_name = "SKILL")]
    pub skill: Option<SkillSpec>,

    /// Name for this agent task.
    #[arg(long = "name", short = 'n')]
    pub name: Option<String>,
    /// Working directory for the agent
    #[arg(short = 'C', long = "cwd")]
    pub cwd: Option<PathBuf>,
    /// Display agent progress in the Warp interface.
    #[arg(long = "gui", hide = true)]
    pub gui: bool,
    #[command(flatten)]
    pub share: ShareArgs,
    /// MCP servers to start before executing the agent.
    ///
    /// Can be specified as:
    /// - A path to a JSON file containing MCP configuration
    /// - Inline JSON with MCP server configuration
    ///
    /// Can be specified multiple times to include multiple servers.
    #[arg(long = "mcp", value_name = "SPEC")]
    pub mcp_specs: Vec<MCPSpec>,
    /// LEGACY: MCP servers to start before executing the agent, identified by UUID.
    #[arg(long = "mcp-server", value_name = "UUID", hide = true)]
    pub mcp_servers: Vec<uuid::Uuid>,
    /// Fail the run when any requested MCP server fails to start.
    ///
    /// By default, MCP servers that don't start within the startup timeout are
    /// skipped and the agent runs without their tools.
    #[arg(long = "strict-mcp-startup")]
    pub strict_mcp_startup: bool,
    /// Maximum time to wait for requested MCP servers to start (e.g. `30s`, `1m`).
    #[arg(long = "mcp-startup-timeout", value_name = "DURATION")]
    pub mcp_startup_timeout: Option<humantime::Duration>,
    /// Cloud environment to use, identified by ID.
    #[arg(long = "environment", short = 'e', value_name = "ID")]
    pub environment: Option<String>,

    /// Keep the agent's session open after the conversation completes.
    ///
    /// This is useful when you want to keep the session alive for follow-up interactions.
    ///
    /// You can optionally provide a duration (e.g. `--idle-on-complete 10m`).
    #[arg(
        long = "idle-on-complete",
        value_name = "DURATION",
        num_args = 0..=1,
        default_missing_value = "45m",
        hide = true
    )]
    pub idle_on_complete: Option<humantime::Duration>,

    /// Keep the agent's session open after the conversation ends in a terminal error, so a human
    /// can attach to the failed run and debug in it. The agent process is the shared-session
    /// sharer, so without this the session dies with the process.
    ///
    /// An idle window, not a fixed one: a follow-up cancels the pending exit.
    ///
    /// Deliberately separate from `--idle-on-complete`, which covers the success/blocked/cancelled
    /// lifecycle. Neither flag is a fallback for the other.
    ///
    /// Cloud workers set this through `OZ_IDLE_ON_FAIL` rather than the flag, so that a pinned
    /// CLI predating this option ignores it instead of rejecting an unknown argument.
    ///
    /// You can optionally provide a duration (e.g. `--idle-on-fail 10m`).
    #[arg(
        long = "idle-on-fail",
        value_name = "DURATION",
        env = "OZ_IDLE_ON_FAIL",
        num_args = 0..=1,
        default_missing_value = "15m",
        hide = true
    )]
    pub idle_on_fail: Option<humantime::Duration>,

    #[command(flatten)]
    pub snapshot: SnapshotArgs,
    /// Identifier for the task that spawned this agent, used to report progress.
    ///
    /// When `--conversation` is omitted, the conversation id is read off the server-side
    /// task metadata. Some worker follow-up call sites still pass both flags, so keep
    /// accepting the compatibility shape until all producers have been updated.
    #[arg(long = "task-id", hide = true, conflicts_with_all = ["prompt", "saved_prompt", "file"])]
    pub task_id: Option<String>,

    /// Whether we are running the agent in a sandboxed environment.
    #[arg(long = "sandboxed", hide = true)]
    pub sandboxed: bool,

    #[command(flatten)]
    pub computer_use: HiddenComputerUseArgs,

    /// Continue an existing cloud conversation by ID.
    #[arg(long = "conversation", value_name = "ID")]
    pub conversation: Option<String>,

    /// Agent profile to configure the terminal session.
    #[arg(long = "profile", value_name = "ID")]
    pub profile: Option<String>,

    /// Execution harness for the agent run.
    ///
    /// "oz" (default) uses Warp Agent.
    /// "claude" delegates to the `claude` CLI.
    #[arg(long = "harness", value_name = "HARNESS", default_value_t = Harness::Oz, hide = true)]
    pub harness: Harness,

    /// Skip the initial LLM turn for this run. Used by the empty-prompt cloud-handoff
    /// path so the cloud agent comes up ready for follow-up without hallucinating a
    /// response against an empty user message.
    ///
    /// Requires `--idle-on-complete` to also be set: with the initial turn skipped, the
    /// driver has nothing to drive a completion event, so the process would exit
    /// immediately on success without an idle window for the user's follow-up to arrive.
    #[arg(
        long = "skip-initial-turn",
        hide = true,
        requires_all = ["task_id", "idle_on_complete"],
        conflicts_with_all = ["prompt", "saved_prompt", "file"]
    )]
    pub skip_initial_turn: bool,

    #[arg(long = "configure-git-credentials-with-github", hide = true, requires_all = ["task_id"])]
    pub configure_git_credentials_with_github: bool,
}

impl RunAgentArgs {
    /// Combine `mcp_specs` with legacy `mcp_servers` (UUIDs) into a single list.
    pub fn all_mcp_specs(&self) -> Vec<MCPSpec> {
        let mut specs = self.mcp_specs.clone();
        specs.extend(self.mcp_servers.iter().cloned().map(MCPSpec::Uuid));
        specs
    }
}

#[derive(Debug, Clone, Args)]
pub struct SnapshotArgs {
    /// Disable the end-of-run workspace snapshot upload.
    #[arg(long = "no-snapshot")]
    pub no_snapshot: bool,

    /// Maximum time to wait for the end-of-run snapshot upload.
    #[arg(long = "snapshot-upload-timeout", value_name = "DURATION")]
    pub snapshot_upload_timeout: Option<humantime::Duration>,

    /// Maximum time to wait for the declarations script before uploading the snapshot.
    #[arg(long = "snapshot-script-timeout", value_name = "DURATION")]
    pub snapshot_script_timeout: Option<humantime::Duration>,
}
