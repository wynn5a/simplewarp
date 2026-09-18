use std::sync::LazyLock;
use std::time::Duration;

use markdown_parser::FormattedTextFragment;
use warpui::r#async::{SpawnedFutureHandle, Timer};
use warpui::keymap::Keystroke;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use crate::palette::PaletteMode;
use crate::server::telemetry::PaletteSource;
use crate::settings::AISettings;
use crate::terminal::input::SET_INPUT_MODE_AGENT_ACTION_NAME;
use crate::terminal::view::init::{
    CANCEL_COMMAND_KEYBINDING, SELECT_PREVIOUS_BLOCK_ACTION_NAME,
    TOGGLE_AUTOEXECUTE_MODE_KEYBINDING,
};
use crate::util::bindings::trigger_to_keystroke;
use crate::workspace::WorkspaceAction;
use crate::workspace::view::{
    TOGGLE_COMMAND_PALETTE_KEYBINDING_NAME, TOGGLE_RIGHT_PANEL_BINDING_NAME,
};
use crate::workspaces::user_workspaces::UserWorkspaces;

/// Trait for tip implementations that can be displayed to users.
/// Tips provide helpful information with optional keybindings.
pub trait AITip: Clone {
    /// Returns the keystroke for this tip, if applicable.
    fn keystroke(&self, app: &AppContext) -> Option<Keystroke>;

    /// Returns the raw description text for this tip.
    fn description(&self) -> &str;

    /// Converts the tip to formatted text fragments for rendering.
    /// Default implementation adds "Tip: " prefix and parses backtick-wrapped text as inline code.
    fn to_formatted_text(&self, _app: &AppContext) -> Vec<FormattedTextFragment> {
        let text = format!("Tip: {}", self.description());

        // Style backtick-wrapped text as inline code
        let parts: Vec<&str> = text.split('`').collect();
        let mut fragments = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }
            if i % 2 == 0 {
                fragments.push(FormattedTextFragment::plain_text(part.to_string()));
            } else {
                fragments.push(FormattedTextFragment::inline_code(part.to_string()));
            }
        }
        fragments
    }

    /// Checks if this tip is applicable in the current context.
    /// Default implementation returns true (tip is always applicable).
    fn is_tip_applicable(
        &self,
        _current_working_directory: Option<&str>,
        _app: &AppContext,
    ) -> bool {
        true
    }
}

static DEFAULT_TIPS: LazyLock<Vec<AgentTip>> = LazyLock::new(|| {
    vec![
        AgentTip {
            link: Some("https://docs.warp.dev/agents/capabilities/slash-commands".to_string()),
            description: "`/` to open the slash-command menu and access quick agent actions.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/terminal/input/universal-input#input-modes".to_string()),
            description: "<keybinding> to toggle natural language detection and switch between agent and terminal input.".to_string(),
            binding_name: Some(SET_INPUT_MODE_AGENT_ACTION_NAME),
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/capabilities/planning".to_string()),
            description: "`/plan` <prompt> to create a plan for the agent before executing.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/terminal/command-palette".to_string()),
            description: "<keybinding> to open the Command Palette and access Warp actions and shortcuts.".to_string(),
            binding_name: Some(TOGGLE_COMMAND_PALETTE_KEYBINDING_NAME),
            action: Some(WorkspaceAction::OpenPalette {
                mode: PaletteMode::Command,
                source: PaletteSource::AgentTip,
                query: None,
            }),
        },
        AgentTip {
            link: Some("https://docs.warp.dev/knowledge-and-collaboration/warp-drive".to_string()),
            description: "Store reusable workflows, notebooks, and prompts in your".to_string(),
            binding_name: None,
            action: Some(WorkspaceAction::OpenWarpDrive),
        },
        AgentTip {
            link: None,
            description: "Enter a new prompt to redirect the agent while it's running.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/local-agents/agent-context/using-to-add-context".to_string()),
            description: "`@` to add context from files, blocks, or Warp Drive objects to your prompt.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/local-agents/agent-context/blocks-as-context#attaching-blocks-as-context".to_string()),
            description: "<keybinding> to attach the prior command output as agent context.".to_string(),
            binding_name: Some(SELECT_PREVIOUS_BLOCK_ACTION_NAME),
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/capabilities/codebase-context".to_string()),
            description: "`/init` to index the repo so the agent can understand your codebase.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/capabilities/agent-profiles-permissions".to_string()),
            description: "Add agent profiles to customize permissions and models per session.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/local-agents/interacting-with-agents/conversation-forking".to_string()),
            description: "Right-click a block to fork the conversation from that point.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/terminal/blocks/block-actions#copy-input-output-of-block".to_string()),
            description: "Right-click a block to copy a conversation's output.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/local-agents/agent-context/images-as-context".to_string()),
            description: "Drag an image into the pane to attach it as agent context.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/capabilities/full-terminal-use".to_string()),
            description: "Prompt the agent to control interactive tools like node, python, postgres, gdb, or vim.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/code/code-review".to_string()),
            description: "<keybinding> to open the code review panel and review the agent's changes.".to_string(),
            binding_name: Some(TOGGLE_RIGHT_PANEL_BINDING_NAME),
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/capabilities/mcp".to_string()),
            description: "`/add-mcp` to add an MCP server to your workspace.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: None,
            description: "`/open-mcp-servers` to view and share MCP servers with your team.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/reference/cli/integration-setup".to_string()),
            description: "`/create-environment` to turn a repo into a remote docker environment an agent can run in.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: None,
            description: "`/add-prompt` to create a reusable prompt for repeatable workflows.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/capabilities/rules".to_string()),
            description: "`/add-rule` to create a global agent rule.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/local-agents/interacting-with-agents/conversation-forking".to_string()),
            description: "`/fork` to create a fresh copy of the current conversation, optionally with a new prompt.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: None,
            description: "`/open-code-review` to open the code review panel and inspect agent-generated diffs.".to_string(),
            binding_name: None,
            action: Some(WorkspaceAction::ToggleRightPanel),
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/local-agents/interacting-with-agents".to_string()),
            description: "`/new` to start a new agent conversation with clean context.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: None,
            description: "`/compact` to summarize the current conversation and free up space in the context window.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: None,
            description: "`/usage` to show your current AI credits usage.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/reference/cli".to_string()),
            description: "Use the `oz` command to run the Warp Agent in headless mode, useful for remote machines.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/local-agents/agent-context/blocks-as-context#attaching-blocks-as-context".to_string()),
            description: "Right-click selected text to attach it as agent context.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/capabilities/rules#project-rules-1".to_string()),
            description: "Use `AGENTS.md` or `CLAUDE.md` to apply project-scoped rules.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/local-agents/agent-context/urls-as-context".to_string()),
            description: "Paste a URL to attach that webpage as context for the agent.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/terminal/warpify".to_string()),
            description: "Warpify a remote SSH session to enable the Warp Agent inside that environment.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/capabilities/agent-profiles-permissions".to_string()),
            description: "Switch agent profiles to quickly change models and agent permissions.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/capabilities/rules".to_string()),
            description: "`/init` to generate a `WARP.md` file and define project rules for the agent.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/agents/capabilities/full-terminal-use#session-level-approvals".to_string()),
            description: "<keybinding> to auto-approve the agent's commands and diffs for the rest of the session.".to_string(),
            binding_name: Some(TOGGLE_AUTOEXECUTE_MODE_KEYBINDING),
            action: None,
        },
        AgentTip {
            link: Some("https://docs.warp.dev/platform/managing-cloud-agents#in-app-agent-notifications".to_string()),
            description: "Enable desktop notifications to get an alert when an agent needs your attention.".to_string(),
            binding_name: None,
            action: None,
        },
        AgentTip {
            link: None,
            description: "<keybinding> to cancel the current agent task.".to_string(),
            binding_name: Some(CANCEL_COMMAND_KEYBINDING),
            action: None,
        },
    ]
});

#[derive(Clone, Debug)]
pub struct AgentTip {
    /// The text that will be displayed to the user. This is parsed such that:
    /// "Tip: " is added as a prefix,
    /// "<keybinding>" is replaced with user-defined and platform-specific keybinding referenced by binding_name,
    /// `text` that is wrapped in backticks is formatted as inline code
    pub description: String,
    pub link: Option<String>,
    pub binding_name: Option<&'static str>,
    pub action: Option<WorkspaceAction>,
}

impl AITip for AgentTip {
    fn keystroke(&self, app: &AppContext) -> Option<Keystroke> {
        let binding_name = self.binding_name?;

        // Special case: voice input uses settings, not editable bindings
        if binding_name == "FN" {
            return AISettings::as_ref(app).voice_input_toggle_key.keystroke();
        }

        if let Some(binding) = app.editable_bindings().find(|b| b.name == binding_name) {
            return trigger_to_keystroke(binding.trigger);
        }
        None
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn to_formatted_text(&self, app: &AppContext) -> Vec<FormattedTextFragment> {
        let mut text = format!("Tip: {}", self.description);

        // Replace <keybinding> with the actual keybinding string
        if let Some(keystroke) = self.keystroke(app) {
            text = text.replace("<keybinding>", &keystroke.displayed());
        }

        // Style backtick-wrapped text as inline code
        let parts: Vec<&str> = text.split('`').collect();
        let mut fragments = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }
            if i % 2 == 0 {
                fragments.push(FormattedTextFragment::plain_text(part.to_string()));
            } else {
                fragments.push(FormattedTextFragment::inline_code(part.to_string()));
            }
        }

        fragments
    }

    fn is_tip_applicable(
        &self,
        _current_working_directory: Option<&str>,
        app: &AppContext,
    ) -> bool {
        // Tips whose description references a keybinding placeholder should only be shown
        // when the keybinding is actually configured, so we never display the raw
        // "<keybinding>" string to users.
        if self.description.contains("<keybinding>") && self.keystroke(app).is_none() {
            return false;
        }
        true
    }
}

impl WorkspaceAction {
    pub fn display_text(&self) -> Option<String> {
        match self {
            WorkspaceAction::OpenPalette { .. } => Some("Open palette".to_string()),
            WorkspaceAction::OpenWarpDrive => Some("Warp Drive.".to_string()),
            WorkspaceAction::ToggleRightPanel => Some("Show diff view".to_string()),
            _ => None,
        }
    }
}

/// Helper function to build the list of agent tips, including the voice tip if enabled.
pub fn get_agent_tips(ctx: &AppContext) -> Vec<AgentTip> {
    let mut tips = DEFAULT_TIPS.clone();

    if cfg!(feature = "voice_input")
        && UserWorkspaces::as_ref(ctx).is_voice_enabled()
        && AISettings::as_ref(ctx).is_voice_input_enabled(ctx)
    {
        tips.push(AgentTip {
            description: "Hold <keybinding> to speak your prompt directly to the agent."
                .to_string(),
            link: Some(
                "https://docs.warp.dev/agents/local-agents/interacting-with-agents/voice"
                    .to_string(),
            ),
            binding_name: Some("FN"),
            action: None,
        });
    }

    tips
}

/// A model for managing tips with cooldown logic.
/// Generic over any type implementing the AITip trait.
pub struct AITipModel<T: AITip> {
    tips: Vec<T>,
    current_tip: Option<T>,
    cooldown_handle: Option<SpawnedFutureHandle>,
}

impl<T: AITip + 'static> AITipModel<T> {
    /// Returns the current tip, if one has been selected.
    pub fn current_tip(&self) -> Option<&T> {
        self.current_tip.as_ref()
    }
}

impl<T: AITip + 'static> Entity for AITipModel<T> {
    type Event = ();
}

// Specific implementation for AgentTip
impl AITipModel<AgentTip> {
    /// Creates a new AITipModel for AgentTips.
    /// This is the constructor used for the singleton model.
    pub fn new_for_agent_tips(ctx: &AppContext) -> Self {
        let tips = get_agent_tips(ctx);
        // Pick an applicable tip so we never show a raw "<keybinding>" placeholder on first render.
        let current_tip = Self::pick_random_applicable_tip(&tips, None, ctx);

        Self {
            tips,
            current_tip,
            cooldown_handle: None,
        }
    }

    /// Rebuilds the tip pool from current settings and invalidates the current tip
    /// if it is no longer applicable. Resets the cooldown timer so the revalidated
    /// tip is shown for the full cooldown period before the next rotation.
    pub fn revalidate_tips(&mut self, ctx: &mut ModelContext<Self>) {
        self.tips = get_agent_tips(ctx);

        // If the current tip is no longer in the pool or no longer applicable, pick a new one.
        let should_replace = self
            .current_tip
            .as_ref()
            .map(|current_tip| {
                let still_in_pool = self
                    .tips
                    .iter()
                    .any(|tip| tip.description == current_tip.description);

                !still_in_pool || !current_tip.is_tip_applicable(None, ctx)
            })
            .unwrap_or(true);

        if should_replace {
            let new_tip = Self::pick_random_applicable_tip(&self.tips, None, ctx);
            if new_tip.is_some() || self.current_tip.is_some() {
                self.current_tip = new_tip;
                self.reset_cooldown(ctx);
                ctx.notify();
            }
        }
    }

    /// Refreshes the current tip with a new random selection that is applicable
    /// for the given working directory.
    /// Only updates if not in cooldown period (60 seconds).
    pub fn maybe_refresh_tip(
        &mut self,
        current_working_directory: Option<&str>,
        ctx: &mut ModelContext<Self>,
    ) {
        // Don't update if cooldown is active
        if self.cooldown_handle.is_some() {
            return;
        }

        // Rebuild tips from current settings so changes are picked up.
        self.tips = get_agent_tips(ctx);

        self.current_tip =
            Self::pick_random_applicable_tip(&self.tips, current_working_directory, ctx);

        // Start 60-second cooldown
        let handle = ctx.spawn(
            async {
                Timer::after(Duration::from_secs(60)).await;
            },
            |me, _, _| {
                me.cooldown_handle = None;
            },
        );
        self.cooldown_handle = Some(handle);
        ctx.notify();
    }

    /// Picks a random applicable tip from the given pool, filtered by working directory.
    /// Returns `None` if no tips are applicable.
    fn pick_random_applicable_tip(
        tips: &[AgentTip],
        current_working_directory: Option<&str>,
        ctx: &AppContext,
    ) -> Option<AgentTip> {
        use rand::seq::SliceRandom;
        let available: Vec<&AgentTip> = tips
            .iter()
            .filter(|tip| tip.is_tip_applicable(current_working_directory, ctx))
            .collect();
        let mut rng = rand::thread_rng();
        available.choose(&mut rng).copied().cloned()
    }

    /// Resets the cooldown timer so the current tip is shown for the full
    /// cooldown period before the next rotation.
    fn reset_cooldown(&mut self, ctx: &mut ModelContext<Self>) {
        if let Some(handle) = self.cooldown_handle.take() {
            handle.abort();
        }
        let handle = ctx.spawn(
            async {
                Timer::after(Duration::from_secs(60)).await;
            },
            |me, _, _| {
                me.cooldown_handle = None;
            },
        );
        self.cooldown_handle = Some(handle);
    }
}

impl SingletonEntity for AITipModel<AgentTip> {}
