//! The MCP tools that the user's connected servers offer to the model.
//!
//! The client sends the connected servers, with their tools and resources, in
//! `Request::mcp_context`. Each server tool becomes one model tool, so the model calls it the
//! same way it calls a built-in one, and the reply goes back as a `CallMCPTool` call that the
//! client already knows how to run.

use serde_json::{Map, Number, Value, json};
use warp_multi_agent_api as api;
use warp_multi_agent_api::message::tool_call;

use crate::tools::{ToolSchema, string_field};

/// The one tool that reads a resource, whichever server holds it.
pub const READ_RESOURCE_TOOL: &str = "read_mcp_resource";

/// The longest tool name that a provider accepts.
const MAX_NAME_LEN: usize = 64;

/// How many resources the prompt lists. A server with thousands of them must not fill the context.
const MAX_PROMPT_RESOURCES: usize = 50;

/// The MCP servers that a request offers, ready to hand to the model.
#[derive(Debug, Default)]
pub struct McpCatalog {
    servers: Vec<McpServer>,
    tools: Vec<McpTool>,
    resources: Vec<McpResource>,
}

#[derive(Debug)]
struct McpServer {
    name: String,
    description: String,
}

#[derive(Debug)]
struct McpTool {
    wire_name: String,
    server_id: String,
    server_name: String,
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug)]
struct McpResource {
    server_id: String,
    server_name: String,
    uri: String,
    name: String,
    description: String,
}

impl McpCatalog {
    /// Reads the servers out of the request. A request from a client that predates server
    /// grouping carries flat tool and resource lists, which become one unnamed server.
    #[allow(deprecated)]
    pub fn from_request(request: &api::Request) -> Self {
        let Some(context) = request.mcp_context.as_ref() else {
            return Self::default();
        };

        let mut catalog = Self::default();
        for server in &context.servers {
            catalog.servers.push(McpServer {
                name: server.name.clone(),
                description: server.description.clone(),
            });
            for tool in &server.tools {
                catalog.push_tool(&server.id, &server.name, tool);
            }
            for resource in &server.resources {
                catalog.push_resource(&server.id, &server.name, resource);
            }
        }
        for tool in &context.tools {
            catalog.push_tool("", "", tool);
        }
        for resource in &context.resources {
            catalog.push_resource("", "", resource);
        }
        catalog
    }

    fn push_tool(
        &mut self,
        server_id: &str,
        server_name: &str,
        tool: &api::request::mcp_context::McpTool,
    ) {
        let wire_name = wire_name(server_id, &tool.name);
        if self.tools.iter().any(|known| known.wire_name == wire_name) {
            return;
        }
        self.tools.push(McpTool {
            wire_name,
            server_id: server_id.to_string(),
            server_name: server_name.to_string(),
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool
                .input_schema
                .as_ref()
                .map(struct_to_json)
                .unwrap_or_else(|| json!({})),
        });
    }

    fn push_resource(
        &mut self,
        server_id: &str,
        server_name: &str,
        resource: &api::request::mcp_context::McpResource,
    ) {
        self.resources.push(McpResource {
            server_id: server_id.to_string(),
            server_name: server_name.to_string(),
            uri: resource.uri.clone(),
            name: resource.name.clone(),
            description: resource.description.clone(),
        });
    }

    /// The tools to offer, in the order the servers listed them.
    ///
    /// `client_supported` is `Settings::supported_tools`, and an empty list means the client
    /// accepts anything.
    pub fn schemas(&self, client_supported: &[i32]) -> Vec<ToolSchema> {
        let accepts = |tool: api::ToolType| {
            client_supported.is_empty() || client_supported.contains(&(tool as i32))
        };

        let mut schemas = Vec::new();
        if accepts(api::ToolType::CallMcpTool) {
            schemas.extend(self.tools.iter().map(|tool| {
                let description = if tool.server_name.is_empty() {
                    tool.description.clone()
                } else {
                    format!("[MCP server: {}] {}", tool.server_name, tool.description)
                };
                ToolSchema {
                    name: tool.wire_name.clone().into(),
                    description: description.into(),
                    input_schema: object_schema(&tool.input_schema),
                }
            }));
        }
        if accepts(api::ToolType::ReadMcpResource) && !self.resources.is_empty() {
            schemas.push(ToolSchema {
                name: READ_RESOURCE_TOOL.into(),
                description: "Read a resource that a connected MCP server exposes. Give the exact \
                    `uri` from the list of MCP resources in the system prompt."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "uri": {
                            "type": "string",
                            "description": "The resource URI.",
                        },
                    },
                    "required": ["uri"],
                }),
            });
        }
        schemas
    }

    /// Turns the model's call of an MCP tool into the proto tool call that the client runs.
    ///
    /// Returns `None` for a name that was not offered.
    pub fn to_proto(
        &self,
        tool_call_id: &str,
        name: &str,
        arguments: &Value,
    ) -> Option<api::message::ToolCall> {
        let tool = if name == READ_RESOURCE_TOOL {
            let uri = string_field(arguments, "uri")?;
            let server_id = self
                .resources
                .iter()
                .find(|resource| resource.uri == uri)
                .map(|resource| resource.server_id.clone())
                .unwrap_or_default();
            tool_call::Tool::ReadMcpResource(tool_call::ReadMcpResource { uri, server_id })
        } else {
            let known = self.tools.iter().find(|tool| tool.wire_name == name)?;
            tool_call::Tool::CallMcpTool(tool_call::CallMcpTool {
                name: known.name.clone(),
                args: Some(json_to_struct(arguments)),
                server_id: known.server_id.clone(),
            })
        };

        Some(api::message::ToolCall {
            tool_call_id: tool_call_id.to_string(),
            tool: Some(tool),
        })
    }

    /// The part of the system prompt that tells the model what the servers are for. Empty when
    /// no server is connected.
    pub fn prompt_section(&self) -> String {
        let named_servers = self
            .servers
            .iter()
            .filter(|server| !server.name.is_empty())
            .collect::<Vec<_>>();
        if named_servers.is_empty() && self.resources.is_empty() && self.tools.is_empty() {
            return String::new();
        }

        let mut section = String::from(
            "\n\n# MCP servers\n\nThe user connected MCP servers. Each of their tools is in the \
             tool list, with the server's name at the start of its description. Prefer them to \
             a guess when they cover the question.\n",
        );
        for server in named_servers {
            if server.description.is_empty() {
                section.push_str(&format!("\n- {}", server.name));
            } else {
                section.push_str(&format!("\n- {}: {}", server.name, server.description));
            }
        }

        if !self.resources.is_empty() {
            section.push_str(&format!(
                "\n\nResources, to read with `{READ_RESOURCE_TOOL}`:\n"
            ));
            for resource in self.resources.iter().take(MAX_PROMPT_RESOURCES) {
                section.push_str(&format!("\n- `{}`", resource.uri));
                if !resource.name.is_empty() {
                    section.push_str(&format!(" {}", resource.name));
                }
                if !resource.server_name.is_empty() {
                    section.push_str(&format!(" ({})", resource.server_name));
                }
                if !resource.description.is_empty() {
                    section.push_str(&format!(": {}", resource.description));
                }
            }
            let hidden = self.resources.len().saturating_sub(MAX_PROMPT_RESOURCES);
            if hidden > 0 {
                section.push_str(&format!("\n- ... and {hidden} more"));
            }
        }
        section
    }
}

/// The name that the model sees for a server's tool.
///
/// It is a pure function of the server and the tool, so a tool call that is already in the
/// conversation gets the same name it had when it was made, with no catalog to look it up in.
/// The server id is cut to its first eight characters to leave room under the provider's limit.
pub fn wire_name(server_id: &str, tool: &str) -> String {
    let short = server_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(8)
        .collect::<String>();
    let tool = tool
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let mut name = format!("mcp_{short}_{tool}");
    name.truncate(MAX_NAME_LEN);
    name
}

/// Makes the schema a valid tool input: a JSON Schema object at the root, which some servers
/// leave out.
fn object_schema(schema: &Value) -> Value {
    let mut schema = match schema {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    };
    schema
        .entry("type")
        .or_insert_with(|| Value::String("object".to_string()));
    if schema.get("type").and_then(Value::as_str) == Some("object") {
        schema
            .entry("properties")
            .or_insert_with(|| Value::Object(Map::new()));
    }
    Value::Object(schema)
}

pub fn struct_to_json(value: &prost_types::Struct) -> Value {
    Value::Object(
        value
            .fields
            .iter()
            .map(|(key, field)| (key.clone(), value_to_json(field)))
            .collect(),
    )
}

fn value_to_json(value: &prost_types::Value) -> Value {
    use prost_types::value::Kind;

    match value.kind.as_ref() {
        None | Some(Kind::NullValue(_)) => Value::Null,
        Some(Kind::NumberValue(number)) => {
            // A whole number reads as `3`, not `3.0`, so the model sees what the server wrote.
            if number.fract() == 0.0 && number.abs() < 9e15 {
                Value::Number(Number::from(*number as i64))
            } else {
                Number::from_f64(*number).map_or(Value::Null, Value::Number)
            }
        }
        Some(Kind::StringValue(text)) => Value::String(text.clone()),
        Some(Kind::BoolValue(flag)) => Value::Bool(*flag),
        Some(Kind::StructValue(inner)) => struct_to_json(inner),
        Some(Kind::ListValue(list)) => {
            Value::Array(list.values.iter().map(value_to_json).collect())
        }
    }
}

/// The arguments of an MCP tool call are named values, so anything but an object becomes an
/// empty one.
pub fn json_to_struct(value: &Value) -> prost_types::Struct {
    let fields = value
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(key, field)| (key.clone(), json_to_value(field)))
                .collect()
        })
        .unwrap_or_default();
    prost_types::Struct { fields }
}

fn json_to_value(value: &Value) -> prost_types::Value {
    use prost_types::value::Kind;

    let kind = match value {
        Value::Null => Kind::NullValue(0),
        Value::Bool(flag) => Kind::BoolValue(*flag),
        Value::Number(number) => Kind::NumberValue(number.as_f64().unwrap_or_default()),
        Value::String(text) => Kind::StringValue(text.clone()),
        Value::Array(items) => Kind::ListValue(prost_types::ListValue {
            values: items.iter().map(json_to_value).collect(),
        }),
        Value::Object(_) => Kind::StructValue(json_to_struct(value)),
    };
    prost_types::Value { kind: Some(kind) }
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
