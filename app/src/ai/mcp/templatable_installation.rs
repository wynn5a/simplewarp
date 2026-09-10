use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use siphasher::sip::SipHasher;
use uuid::Uuid;
use warp_errors::report_error;

use crate::ai::mcp::{TemplatableMCPServer, TemplateVariable};

lazy_static! {
    static ref HASHER: SipHasher = SipHasher::new_with_keys(0, 0);
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum VariableType {
    Text,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VariableValue {
    pub variable_type: VariableType,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplatableMCPServerInstallation {
    uuid: Uuid,
    templatable_mcp_server: TemplatableMCPServer,
    variable_values: HashMap<String, VariableValue>,
}

impl TemplatableMCPServerInstallation {
    pub fn new(
        uuid: Uuid,
        templatable_mcp_server: TemplatableMCPServer,
        variable_values: HashMap<String, VariableValue>,
    ) -> TemplatableMCPServerInstallation {
        TemplatableMCPServerInstallation {
            uuid,
            templatable_mcp_server,
            variable_values,
        }
    }

    /// Returns a consistent hash for the installation based on the MCP server's name, JsonTemplate, and variable values.
    /// Returns None if the variable values cannot be serialized.
    pub fn hash(&self) -> Option<u64> {
        let mut hasher = *HASHER;

        let name = self.templatable_mcp_server.name.as_str();
        let template_json = self.templatable_mcp_server.template.json.as_str();

        // Converts the variable values to a sorted BTreeMap for consistent hashing
        let variable_values: BTreeMap<String, String> = self
            .variable_values
            .iter()
            .map(|(key, value)| (key.clone(), value.value.clone()))
            .collect();
        let variable_values_json = match serde_json::to_string(&variable_values) {
            Ok(json) => json,
            Err(err) => {
                report_error!(
                    anyhow::Error::new(err)
                        .context("Failed to serialize variable values for hashing")
                );
                return None;
            }
        };

        // Hashes the name, template JSON, and variable values
        (name, template_json, variable_values_json).hash(&mut hasher);

        Some(hasher.finish())
    }

    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    pub fn templatable_mcp_server(&self) -> &TemplatableMCPServer {
        &self.templatable_mcp_server
    }

    pub fn template_uuid(&self) -> Uuid {
        self.templatable_mcp_server.uuid
    }

    pub fn template_json(&self) -> &str {
        &self.templatable_mcp_server.template.json
    }

    pub fn template_variables(&self) -> &Vec<TemplateVariable> {
        &self.templatable_mcp_server.template.variables
    }

    pub fn variable_values(&self) -> &HashMap<String, VariableValue> {
        &self.variable_values
    }

    pub fn gallery_uuid(&self) -> Option<Uuid> {
        self.templatable_mcp_server
            .gallery_data
            .as_ref()
            .map(|g| g.gallery_item_id)
    }

    pub fn gallery_version(&self) -> Option<i32> {
        self.templatable_mcp_server
            .gallery_data
            .as_ref()
            .map(|g| g.version)
    }
}
