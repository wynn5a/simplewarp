//! Settings for Blocklist AI.
//!
//! These settings are currently used to configure the underlying model/API used to power the AI
//! UX, as well as small UX configurations.

use std::collections::HashMap;
use std::path::PathBuf;

pub use cloud_object_models::{
    AgentModeCommandExecutionPredicate, DEFAULT_COMMAND_EXECUTION_ALLOWLIST,
    DEFAULT_COMMAND_EXECUTION_DENYLIST,
};
use indexmap::IndexMap;
use regex::Regex;
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use settings::{Setting, SupportedPlatforms, define_settings_group};
use strum_macros::EnumIter;
use warp_core::execution_mode::AppExecutionMode;
use warp_core::features::FeatureFlag;
use warp_errors::report_if_error;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use crate::ai::execution_profiles::ExecutionProfilesConfig;
use crate::terminal::CLIAgent;

/// The default mode for new terminal sessions.
#[derive(
    Default,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    Copy,
    Clone,
    EnumIter,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "Default mode for new sessions.",
    rename_all = "snake_case"
)]
pub enum DefaultSessionMode {
    /// New sessions start in the terminal mode (default).
    #[default]
    Terminal,
    /// New sessions start in agent view.
    Agent,
    /// New sessions start in cloud (ambient) agent mode.
    CloudAgent,
    /// New sessions open a user-defined tab config.
    /// The specific config is identified by the companion `default_tab_config_path` setting.
    TabConfig,
    /// New sessions open in a local Docker sandbox.
    /// Requires the `LocalDockerSandbox` feature flag; falls back to `Terminal` when disabled.
    DockerSandbox,
}

settings::macros::implement_setting_for_enum!(
    DefaultSessionMode,
    AISettings,
    SupportedPlatforms::ALL,
    surface: settings::SettingSurfaces::GUI,
    private: false,
    toml_path: "general.default_session_mode",
    description: "The default mode for new terminal sessions.",
);

impl DefaultSessionMode {
    /// Display name for the settings dropdown.
    pub fn display_name(&self) -> &'static str {
        match self {
            DefaultSessionMode::Terminal => "Terminal",
            DefaultSessionMode::Agent => "Agent",
            DefaultSessionMode::CloudAgent => "Cloud agent",
            DefaultSessionMode::TabConfig => "Tab Config",
            DefaultSessionMode::DockerSandbox => "Local Docker Sandbox",
        }
    }
}

/// Controls how agent thinking/reasoning traces are displayed after streaming.
#[derive(
    Default,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    Copy,
    Clone,
    EnumIter,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "Controls how agent thinking is displayed after streaming.",
    rename_all = "snake_case"
)]
pub enum ThinkingDisplayMode {
    /// Show reasoning blocks while streaming, then collapse them when complete (default).
    #[default]
    ShowAndCollapse,
    /// Always keep reasoning blocks expanded, even after streaming finishes.
    AlwaysShow,
    /// Never show reasoning blocks.
    NeverShow,
}

settings::macros::implement_setting_for_enum!(
    ThinkingDisplayMode,
    AISettings,
    SupportedPlatforms::ALL,
    surface: settings::SettingSurfaces::GUI,
    private: false,
    toml_path: "agents.warp_agent.other.thinking_display_mode",
    description: "Controls how agent thinking traces are displayed after streaming.",
);

impl ThinkingDisplayMode {
    /// Display name for the settings dropdown.
    pub fn display_name(&self) -> &'static str {
        match self {
            ThinkingDisplayMode::ShowAndCollapse => "Show & collapse",
            ThinkingDisplayMode::AlwaysShow => "Always show",
            ThinkingDisplayMode::NeverShow => "Never show",
        }
    }

    pub fn command_palette_description(&self) -> &'static str {
        match self {
            ThinkingDisplayMode::ShowAndCollapse => "Set agent thinking display: show & collapse",
            ThinkingDisplayMode::AlwaysShow => "Set agent thinking display: always show",
            ThinkingDisplayMode::NeverShow => "Set agent thinking display: never show",
        }
    }

    pub fn should_render(&self) -> bool {
        !matches!(self, ThinkingDisplayMode::NeverShow)
    }

    pub fn should_keep_expanded(&self) -> bool {
        matches!(self, ThinkingDisplayMode::AlwaysShow)
    }
}

/// Controls how child-agent message bodies are displayed.
#[derive(
    Default,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    Copy,
    Clone,
    EnumIter,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "Controls how child-agent messages are displayed.",
    rename_all = "snake_case"
)]
pub enum OrchestrationMessageDisplayMode {
    /// Show child-agent messages while streaming, then collapse them.
    ShowAndCollapse,
    /// Keep child-agent message bodies expanded.
    AlwaysShow,
    /// Keep child-agent message bodies collapsed.
    #[default]
    AlwaysCollapse,
}

settings::macros::implement_setting_for_enum!(
    OrchestrationMessageDisplayMode,
    AISettings,
    SupportedPlatforms::ALL,
    surface: settings::SettingSurfaces::GUI,
    private: false,
    toml_path: "agents.warp_agent.other.orchestration_message_display_mode",
    description: "Controls how child-agent messages are displayed.",
);

impl OrchestrationMessageDisplayMode {
    /// Display name for the settings dropdown.
    pub fn display_name(&self) -> &'static str {
        match self {
            OrchestrationMessageDisplayMode::ShowAndCollapse => "Show & collapse",
            OrchestrationMessageDisplayMode::AlwaysShow => "Always show",
            OrchestrationMessageDisplayMode::AlwaysCollapse => "Always collapse",
        }
    }

    pub fn command_palette_description(&self) -> &'static str {
        match self {
            OrchestrationMessageDisplayMode::ShowAndCollapse => {
                "Set child-agent message display: show & collapse"
            }
            OrchestrationMessageDisplayMode::AlwaysShow => {
                "Set child-agent message display: always show"
            }
            OrchestrationMessageDisplayMode::AlwaysCollapse => {
                "Set child-agent message display: always collapse"
            }
        }
    }

    /// Whether child-agent message bodies should expand while streaming.
    pub fn should_expand_agent_message_body(&self) -> bool {
        matches!(
            self,
            OrchestrationMessageDisplayMode::ShowAndCollapse
                | OrchestrationMessageDisplayMode::AlwaysShow
        )
    }

    /// Whether child-agent message bodies should collapse after streaming.
    pub fn should_collapse_agent_message_body_on_finish(&self) -> bool {
        matches!(self, OrchestrationMessageDisplayMode::ShowAndCollapse)
    }
}

/// Controls what happens when a user submits a new prompt while the agent is
/// still responding to an earlier prompt.
///
/// This is the *default* used when a conversation has no explicit auto-queue
/// override. Per-conversation overrides live on `QueuedQueryModel` and take
/// precedence over this setting.
#[derive(
    Default,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    Copy,
    Clone,
    EnumIter,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "Default behavior when submitting a new prompt while the agent is still responding.",
    rename_all = "snake_case"
)]
pub enum PromptSubmissionMode {
    /// Cancel the in-flight response and submit the new prompt immediately
    /// (default).
    #[default]
    Interrupt,
    /// Hold the new prompt until the in-flight response finishes, then submit.
    Queue,
}

settings::macros::implement_setting_for_enum!(
    PromptSubmissionMode,
    AISettings,
    SupportedPlatforms::ALL,
    surface: settings::SettingSurfaces::GUI,
    private: false,
    toml_path: "agents.warp_agent.other.default_prompt_submission_mode",
    description: "Default behavior when submitting a new prompt while the agent is still responding.",
    feature_flag: FeatureFlag::QueueSlashCommand,
);

impl PromptSubmissionMode {
    /// Display name for the settings dropdown.
    pub fn display_name(&self) -> &'static str {
        match self {
            PromptSubmissionMode::Interrupt => "Interrupt response",
            PromptSubmissionMode::Queue => "Queue until response finishes",
        }
    }

    pub fn command_palette_description(&self) -> &'static str {
        match self {
            PromptSubmissionMode::Interrupt => "Set default prompt submission: interrupt response",
            PromptSubmissionMode::Queue => {
                "Set default prompt submission: queue until response finishes"
            }
        }
    }
}

/// What happens when a prompt is submitted while an agent controls an agent-requested
/// long-running command (LRC).
///
/// Only consulted when [`PromptSubmissionMode`] is `Interrupt`: in `Queue` mode
/// prompts always queue until the full response finishes, so this setting is
/// hidden and ignored.
#[derive(
    Default,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    Copy,
    Clone,
    EnumIter,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "What happens when a prompt is submitted while an agent controls an agent-requested long-running command.",
    rename_all = "snake_case"
)]
pub enum LongRunningCommandSubmissionMode {
    /// Send the prompt to the agent immediately, steering it mid-command.
    SendImmediately,
    /// Queue the prompt and send it to the agent when the command finishes
    /// (default).
    #[default]
    QueueUntilCommandCompletes,
}

settings::macros::implement_setting_for_enum!(
    LongRunningCommandSubmissionMode,
    AISettings,
    SupportedPlatforms::ALL,
    surface: settings::SettingSurfaces::GUI,
    private: false,
    toml_path: "agents.warp_agent.other.long_running_command_submission_mode",
    description: "What happens when a prompt is submitted while an agent controls an agent-requested long-running command.",
    feature_flag: FeatureFlag::QueueSlashCommand,
);

impl LongRunningCommandSubmissionMode {
    /// Display name for the settings dropdown.
    pub fn display_name(&self) -> &'static str {
        match self {
            LongRunningCommandSubmissionMode::SendImmediately => "Send immediately",
            LongRunningCommandSubmissionMode::QueueUntilCommandCompletes => {
                "Queue until command finishes"
            }
        }
    }

    pub fn command_palette_description(&self) -> &'static str {
        match self {
            LongRunningCommandSubmissionMode::SendImmediately => {
                "Set long-running command submission: send immediately"
            }
            LongRunningCommandSubmissionMode::QueueUntilCommandCompletes => {
                "Set long-running command submission: queue until command finishes"
            }
        }
    }
}

#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    Default,
    PartialEq,
    EnumIter,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "File read permission level for the agent.",
    rename_all = "snake_case"
)]
pub enum AgentModeCodingPermissionsType {
    /// Agent Mode must ask for explicit permission for any type of file read.
    #[default]
    AlwaysAskBeforeReading,
    /// Agent Mode can always read files without explicit consent.
    AlwaysAllowReading,
    /// Agent Mode can only read certain files without explicit consent.
    ///
    /// The specific filepaths are backed by the
    /// [`AISettings::agent_mode_coding_file_read_allowlist`] setting.
    AllowReadingSpecificFiles,
}

/// Maps custom toolbar command regex patterns to CLI agent names.
/// Keys are regex patterns (insertion-ordered), values are serialized CLIAgent names (e.g. "Claude").
/// An empty string value means "Any CLI Agent" (CLIAgent::Unknown).
///
/// Uses `IndexMap` to preserve insertion order so the settings UI list is deterministic.
/// Supports backward-compatible deserialization from the legacy `Vec<String>` format,
/// where each string is converted to a key with an empty agent value.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolbarCommandMap(IndexMap<String, String>);

impl ToolbarCommandMap {
    pub(crate) fn new(map: IndexMap<String, String>) -> Self {
        Self(map)
    }
}

impl<'de> Deserialize<'de> for ToolbarCommandMap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum MapOrVec {
            Map(IndexMap<String, String>),
            Vec(Vec<String>),
        }

        match MapOrVec::deserialize(deserializer) {
            Ok(MapOrVec::Map(map)) => Ok(ToolbarCommandMap::new(map)),
            Ok(MapOrVec::Vec(vec)) => {
                let map = vec
                    .into_iter()
                    .map(|pattern| (pattern, String::new()))
                    .collect();
                Ok(ToolbarCommandMap::new(map))
            }
            Err(e) => Err(e),
        }
    }
}

impl schemars::JsonSchema for ToolbarCommandMap {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ToolbarCommandMap")
    }

    fn json_schema(r#gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        r#gen.subschema_for::<HashMap<String, String>>()
    }
}

impl std::ops::Deref for ToolbarCommandMap {
    type Target = IndexMap<String, String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl settings_value::SettingsValue for ToolbarCommandMap {
    fn to_file_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.0).unwrap_or_default()
    }

    fn from_file_value(value: &serde_json::Value) -> Option<Self> {
        // Try map format first (using from_value to preserve insertion order), then legacy array format.
        if value.is_object()
            && let Ok(map) = serde_json::from_value::<IndexMap<String, String>>(value.clone())
        {
            return Some(ToolbarCommandMap::new(map));
        }
        if let Some(arr) = value.as_array() {
            let result: IndexMap<String, String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| (s.to_string(), String::new())))
                .collect();
            return Some(ToolbarCommandMap::new(result));
        }
        None
    }
}

define_settings_group!(AISettings, settings: [
    // If `false`, all AI features are disabled.
    is_any_ai_enabled: IsAnyAIEnabled {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.is_any_ai_enabled",
        description: "Controls whether all AI features are enabled.",
    },
    // This field should not be referenced directly to lookup active AI enablement -- use the
    // `is_active_ai_enabled()` getter.
    is_active_ai_enabled_internal: IsActiveAIEnabled {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.active_ai.enabled",
        description: "Controls whether proactive AI features like suggestions are enabled.",
    },
    // This field should not be referenced directly to lookup autodetection enablement -- use the
    // `is_ai_autodetection_enabled()` getter.
    ai_autodetection_enabled_internal: AIAutoDetectionEnabled {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::ALL,
        private: false,
        toml_path: "agents.warp_agent.input.ai_auto_detection_enabled",
        description: "Controls whether AI automatically detects natural language input.",
    },
    // This field should not be referenced directly -- use the
    // `is_nld_in_terminal_enabled()` getter.
    // Controls whether natural language detection is enabled in the terminal input.
    //
    // This is only used when `FeatureFlag::AgentView` is enabled.
    nld_in_terminal_enabled_internal: NLDInTerminalEnabled {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.input.nld_in_terminal_enabled",
        description: "Controls whether natural language detection is enabled in the terminal input.",
    },
    autodetection_command_denylist: AICommandDenylist {
        type: String,
        default: String::new(),
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.input.ai_command_denylist",
        description: "Commands to exclude from AI natural language autodetection.",
    },
    // This field should not be referenced directly to lookup intelligent autosuggestion enablement
    // -- use the `is_intelligent_autosuggestions_enabled()` getter.
    intelligent_autosuggestions_enabled_internal: IntelligentAutosuggestionsEnabled {
        type: bool,
        default: true, // TODO(roland): revisit this when launched to stable
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.active_ai.intelligent_autosuggestions_enabled",
        description: "Controls whether AI-powered intelligent autosuggestions are enabled.",
    }
    // This field should not be referenced directly to lookup Prompt Suggestions
    // enablement -- use the `is_prompt_suggestions_enabled()` getter.
    // Note that AgentModeQuerySuggestionsEnabled is a legacy name (the feature was initially named Agent
    // Mode Query Suggestions), however, we do not want to change the name of the setting key to avoid
    // breaking existing user settings.
    prompt_suggestions_enabled_internal: AgentModeQuerySuggestionsEnabled {
        type: bool,
        default: true, // TODO(advait): revisit this when launched to stable
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.active_ai.agent_mode_query_suggestions_enabled",
        description: "Controls whether prompt suggestions are shown in agent mode.",
    }

    // This field should not be referenced directly to lookup Code Suggestions
    // enablement -- use the `is_code_suggestions_enabled()` getter.
    code_suggestions_enabled_internal: CodeSuggestionsEnabled {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.active_ai.code_suggestions_enabled",
        description: "Controls whether AI code suggestions are enabled.",
    }
    // This field should not be referenced directly to lookup git operations AI autogen
    // enablement -- use the `is_git_operations_autogen_enabled()` getter.
    git_operations_autogen_enabled_internal: GitOperationsAutogenEnabled {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.active_ai.git_operations_autogen_enabled",
        description: "Controls whether AI auto-generates commit messages and PR title/body in the code review dialogs.",
    }
    // This field should not be referenced directly to lookup Rule Suggestions
    // enablement -- use the `is_rule_suggestions_enabled()` getter.
    rule_suggestions_enabled_internal: RuleSuggestionsEnabled {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.active_ai.rule_suggestions_enabled",
        description: "Controls whether the agent suggests rules to save after responses.",
        feature_flag: FeatureFlag::SuggestedRules,
    }
    // Predicates that Agent Mode can use to decide if it can execute
    // a command without explicit user consent.
    //
    // Prefer [`BlocklistAIPermissions::can_autoexecute_command`] to
    // interpret this allowlist.
    agent_mode_command_execution_allowlist: AgentModeCommandExecutionAllowlist {
        type: Vec<AgentModeCommandExecutionPredicate>,
        default: DEFAULT_COMMAND_EXECUTION_ALLOWLIST.clone(),
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::ALL,
        private: false,
        toml_path: "agents.profiles.agent_mode_command_execution_allowlist",
        description: "Commands that the agent can execute without explicit permission.",
    },
    // Predicates that Agent Mode can use to decide if a command must
    // be executed by the user.
    //
    // Prefer [`BlocklistAIPermissions::can_autoexecute_command`] to
    // interpret this denylist.
    agent_mode_command_execution_denylist: AgentModeCommandExecutionDenylist {
        type: Vec<AgentModeCommandExecutionPredicate>,
        default: DEFAULT_COMMAND_EXECUTION_DENYLIST.clone(),
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::ALL,
        private: false,
        toml_path: "agents.profiles.agent_mode_command_execution_denylist",
        description: "Commands that the agent must always ask before executing.",
    },
    // Enabled iff Agent Mode can execute readonly commands without explicit user consent.
    //
    // Prefer [`BlocklistAIPermissions::can_autoexecute_command`] to
    // interpret this setting.
    agent_mode_execute_read_only_commands: AgentModeExecuteReadonlyCommands {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::ALL,
        private: false,
        toml_path: "agents.profiles.agent_mode_execute_readonly_commands",
        description: "Whether the agent can auto-execute read-only commands without asking.",
    },
    // Determines coding permissions that Agent Mode has.
    // Note that if Agent Mode has permissions to execute readonly commands,
    // that automatically gives Agent Mode the ability to also _read_ files for coding
    // tasks, including codebase search.
    //
    // Prefer [`BlocklistAIPermissions::can_read_file`] to interpret this setting.
    agent_mode_coding_permissions: AgentModeCodingPermissions {
        type: AgentModeCodingPermissionsType,
        default: AgentModeCodingPermissionsType::default(),
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::ALL,
        private: false,
        toml_path: "agents.profiles.agent_mode_coding_permissions",
        description: "The file read permission level for the agent.",
    }
    // Specific filepaths that Agent Mode can read without asking for additional permissions.
    // These should be persisted as absolute filepaths to avoid ambiguity.
    //
    // This is used in conjunction with [`AgentModeCodingPermissionsType::AllowReadingSpecificFiles`].
    //
    // Prefer [`BlocklistAIPermissions::can_read_file`] to interpret this setting.
    agent_mode_coding_file_read_allowlist: AgentModeCodingFileReadAllowlist {
        type: Vec<PathBuf>,
        default: vec![],
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::ALL,
        private: false,
        toml_path: "agents.profiles.agent_mode_coding_file_read_allowlist",
        description: "File paths the agent can read without asking for permission.",
    }
    // The complete execution-profile collection.
    execution_profiles: ExecutionProfiles {
        type: ExecutionProfilesConfig,
        default: ExecutionProfilesConfig::default(),
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::ALL,
        private: false,
        toml_path: "agents.execution_profiles",
        max_table_depth: 2,
        description: "AI execution profiles and their permissions.",
    }
    // Whether or not the profile-level command autoexecution speedbump has been shown.
    //
    // Not a user-visible setting - we model it as a setting so we can track how often
    // it's shown across devices.
    has_shown_agent_mode_profile_command_autoexecution_speedbump: HasShownAgentModeProfileCommandAutoexecutionSpeedbump {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    }
    // Whether or not we should show the speedbump for auto-executing readonly cmds.
    //
    // Not a user-visible settings - we model it as a setting so we can track how often
    // it's shown across devices.
    should_show_agent_mode_autoexecute_readonly_commands_speedbump: ShouldShowAgentModeModelExecuteReadonlyCommandsSpeedbump {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    }
    // Whether or not we should show the speedbump for auto-writing to the PTY.
    //
    // Not a user-visible settings - we model it as a setting so we can track how often
    // it's shown across devices.
    should_show_agent_mode_write_to_pty_speedbump: ShouldShowAgentModeWriteToPtySpeedbump {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    }
    // Whether or not we should show the speedbump for auto-reading files.
    //
    // Not a user-visible settings - we model it as a setting so we can track how often
    // it's shown across devices.
    should_show_agent_mode_autoread_files_speedbump: ShouldShowAgentModeCodingReadPermissionsNudge {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    }
    // Whether or not we should show the one-shot speedbump on Ask-User-Question cards.
    //
    // Not a user-visible setting - we model it as a setting so we can track state.
    should_show_agent_mode_ask_user_question_speedbump: ShouldShowAgentModeAskUserQuestionSpeedbump {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    }
    // Whether or not the user wants agent mode requests to use their saved rules.
    memory_enabled: MemoryEnabled {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.knowledge.rules_enabled",
        description: "Whether the agent uses your saved rules during requests.",
    }
    // Whether the agent mode setup banner has been shown for a given repo path.
    // Once shown, it will not be shown again for that repo.
    //
    // Not a user-visible settings - we model it as a setting so we can track state.
    agent_mode_setup_banner_shown_for_repo_paths: AgentModeSetupBannerShownForRepoPaths {
        type: Vec<PathBuf>,
        default: vec![],
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    }

    // Whether or not we should show the speedbump for showing code suggestion banners.
    // This includes both passive code diffs and suggested prompts (passive unit tests).
    //
    // Not a user-visible settings - we model it as a setting so we can track if the speedbump has already been shown or not.
    show_code_suggestion_speedbump: ShouldShowCodeSuggestionSpeedbump {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    }

    mcp_execution_path: MCPExecutionPath {
        type: Option<String>,
        default: None,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },

    should_render_use_agent_footer_for_user_commands: ShouldRenderUseAgentToolbarForUserCommands {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.other.should_render_use_agent_toolbar_for_user_commands",
        description: "Whether to show the \"Use Agent\" footer for terminal commands.",
    }

    // Whether to render the CLI agent footer for commands like Claude, Codex, Gemini, etc.
    // This is independent of the "Use Agent" footer setting.
    should_render_cli_agent_footer: ShouldRenderCLIAgentToolbar {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.third_party.should_render_cli_agent_toolbar",
        description: "Whether to show the CLI agent footer for coding agent commands.",
    }
    // When enabled and a CLI agent session has a pl
    // Maps custom toolbar command regex patterns to specific CLI agents.
    // Keys are regex patterns matched against the full command string.
    // Values are serialized CLIAgent names (empty string = any agent).
    // Supports migration from the legacy Vec<String> format.
    cli_agent_footer_enabled_commands: CLIAgentToolbarEnabledCommands {
        type: ToolbarCommandMap,
        default: ToolbarCommandMap::default(),
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.third_party.cli_agent_toolbar_enabled_commands",
        max_table_depth: 1,
        description: "Maps custom toolbar command patterns to specific CLI agents.",
    }

    // This is not a user-visible setting - it tracks whether a paid user has dismissed the
    // agent management help page by clicking "View Agents".
    //
    // When false and user is on a paid plan, the help page is shown.
    // When true, the help page is hidden (user dismissed it).
    // Free users never see the help page by default regardless of this setting.
    did_dismiss_cloud_setup_guide: DidDismissAgentManagementHelpPage {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    }

    // Whether the ambient agent trial widget has been dismissed by the user.
    //
    // Not a user-visible setting - we model it as a setting so we can track state.
    ambient_agent_trial_widget_dismissed: AmbientAgentTrialWidgetDismissed {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    }

    // The raw stored default mode for new sessions. Use `default_session_mode()` to retrieve the
    // effective value, which is gated on AI availability.
    default_session_mode_internal: DefaultSessionMode,

    // The file path of the tab config used when default_session_mode_internal is TabConfig.
    // Only read when mode is TabConfig; ignored for all other modes.
    // Machine-local (tab config paths vary per machine), so never synced to cloud.
    default_tab_config_path: DefaultTabConfigPath {
        type: String,
        default: String::new(),
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "general.default_tab_config_path",
    }

    // Whether computer use is enabled for cloud agent conversations started from the Warp app.
    // This setting is only used when the AI autonomy setting is AlwaysAsk or not set.
    cloud_agent_computer_use_enabled: CloudAgentComputerUseEnabled {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::DESKTOP,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.other.cloud_agent_computer_use_enabled",
        description: "Whether computer use is enabled for cloud agent conversations.",
    }


    // Whether file-based MCP servers from third-party AI tools (e.g. Claude, Codex) should
    // be automatically detected and spawned. Warp-native config files (.warp/.mcp.json) are
    // always detected and spawned, regardless of this setting.
    file_based_mcp_enabled: FileBasedMcpEnabled {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::DESKTOP,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.mcp_servers.file_based_mcp_enabled",
        description: "Whether third-party file-based MCP servers are automatically detected.",
    }

    // Controls how agent thinking/reasoning traces are displayed.
    thinking_display_mode: ThinkingDisplayMode,

    // Controls how orchestration message bodies are expanded by default.
    orchestration_message_display_mode: OrchestrationMessageDisplayMode,

    // Default behavior when the user submits a new prompt while the agent is still
    // responding. Per-conversation overrides live on `QueuedQueryModel`; this
    // setting is the fallback used when a conversation has no explicit override.
    default_prompt_submission_mode: PromptSubmissionMode,

    // What happens when a prompt is submitted while an agent controls an agent-requested
    // long-running command. Only consulted when `default_prompt_submission_mode` is `Interrupt`;
    // per-LRC manual overrides live on `QueuedQueryModel`.
    long_running_command_submission_mode: LongRunningCommandSubmissionMode,

    // Whether agent-executed shell commands should be included in command history
    // (up-arrow, Ctrl-R search, inline history menu).
    // When false, commands run by the AI agent are excluded from history.
    include_agent_commands_in_history: IncludeAgentCommandsInHistory {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.input.include_agent_commands_in_history",
        description: "Whether agent-executed commands are included in command history.",
    }

    // Whether fast forward / auto-approve can run commands that match the command denylist.
    auto_approve_bypasses_command_denylist: AutoApproveBypassesCommandDenylist {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::ALL,
        private: false,
        toml_path: "agents.warp_agent.other.auto_approve_bypasses_command_denylist",
        description: "Whether auto-approve bypasses the command denylist.",
    }

    // Controls whether the conversation history view appears in the tools panel.
    show_conversation_history: ShowConversationHistory {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.other.show_conversation_history",
        description: "Whether conversation history appears in the tools panel.",
    }


    // Controls whether agent notifications (mailbox button, toasts, notification items) are shown.
    show_agent_notifications: ShowAgentNotifications {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.other.show_agent_notifications",
        description: "Whether agent notifications are shown.",
    }

    // Per-agent, per-host tracking of whether the user dismissed the plugin install chip.
    // Keys are "<agent_prefix>" for local sessions or "<agent_prefix>@<host>" for remote.
    // Local-only so dismissal doesn't sync across devices.
    plugin_install_chip_dismissed_map: PluginInstallChipDismissedMap {
        type: HashMap<String, bool>,
        default: HashMap::default(),
        supported_platforms: SupportedPlatforms::DESKTOP,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    }

    // Per-agent, per-host tracking of the MINIMUM_PLUGIN_VERSION for which the user
    // dismissed the plugin update chip. Empty/absent means not dismissed.
    // Keys are "<agent_prefix>" for local sessions or "<agent_prefix>@<host>" for remote.
    // Local-only so dismissal doesn't sync across devices.
    plugin_update_chip_dismissed_for_version_map: PluginUpdateChipDismissedForVersionMap {
        type: HashMap<String, String>,
        default: HashMap::default(),
        supported_platforms: SupportedPlatforms::DESKTOP,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    }

    // Whether Oz should add attribution (co-author line) to commit messages and PRs.
    agent_attribution_enabled: AgentAttributionEnabled {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.other.agent_attribution_enabled",
        description: "Whether the Warp Agent adds an attribution co-author line to commit messages and pull requests it creates.",
    }


    // Not a user-visible setting - it tracks which one-time feature-intro popups the
    // user has already seen, keyed by the feature-intro id (see `FEATURE_INTROS`).
    //
    // We model it as a globally-synced setting (not respecting the user's sync setting)
    // so each feature is announced at most once per user, regardless of how many devices
    // they use. A feature is considered seen when its id is present and mapped to `true`.
    seen_feature_intro_ids: SeenFeatureIntroIds {
        type: HashMap<String, bool>,
        default: HashMap::default(),
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    }
]);

impl AISettings {
    pub fn register_and_subscribe_to_events(app: &mut AppContext) {
        Self::register(app);
        CompiledCommandsForCodingAgentToolbar::register(app);
    }

    pub fn is_any_ai_enabled(&self) -> bool {
        *self.is_any_ai_enabled
    }

    /// Returns whether conversation history is available for the current
    /// account and AI state.
    ///
    /// The stored `show_conversation_history` preference is kept separately so
    /// an onboarding choice can take effect automatically after signup and AI
    /// enablement without asking the user to toggle the setting again.
    pub fn is_conversation_history_available(&self) -> bool {
        self.is_any_ai_enabled()
    }

    /// Returns whether conversation history should currently appear in the
    /// tools panel.
    pub fn is_conversation_history_enabled(&self) -> bool {
        *self.show_conversation_history && self.is_conversation_history_available()
    }

    pub fn default_session_mode(&self) -> DefaultSessionMode {
        let mode = *self.default_session_mode_internal.value();
        match mode {
            // Terminal and TabConfig don't require AI.
            DefaultSessionMode::Terminal | DefaultSessionMode::TabConfig => mode,
            // Agent requires AI to be enabled.
            DefaultSessionMode::Agent => {
                if self.is_any_ai_enabled() {
                    mode
                } else {
                    DefaultSessionMode::Terminal
                }
            }
            // Cloud agent tabs can no longer be created; a persisted value degrades to Terminal.
            DefaultSessionMode::CloudAgent => DefaultSessionMode::Terminal,
            // DockerSandbox is gated on its feature flag; fall back to Terminal
            // when disabled so a stale stored value doesn't wedge the user.
            DefaultSessionMode::DockerSandbox => {
                if FeatureFlag::LocalDockerSandbox.is_enabled() {
                    mode
                } else {
                    DefaultSessionMode::Terminal
                }
            }
        }
    }

    /// Returns the stored default tab config path (only meaningful when mode is `TabConfig`).
    pub fn default_tab_config_path(&self) -> &str {
        &self.default_tab_config_path
    }

    /// Looks up the `TabConfig` matching the stored `default_tab_config_path`.
    /// Returns `None` if the path is empty or no loaded config matches.
    pub fn resolved_default_tab_config(
        &self,
        app: &AppContext,
    ) -> Option<crate::tab_configs::TabConfig> {
        let path_str = self.default_tab_config_path.as_str();
        if path_str.is_empty() {
            return None;
        }
        let path = std::path::Path::new(path_str);
        crate::user_config::WarpConfig::as_ref(app)
            .tab_configs()
            .iter()
            .find(|config| config.source_path.as_deref().is_some_and(|p| p == path))
            .cloned()
    }

    pub fn is_active_ai_enabled(&self, app: &warpui::AppContext) -> bool {
        self.is_any_ai_enabled()
            && *self.is_active_ai_enabled_internal
            && AppExecutionMode::as_ref(app).allows_active_ai()
    }

    pub fn is_prompt_suggestions_enabled(&self, app: &warpui::AppContext) -> bool {
        self.is_active_ai_enabled(app) && *self.prompt_suggestions_enabled_internal
    }

    pub fn is_rule_suggestions_enabled(&self, app: &warpui::AppContext) -> bool {
        self.is_active_ai_enabled(app) && *self.rule_suggestions_enabled_internal
    }

    pub fn is_code_suggestions_enabled(&self, app: &warpui::AppContext) -> bool {
        self.is_active_ai_enabled(app) && *self.code_suggestions_enabled_internal
    }

    pub fn is_git_operations_autogen_enabled(&self, app: &warpui::AppContext) -> bool {
        self.is_active_ai_enabled(app) && *self.git_operations_autogen_enabled_internal
    }

    pub fn is_intelligent_autosuggestions_enabled(&self, app: &warpui::AppContext) -> bool {
        self.is_active_ai_enabled(app) && *self.intelligent_autosuggestions_enabled_internal
    }

    /// Returns `true` if input autodetection is enabled.
    ///
    /// If `FeatureFlag::AgentView` is enabled, this specifically gates NLD enablement in the agent
    /// view only.
    pub fn is_ai_autodetection_enabled(&self) -> bool {
        self.is_any_ai_enabled() && *self.ai_autodetection_enabled_internal
    }

    /// Returns `true` if NLD is enabled in the terminal.
    ///
    /// This is only used when `FeatureFlag::AgentView` is enabled.
    /// If the user has not explicitly set this setting, it defaults to the value of
    /// `ai_autodetection_enabled_internal`.
    pub fn is_nld_in_terminal_enabled(&self) -> bool {
        self.is_any_ai_enabled() && *self.nld_in_terminal_enabled_internal
    }

    pub fn is_memory_enabled(&self) -> bool {
        self.is_any_ai_enabled() && *self.memory_enabled
    }

    pub fn is_file_based_mcp_enabled(&self) -> bool {
        if !FeatureFlag::FileBasedMcp.is_enabled() || !self.is_any_ai_enabled() {
            return false;
        }
        // NOTE: we intentionally do not force-enable this in Cloud Mode. Previously
        // we auto-spawned file-based MCPs in autonomous execution, but that bypassed
        // the user's explicit opt-in and let any MCP config checked into a repo run
        // arbitrary commands as part of a cloud agent run. Respecting the toggle
        // closes that attack surface; cloud agents that need project-scoped MCP
        // servers should surface an explicit, auditable opt-in. A more robust
        // solution (e.g. per-environment allowlisting, signed configs) should be
        // explored in the future.
        *self.file_based_mcp_enabled
    }

    pub fn is_command_denylist_editable(&self) -> bool {
        self.is_any_ai_enabled()
    }

    pub fn is_command_allowlist_editable(&self) -> bool {
        self.is_any_ai_enabled()
    }

    pub fn is_directory_allowlist_editable(&self) -> bool {
        self.is_any_ai_enabled()
    }

    pub fn is_execute_commands_permissions_editable(&self) -> bool {
        self.is_any_ai_enabled()
    }

    pub fn is_write_to_pty_permissions_editable(&self) -> bool {
        self.is_any_ai_enabled()
    }

    pub fn is_computer_use_permissions_editable(&self) -> bool {
        self.is_any_ai_enabled()
    }

    pub fn is_read_files_permissions_editable(&self) -> bool {
        self.is_any_ai_enabled()
    }

    pub fn is_code_diffs_permissions_editable(&self) -> bool {
        self.is_any_ai_enabled()
    }

    pub fn is_ask_user_question_permissions_editable(&self) -> bool {
        self.is_any_ai_enabled()
    }

    pub fn is_mcp_permission_editable(&self) -> bool {
        // TODO: Allow workspace overrides on MCP permissions.
        self.is_any_ai_enabled()
    }

    pub fn show_code_suggestion_speedbump(&self) -> bool {
        self.is_any_ai_enabled() && *self.show_code_suggestion_speedbump
    }

    pub fn add_cli_agent_footer_enabled_command(
        &mut self,
        command: &str,
        ctx: &mut ModelContext<Self>,
    ) {
        let command = command.trim();
        if command.is_empty() {
            return;
        }
        if self
            .cli_agent_footer_enabled_commands
            .value()
            .contains_key(command)
        {
            return;
        }

        let mut map = self.cli_agent_footer_enabled_commands.value().0.clone();
        map.insert(command.to_string(), String::new());
        report_if_error!(
            self.cli_agent_footer_enabled_commands
                .set_value(ToolbarCommandMap::new(map), ctx)
        );
    }

    pub fn remove_cli_agent_footer_enabled_command(
        &mut self,
        command: &str,
        ctx: &mut ModelContext<Self>,
    ) {
        let command = command.trim();
        let mut map = self.cli_agent_footer_enabled_commands.value().0.clone();
        map.shift_remove(command);
        report_if_error!(
            self.cli_agent_footer_enabled_commands
                .set_value(ToolbarCommandMap::new(map), ctx)
        );
    }

    pub fn set_cli_agent_for_command(
        &mut self,
        pattern: &str,
        agent: Option<CLIAgent>,
        ctx: &mut ModelContext<Self>,
    ) {
        let mut map = self.cli_agent_footer_enabled_commands.value().0.clone();
        if !map.contains_key(pattern) {
            return;
        }
        let value = agent.map(|a| a.to_serialized_name()).unwrap_or_default();
        map.insert(pattern.to_string(), value);
        report_if_error!(
            self.cli_agent_footer_enabled_commands
                .set_value(ToolbarCommandMap::new(map), ctx)
        );
    }

    /// Whether the feature-intro popover with the given id key has been seen.
    pub fn is_feature_intro_seen(&self, key: &str) -> bool {
        self.seen_feature_intro_ids
            .get(key)
            .copied()
            .unwrap_or(false)
    }

    /// Records that the feature-intro popover with the given id key has been seen,
    /// so it is never shown again. No-op if already recorded.
    pub fn mark_feature_intro_seen(&mut self, key: &str, ctx: &mut ModelContext<Self>) {
        if self.is_feature_intro_seen(key) {
            return;
        }
        let mut map = self.seen_feature_intro_ids.clone();
        map.insert(key.to_owned(), true);
        report_if_error!(self.seen_feature_intro_ids.set_value(map, ctx));
    }
}

/// Singleton model that caches compiled regexes for the `cli_agent_footer_enabled_commands`
/// setting. Each entry pairs a compiled regex with the CLI agent it maps to.
pub struct CompiledCommandsForCodingAgentToolbar {
    regexes: Vec<(Regex, CLIAgent)>,
}

impl CompiledCommandsForCodingAgentToolbar {
    fn parse(app: &AppContext) -> Vec<(Regex, CLIAgent)> {
        AISettings::as_ref(app)
            .cli_agent_footer_enabled_commands
            .value()
            .iter()
            .filter_map(|(pattern, agent_name)| {
                let regex = Regex::new(pattern).ok()?;
                let agent = CLIAgent::from_serialized_name(agent_name);
                Some((regex, agent))
            })
            .collect()
    }

    fn register(app: &mut AppContext) {
        let handle = app.add_singleton_model(|ctx| Self {
            regexes: Self::parse(ctx),
        });
        let ai_settings = AISettings::handle(app);
        app.subscribe_to_model(&ai_settings, move |_, event, ctx| {
            if matches!(
                event,
                AISettingsChangedEvent::CLIAgentToolbarEnabledCommands
            ) {
                let regexes = Self::parse(ctx);
                handle.update(ctx, |me, _| {
                    me.regexes = regexes;
                });
            }
        });
    }

    /// Returns the CLI agent assigned to the first matching pattern, or `None`
    /// if no pattern matches the command.
    pub fn matched_agent(app: &AppContext, command: &str) -> Option<CLIAgent> {
        Self::as_ref(app)
            .regexes
            .iter()
            .find(|(regex, _)| regex.is_match(command))
            .map(|(_, agent)| *agent)
    }
}

impl Entity for CompiledCommandsForCodingAgentToolbar {
    type Event = ();
}

impl SingletonEntity for CompiledCommandsForCodingAgentToolbar {}

#[cfg(test)]
#[path = "ai_tests.rs"]
mod tests;
