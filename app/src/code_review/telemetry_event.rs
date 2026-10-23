use std::fmt::Display;

use serde::Serialize;
use serde_with::SerializeDisplay;

/// Entry points for opening the code review pane.
#[derive(Clone, Copy, Debug, SerializeDisplay, Default)]
pub enum CodeReviewPaneEntrypoint {
    /// Opened via the git diff chip (git changes button in AI control panel).
    GitDiffChip,
    /// Opened via the "View changes" button when Agent mode is done running.
    AgentModeCompleted,
    /// Opened via the "Review changes" button when Agent mode is running.
    AgentModeRunning,
    /// Opened via the "/code-review" slash command.
    SlashCommand,
    /// Opened by the agent tool call.
    InvokedByAgent,
    // Force opened when user accepted first diff of a conversation
    ForceOpened,
    // Opened via the agent mode diff header
    CodeDiffHeader,
    // Opened via the pane header
    PaneHeader,
    // Opened via the code mode v2 right panel button
    RightPanel,
    /// Opened via the CLI agent view footer (e.g., Claude Code).
    CLIAgentView,
    /// Opened via other means (unknown entry point).
    #[default]
    Other,
}

impl Display for CodeReviewPaneEntrypoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GitDiffChip => write!(f, "git_diff_chip"),
            Self::AgentModeCompleted => write!(f, "agent_mode_completed"),
            Self::AgentModeRunning => write!(f, "agent_mode_running"),
            Self::SlashCommand => write!(f, "slash_command"),
            Self::InvokedByAgent => write!(f, "invoked_by_agent"),
            Self::ForceOpened => write!(f, "force_opened"),
            Self::CodeDiffHeader => write!(f, "agent_mode_diff_header"),
            Self::PaneHeader => write!(f, "pane_header"),
            Self::RightPanel => write!(f, "right_panel"),
            Self::CLIAgentView => write!(f, "cli_agent_view"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// Where code review content was sent after the user action.
#[derive(Clone, Copy, Debug, Serialize)]
pub enum CodeReviewContextDestination {
    /// Written directly to the terminal PTY for an active CLI agent.
    #[serde(rename = "pty")]
    Pty,
    /// Inserted into the Warp AI input buffer as plain text.
    #[serde(rename = "agent_input")]
    AgentInput,
    /// Registered as an AI attachment and referenced from the input.
    #[serde(rename = "agent_attachment")]
    AgentAttachment,
    /// Inserted into the active command buffer while a command is running.
    #[serde(rename = "active_command_buffer")]
    ActiveCommandBuffer,
    /// Submitted as an inline code review request through the Warp AI path.
    #[serde(rename = "agent_review")]
    AgentReview,
    /// Inserted into CLI agent rich input.
    #[serde(rename = "rich_input")]
    RichInput,
}
