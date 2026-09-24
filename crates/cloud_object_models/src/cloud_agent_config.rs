use std::collections::HashMap;

use cloud_objects::cloud_object::{GenericCloudObject, GenericStringModel, JsonObjectType};
use cloud_objects::ids::GenericStringObjectId;
use serde::{Deserialize, Serialize};

use crate::{JsonModel, JsonSerializer};

/// A CloudAgentConfig represents a saved agent configuration that can be referenced
/// when running agents via `--agent-id`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct AgentConfig {
    /// Configuration name
    pub name: String,
    /// Base model ID to use for the agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_model_id: Option<String>,
    /// Base prompt to prepend to user prompts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_prompt: Option<String>,
    /// MCP servers configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<HashMap<String, serde_json::Value>>,
}

impl JsonModel for AgentConfig {
    fn json_object_type() -> JsonObjectType {
        JsonObjectType::CloudAgentConfig
    }
}

pub type CloudAgentConfig = GenericCloudObject<GenericStringObjectId, CloudAgentConfigModel>;
pub type CloudAgentConfigModel = GenericStringModel<AgentConfig, JsonSerializer>;
