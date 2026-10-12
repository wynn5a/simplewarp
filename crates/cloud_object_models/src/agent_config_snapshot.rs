use serde::{Deserialize, Deserializer, Serialize, Serializer};
use warp_cli::agent::Harness;

/// Runtime configuration snapshot for agent execution.
///
/// This is the merged/resolved config used when spawning or running an agent.
/// It combines settings from config files and CLI args.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AgentConfigSnapshot {
    /// Config name for searchability/traceability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_prompt: Option<String>,
    /// MCP server configuration map (unwrapped; no `mcpServers` wrapper).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<serde_json::Map<String, serde_json::Value>>,
    /// Skill spec to use as the base prompt for the agent.
    /// Format: "skill_name", "repo:skill_name", or "org/repo:skill_name".
    /// The skill is resolved at runtime in the agent environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_spec: Option<String>,
    /// Execution harness for the agent run.
    /// If None, we use Warp's default ("oz").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<HarnessConfig>,
}

/// Configuration for a third-party execution harness.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct HarnessConfig {
    /// The harness type, e.g. [`Harness::Claude`].
    #[serde(
        rename = "type",
        serialize_with = "serialize_harness",
        deserialize_with = "deserialize_harness"
    )]
    pub harness_type: Harness,
    /// The model to use with this harness. None means use the harness default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// Optional reasoning level for harnesses that support it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_level: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HarnessModelConfig {
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_level: Option<String>,
}

impl HarnessConfig {
    pub fn model_config(&self) -> Option<HarnessModelConfig> {
        self.model_id
            .as_ref()
            .filter(|id| !id.is_empty())
            .map(|model_id| HarnessModelConfig {
                model_id: model_id.clone(),
                reasoning_level: self.reasoning_level.clone(),
            })
    }
}

fn serialize_harness<S: Serializer>(harness: &Harness, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(harness.config_name())
}

fn deserialize_harness<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Harness, D::Error> {
    let name = String::deserialize(deserializer)?;
    Ok(Harness::from_config_name(&name).unwrap_or_else(|| {
        log::warn!("Unknown harness config name: {name:?}; treating as Unknown");
        Harness::Unknown
    }))
}

impl AgentConfigSnapshot {
    /// Returns true if this config is empty (no options are set).
    pub fn is_empty(&self) -> bool {
        let Self {
            name,
            environment_id,
            model_id,
            base_prompt,
            mcp_servers,
            skill_spec,
            harness,
        } = self;

        name.is_none()
            && environment_id.is_none()
            && model_id.is_none()
            && base_prompt.is_none()
            && mcp_servers.is_none()
            && skill_spec.is_none()
            && harness.is_none()
    }
}
