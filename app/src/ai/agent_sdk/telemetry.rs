use serde_json::{Value, json};
use strum_macros::{EnumDiscriminants, EnumIter};
use warp_core::telemetry::{EnablementState, TelemetryEvent, TelemetryEventDesc};

use crate::features::FeatureFlag;

#[derive(Debug, EnumDiscriminants)]
#[strum_discriminants(derive(EnumIter))]
pub(super) enum CliTelemetryEvent {
    /// Executing `warp agent run`
    AgentRun {
        gui: bool,
        requested_mcp_servers: usize,
        has_environment: bool,
        /// Optional task ID when running against an ambient agent task.
        task_id: Option<String>,
        /// Which execution harness was selected (e.g. "oz", "claude").
        harness: String,
    },
    /// Executing `warp agent profile list`
    AgentProfileList,
    /// Executing `warp agent list`
    AgentList,
    /// Executing `warp agent get`
    AgentGet,
    /// Executing `warp agent create`
    AgentCreate,
    /// Executing `warp agent update`
    AgentUpdate,
    /// Executing `warp agent delete`
    AgentDelete,
    /// Executing `warp agent skills`
    AgentSkills,
    /// Executing `warp mcp list`
    MCPList,
    /// Executing `warp model list`
    ModelList,
    /// Executing `warp task list`
    TaskList,
    /// Executing `warp task get`
    TaskGet,
    /// Executing `warp run conversation get`
    ConversationGet,
    /// Executing `warp run get <id> --conversation`
    RunConversationGet,
    /// Executing `warp run message watch`
    RunMessageWatch { harness: &'static str },
    /// Executing `warp run message send`
    RunMessageSend { harness: &'static str },
    /// Executing `warp run message list`
    RunMessageList { harness: &'static str },
    /// Executing `warp run message read`
    RunMessageRead { harness: &'static str },
    /// Executing `warp run message mark-delivered`
    RunMessageMarkDelivered { harness: &'static str },
    /// Executing `warp login`
    Login,
    /// Executing `warp logout`
    Logout,
    /// Executing `warp whoami`
    Whoami,
    /// Executing `warp provider setup`
    ProviderSetup,
    /// Executing `warp provider list`
    ProviderList,
    /// Executing `warp artifact upload`
    ArtifactUpload,
    /// Executing `warp artifact get`
    ArtifactGet,
    /// Executing `warp artifact download`
    ArtifactDownload,
    /// Executing `warp api-key list`
    ApiKeyList,
    /// Executing `warp api-key create`
    ApiKeyCreate,
    /// Executing `warp api-key expire`
    ApiKeyExpire,
}

impl TelemetryEvent for CliTelemetryEvent {
    fn name(&self) -> &'static str {
        CliTelemetryEventDiscriminants::from(self).name()
    }

    fn payload(&self) -> Option<Value> {
        match self {
            CliTelemetryEvent::AgentRun {
                gui,
                requested_mcp_servers,
                has_environment,
                task_id,
                harness,
            } => Some(json!({
                "gui": gui,
                "requested_mcp_servers": requested_mcp_servers,
                "has_environment": has_environment,
                "task_id": task_id,
                "harness": harness,
            })),
            CliTelemetryEvent::AgentProfileList => None,
            CliTelemetryEvent::AgentList => None,
            CliTelemetryEvent::AgentGet => None,
            CliTelemetryEvent::AgentCreate => None,
            CliTelemetryEvent::AgentUpdate => None,
            CliTelemetryEvent::AgentDelete => None,
            CliTelemetryEvent::AgentSkills => None,
            CliTelemetryEvent::MCPList => None,
            CliTelemetryEvent::ModelList => None,
            CliTelemetryEvent::TaskList => None,
            CliTelemetryEvent::TaskGet => None,
            CliTelemetryEvent::ConversationGet => None,
            CliTelemetryEvent::RunConversationGet => None,
            CliTelemetryEvent::RunMessageWatch { harness } => Some(json!({ "harness": harness })),
            CliTelemetryEvent::RunMessageSend { harness } => Some(json!({ "harness": harness })),
            CliTelemetryEvent::RunMessageList { harness } => Some(json!({ "harness": harness })),
            CliTelemetryEvent::RunMessageRead { harness } => Some(json!({ "harness": harness })),
            CliTelemetryEvent::RunMessageMarkDelivered { harness } => {
                Some(json!({ "harness": harness }))
            }
            CliTelemetryEvent::Login => None,
            CliTelemetryEvent::Logout => None,
            CliTelemetryEvent::Whoami => None,
            CliTelemetryEvent::ProviderSetup => None,
            CliTelemetryEvent::ProviderList => None,
            CliTelemetryEvent::ArtifactUpload => None,
            CliTelemetryEvent::ArtifactGet => None,
            CliTelemetryEvent::ArtifactDownload => None,
            CliTelemetryEvent::ApiKeyList => None,
            CliTelemetryEvent::ApiKeyCreate => None,
            CliTelemetryEvent::ApiKeyExpire => None,
        }
    }

    fn description(&self) -> &'static str {
        CliTelemetryEventDiscriminants::from(self).description()
    }

    fn enablement_state(&self) -> EnablementState {
        CliTelemetryEventDiscriminants::from(self).enablement_state()
    }

    fn contains_ugc(&self) -> bool {
        false
    }

    fn event_descs() -> impl Iterator<Item = Box<dyn TelemetryEventDesc>> {
        warp_core::telemetry::enum_events::<Self>()
    }
}

impl TelemetryEventDesc for CliTelemetryEventDiscriminants {
    fn name(&self) -> &'static str {
        match self {
            CliTelemetryEventDiscriminants::AgentRun => "CLI.Execute.Agent.Run",
            CliTelemetryEventDiscriminants::AgentProfileList => "CLI.Execute.Agent.Profile.List",
            CliTelemetryEventDiscriminants::AgentList => "CLI.Execute.Agent.List",
            CliTelemetryEventDiscriminants::AgentGet => "CLI.Execute.Agent.Get",
            CliTelemetryEventDiscriminants::AgentCreate => "CLI.Execute.Agent.Create",
            CliTelemetryEventDiscriminants::AgentUpdate => "CLI.Execute.Agent.Update",
            CliTelemetryEventDiscriminants::AgentDelete => "CLI.Execute.Agent.Delete",
            CliTelemetryEventDiscriminants::AgentSkills => "CLI.Execute.Agent.Skills",
            CliTelemetryEventDiscriminants::MCPList => "CLI.Execute.MCP.List",
            CliTelemetryEventDiscriminants::ModelList => "CLI.Execute.Model.List",
            CliTelemetryEventDiscriminants::TaskList => "CLI.Execute.Task.List",
            CliTelemetryEventDiscriminants::TaskGet => "CLI.Execute.Task.Get",
            CliTelemetryEventDiscriminants::ConversationGet => "CLI.Execute.Conversation.Get",
            CliTelemetryEventDiscriminants::RunConversationGet => {
                "CLI.Execute.Run.Conversation.Get"
            }
            CliTelemetryEventDiscriminants::RunMessageWatch => "CLI.Execute.Run.Message.Watch",
            CliTelemetryEventDiscriminants::RunMessageSend => "CLI.Execute.Run.Message.Send",
            CliTelemetryEventDiscriminants::RunMessageList => "CLI.Execute.Run.Message.List",
            CliTelemetryEventDiscriminants::RunMessageRead => "CLI.Execute.Run.Message.Read",
            CliTelemetryEventDiscriminants::RunMessageMarkDelivered => {
                "CLI.Execute.Run.Message.MarkDelivered"
            }
            CliTelemetryEventDiscriminants::Login => "CLI.Execute.Login",
            CliTelemetryEventDiscriminants::Logout => "CLI.Execute.Logout",
            CliTelemetryEventDiscriminants::Whoami => "CLI.Execute.Whoami",
            CliTelemetryEventDiscriminants::ProviderSetup => "CLI.Execute.Provider.Setup",
            CliTelemetryEventDiscriminants::ProviderList => "CLI.Execute.Provider.List",
            CliTelemetryEventDiscriminants::ArtifactUpload => "CLI.Execute.Artifact.Upload",
            CliTelemetryEventDiscriminants::ArtifactGet => "CLI.Execute.Artifact.Get",
            CliTelemetryEventDiscriminants::ArtifactDownload => "CLI.Execute.Artifact.Download",
            CliTelemetryEventDiscriminants::ApiKeyList => "CLI.Execute.ApiKey.List",
            CliTelemetryEventDiscriminants::ApiKeyCreate => "CLI.Execute.ApiKey.Create",
            CliTelemetryEventDiscriminants::ApiKeyExpire => "CLI.Execute.ApiKey.Expire",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            CliTelemetryEventDiscriminants::AgentRun => "Ran an agent from the Warp CLI",
            CliTelemetryEventDiscriminants::AgentProfileList => {
                "Listed agent profiles from the Warp CLI"
            }
            CliTelemetryEventDiscriminants::AgentList => "Listed agents from the Warp CLI",
            CliTelemetryEventDiscriminants::AgentGet => "Got agent details from the Warp CLI",
            CliTelemetryEventDiscriminants::AgentCreate => "Created an agent from the Warp CLI",
            CliTelemetryEventDiscriminants::AgentUpdate => "Updated an agent from the Warp CLI",
            CliTelemetryEventDiscriminants::AgentDelete => "Deleted an agent from the Warp CLI",
            CliTelemetryEventDiscriminants::AgentSkills => "Listed agent skills from the Warp CLI",
            CliTelemetryEventDiscriminants::MCPList => "Listed MCP servers from the Warp CLI",
            CliTelemetryEventDiscriminants::ModelList => "Listed models from the Warp CLI",
            CliTelemetryEventDiscriminants::TaskList => "Listed tasks from the Warp CLI",
            CliTelemetryEventDiscriminants::TaskGet => "Got status of task from the Warp CLI",
            CliTelemetryEventDiscriminants::ConversationGet => {
                "Got conversation by ID from the Warp CLI"
            }
            CliTelemetryEventDiscriminants::RunConversationGet => {
                "Got run conversation from the Warp CLI"
            }
            CliTelemetryEventDiscriminants::RunMessageWatch => {
                "Watched run messages from the Warp CLI"
            }
            CliTelemetryEventDiscriminants::RunMessageSend => {
                "Sent a run message from the Warp CLI"
            }
            CliTelemetryEventDiscriminants::RunMessageList => {
                "Listed run messages from the Warp CLI"
            }
            CliTelemetryEventDiscriminants::RunMessageRead => {
                "Read a run message from the Warp CLI"
            }
            CliTelemetryEventDiscriminants::RunMessageMarkDelivered => {
                "Marked a run message as delivered from the Warp CLI"
            }
            CliTelemetryEventDiscriminants::Login => "Logged in via the Warp CLI",
            CliTelemetryEventDiscriminants::Logout => "Logged out via the Warp CLI",
            CliTelemetryEventDiscriminants::Whoami => "Printed current user info from the Warp CLI",
            CliTelemetryEventDiscriminants::ProviderSetup => "Set up a provider via the Warp CLI",
            CliTelemetryEventDiscriminants::ProviderList => "Listed providers from the Warp CLI",
            CliTelemetryEventDiscriminants::ArtifactUpload => {
                "Uploaded an artifact from the Warp CLI"
            }
            CliTelemetryEventDiscriminants::ArtifactGet => {
                "Got artifact metadata from the Warp CLI"
            }
            CliTelemetryEventDiscriminants::ArtifactDownload => {
                "Downloaded an artifact from the Warp CLI"
            }
            CliTelemetryEventDiscriminants::ApiKeyList => "Listed API keys from the Warp CLI",
            CliTelemetryEventDiscriminants::ApiKeyCreate => "Created an API key from the Warp CLI",
            CliTelemetryEventDiscriminants::ApiKeyExpire => "Expired an API key from the Warp CLI",
        }
    }

    fn enablement_state(&self) -> EnablementState {
        match self {
            Self::ArtifactUpload | Self::ArtifactGet | Self::ArtifactDownload => {
                EnablementState::Flag(FeatureFlag::ArtifactCommand)
            }
            Self::ApiKeyList | Self::ApiKeyCreate | Self::ApiKeyExpire => {
                EnablementState::Flag(FeatureFlag::APIKeyManagement)
            }
            Self::RunMessageWatch
            | Self::RunMessageSend
            | Self::RunMessageList
            | Self::RunMessageRead
            | Self::RunMessageMarkDelivered => EnablementState::Always,
            _ => EnablementState::Always,
        }
    }
}

warp_core::register_telemetry_event!(CliTelemetryEvent);
