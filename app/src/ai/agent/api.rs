pub(crate) mod convert_conversation;
mod convert_from;
mod convert_to;
mod r#impl;

use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

pub use ai::agent::convert::ConvertToAPITypeError;
use ai::api_keys::ApiKeyManager;
pub use convert_from::{
    ConversionParams, ConvertAPIMessageToClientOutputMessage, MaybeAIAgentOutputMessage,
    MessageToAIAgentOutputMessageError,
};
use futures_lite::Stream;
pub use r#impl::generate_multi_agent_output;
use mcp::TemplatableMCPServerInfo;
use serde::Serialize;
use warp_core::execution_mode::AppExecutionMode;
use warpui::{AppContext, EntityId, SingletonEntity as _};

use super::{AIAgentInput, MCPContext, MCPServer, RequestMetadata, ServerOutputId};
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::ambient_agents::AmbientAgentTaskId;
use crate::ai::blocklist::{RequestInput, SessionContext};
use crate::ai::llms::LLMId;
use crate::ai::mcp::TemplatableMCPServerManager;
use crate::server::server_api::AIApiError;
use crate::settings::AISettings;
use crate::terminal::safe_mode_settings::get_secret_obfuscation_mode;

/// Unique, server-generated conversation-scoped token to be roundtripped to the API when sending
/// requests that follow-up within a given conversation.
#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerConversationToken(String);

impl ServerConversationToken {
    pub fn new(id: String) -> Self {
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn debugging_payload(&self, request_id: Option<&ServerOutputId>) -> String {
        match request_id {
            Some(request_id) => format!(
                "{{\"request_id\":\"{request_id}\",\"conversation_id\":\"{}\"}}",
                self.as_str()
            ),
            None => format!("{{\"conversation_id\":\"{}\"}}", self.as_str()),
        }
    }
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
impl From<ServerConversationToken> for String {
    fn from(value: ServerConversationToken) -> Self {
        value.0
    }
}

#[derive(Debug, Clone)]
pub struct RequestParams {
    pub input: Vec<AIAgentInput>,
    pub conversation_token: Option<ServerConversationToken>,
    pub forked_from_conversation_token: Option<ServerConversationToken>,
    pub ambient_agent_task_id: Option<AmbientAgentTaskId>,
    pub tasks: Vec<warp_multi_agent_api::Task>,
    pub metadata: Option<RequestMetadata>,
    pub session_context: SessionContext,
    pub model: LLMId,
    pub cli_agent_model: LLMId,
    pub is_memory_enabled: bool,
    /// The user's own rules (the Knowledge page), as (name, content). Empty when rules are off.
    /// The Warp server used to read these from the user's account; the local adapter reads them
    /// from here.
    pub global_rules: Vec<(String, String)>,
    pub mcp_context: Option<MCPContext>,
    should_redact_secrets: bool,

    /// User-provided API keys for AI providers (BYO API Key).
    pub api_keys: Option<warp_multi_agent_api::request::settings::ApiKeys>,
    /// User-provided custom model providers (BYOK endpoints).
    pub custom_model_providers:
        Option<warp_multi_agent_api::request::settings::CustomModelProviders>,
    pub autonomy_level: warp_multi_agent_api::AutonomyLevel,
    pub isolation_level: warp_multi_agent_api::IsolationLevel,
    pub supported_tools_override: Option<Vec<warp_multi_agent_api::ToolType>>,
}

pub type Event = Result<warp_multi_agent_api::ResponseEvent, Arc<AIApiError>>;

pub type ResponseStream = Pin<Box<dyn Stream<Item = Event> + Send + 'static>>;

#[derive(Debug, Clone)]
pub struct ConversationData {
    pub id: AIConversationId,
    pub tasks: Vec<warp_multi_agent_api::Task>,
    pub server_conversation_token: Option<ServerConversationToken>,
    pub forked_from_conversation_token: Option<ServerConversationToken>,
    pub ambient_agent_task_id: Option<AmbientAgentTaskId>,
}

/// The user's active rules, in a stable order so that an unchanged set of rules produces an
/// unchanged prompt.
fn global_rules(app: &AppContext) -> Vec<(String, String)> {
    use crate::ai::facts::{AIFact, AIMemory, CloudAIFactModel};
    use crate::cloud_object::model::generic_string_model::GenericStringObjectId;
    use crate::cloud_object::model::persistence::CloudModel;

    let mut rules: Vec<(String, String)> = CloudModel::as_ref(app)
        .get_all_objects_of_type::<GenericStringObjectId, CloudAIFactModel>()
        .filter(|fact| fact.metadata.trashed_ts.is_none())
        .filter_map(|fact| {
            let AIFact::Memory(AIMemory { name, content, .. }) = fact.model().string_model.clone();
            (!content.trim().is_empty()).then(|| (name.unwrap_or_default(), content))
        })
        .collect();
    rules.sort();
    rules
}

impl RequestParams {
    #[cfg(test)]
    pub fn new_for_test() -> Self {
        Self {
            input: vec![],
            conversation_token: None,
            forked_from_conversation_token: None,
            ambient_agent_task_id: None,
            tasks: vec![],
            metadata: None,
            session_context: SessionContext::new_for_test(),
            model: LLMId::from("test-model"),
            cli_agent_model: LLMId::from("test-model"),
            is_memory_enabled: false,
            global_rules: vec![],
            mcp_context: None,
            should_redact_secrets: false,
            api_keys: None,
            custom_model_providers: None,
            autonomy_level: Default::default(),
            isolation_level: Default::default(),
            supported_tools_override: None,
        }
    }

    pub fn new(
        _terminal_view_id: Option<EntityId>,
        session_context: SessionContext,
        request_input: &RequestInput,
        conversation: ConversationData,
        metadata: Option<RequestMetadata>,
        app: &AppContext,
    ) -> Self {
        let ai_settings = AISettings::as_ref(app);
        let is_memory_enabled = ai_settings.is_memory_enabled();
        let global_rules = if is_memory_enabled {
            global_rules(app)
        } else {
            vec![]
        };

        // Group MCP tools and resources by server.
        let templatable_manager = TemplatableMCPServerManager::as_ref(app);

        let mut active_servers: Vec<&TemplatableMCPServerInfo> = templatable_manager
            .get_active_templatable_servers()
            .values()
            .copied()
            .collect();

        // If file-based MCP servers are enabled, add active servers in scope of
        // the user's current working directory
        if let Some(cwd) = session_context.current_working_directory() {
            active_servers.extend(
                templatable_manager
                    .get_active_file_based_servers(Path::new(cwd), app)
                    .values(),
            );
        }

        // Include any ephemeral MCP servers started via the CLI.
        active_servers.extend(
            templatable_manager
                .get_active_cli_spawned_servers()
                .values(),
        );

        let servers: Vec<MCPServer> = active_servers
            .into_iter()
            .map(|server| MCPServer {
                name: server.name().to_string(),
                description: server.description().unwrap_or_default().to_string(),
                id: server.installation_id().to_string(),
                resources: server.resources().to_vec(),
                tools: server.tools().to_vec(),
            })
            .collect();

        #[allow(deprecated)]
        let mcp_context = (!servers.is_empty()).then(|| MCPContext {
            resources: vec![],
            tools: vec![],
            servers,
        });

        let should_redact_secrets = get_secret_obfuscation_mode(app).should_redact_secret();

        let api_key_manager = ApiKeyManager::as_ref(app);
        let api_keys = api_key_manager.api_keys_for_request();
        let custom_model_providers = api_key_manager.custom_model_providers_for_request();
        let app_execution_mode = AppExecutionMode::as_ref(app);
        let autonomy_level = if app_execution_mode.is_autonomous() {
            warp_multi_agent_api::AutonomyLevel::Unsupervised
        } else {
            warp_multi_agent_api::AutonomyLevel::Supervised
        };

        let isolation_level = if app_execution_mode.is_sandboxed() {
            warp_multi_agent_api::IsolationLevel::Sandbox
        } else {
            warp_multi_agent_api::IsolationLevel::None
        };

        Self {
            input: request_input.all_inputs().cloned().collect(),
            conversation_token: conversation.server_conversation_token,
            forked_from_conversation_token: conversation.forked_from_conversation_token,
            ambient_agent_task_id: conversation.ambient_agent_task_id,
            tasks: conversation.tasks,
            metadata,
            session_context,
            model: request_input.model_id.clone(),
            cli_agent_model: request_input.cli_agent_model_id.clone(),
            is_memory_enabled,
            global_rules,
            mcp_context,
            should_redact_secrets,
            api_keys,
            custom_model_providers,
            autonomy_level,
            isolation_level,
            supported_tools_override: request_input.supported_tools_override.clone(),
        }
    }
}
