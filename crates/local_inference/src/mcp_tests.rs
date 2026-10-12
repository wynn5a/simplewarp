use serde_json::json;
use warp_multi_agent_api as api;
use warp_multi_agent_api::message::tool_call;

use super::*;

const SERVER_ID: &str = "3f9c2a10-1b2c-4d5e-8f90-123456789abc";

fn mcp_tool(name: &str, description: &str) -> api::request::mcp_context::McpTool {
    api::request::mcp_context::McpTool {
        name: name.to_string(),
        description: description.to_string(),
        input_schema: Some(json_to_struct(&json!({
            "type": "object",
            "properties": { "query": { "type": "string" } },
            "required": ["query"],
        }))),
    }
}

fn request_with_server() -> api::Request {
    api::Request {
        mcp_context: Some(api::request::McpContext {
            servers: vec![api::request::mcp_context::McpServer {
                id: SERVER_ID.to_string(),
                name: "Linear".to_string(),
                description: "Issue tracker".to_string(),
                tools: vec![mcp_tool("search issues", "Find issues.")],
                resources: vec![api::request::mcp_context::McpResource {
                    uri: "linear://team/core".to_string(),
                    name: "Core team".to_string(),
                    description: "The core team".to_string(),
                    mime_type: "text/plain".to_string(),
                }],
            }],
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn a_wire_name_is_valid_for_a_provider() {
    let name = wire_name(SERVER_ID, "search issues/v2");
    assert_eq!(name, "mcp_3f9c2a10_search_issues_v2");

    let long = wire_name(SERVER_ID, &"x".repeat(200));
    assert_eq!(long.len(), 64);
    assert!(
        long.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    );
}

#[test]
fn servers_with_the_same_tool_name_get_different_wire_names() {
    assert_ne!(
        wire_name("aaaaaaaa-0000", "search"),
        wire_name("bbbbbbbb-0000", "search")
    );
}

#[test]
fn a_request_without_servers_offers_nothing() {
    let catalog = McpCatalog::from_request(&api::Request::default());
    assert!(catalog.schemas(&[]).is_empty());
    assert!(catalog.prompt_section().is_empty());
}

#[test]
fn each_server_tool_and_the_resource_reader_are_offered() {
    let catalog = McpCatalog::from_request(&request_with_server());
    let schemas = catalog.schemas(&[]);

    assert_eq!(schemas.len(), 2);
    assert_eq!(schemas[0].name, "mcp_3f9c2a10_search_issues");
    assert!(schemas[0].description.starts_with("[MCP server: Linear]"));
    assert_eq!(schemas[0].input_schema["required"], json!(["query"]));
    assert_eq!(schemas[1].name, READ_RESOURCE_TOOL);
}

#[test]
fn the_client_support_list_gates_mcp_tools() {
    let catalog = McpCatalog::from_request(&request_with_server());

    let only_shell = [api::ToolType::RunShellCommand as i32];
    assert!(catalog.schemas(&only_shell).is_empty());

    let only_call = [api::ToolType::CallMcpTool as i32];
    let schemas = catalog.schemas(&only_call);
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].name, "mcp_3f9c2a10_search_issues");
}

#[test]
fn a_tool_without_a_schema_still_gets_a_valid_one() {
    let mut request = request_with_server();
    request.mcp_context.as_mut().unwrap().servers[0].tools[0].input_schema = None;

    let schemas = McpCatalog::from_request(&request).schemas(&[]);
    assert_eq!(
        schemas[0].input_schema,
        json!({ "type": "object", "properties": {} })
    );
}

#[test]
fn a_model_call_becomes_a_call_to_the_server_tool() {
    let catalog = McpCatalog::from_request(&request_with_server());
    let call = catalog
        .to_proto(
            "call-1",
            "mcp_3f9c2a10_search_issues",
            &json!({ "query": "bug", "limit": 3 }),
        )
        .expect("an offered tool");

    assert_eq!(call.tool_call_id, "call-1");
    let Some(tool_call::Tool::CallMcpTool(mcp)) = call.tool else {
        panic!("expected a CallMcpTool");
    };
    assert_eq!(mcp.name, "search issues");
    assert_eq!(mcp.server_id, SERVER_ID);
    assert_eq!(
        struct_to_json(&mcp.args.expect("args")),
        json!({ "query": "bug", "limit": 3 })
    );
}

#[test]
fn a_tool_that_was_not_offered_is_refused() {
    let catalog = McpCatalog::from_request(&request_with_server());
    assert!(
        catalog
            .to_proto("c", "mcp_deadbeef_other", &json!({}))
            .is_none()
    );
}

#[test]
fn reading_a_resource_names_the_server_that_holds_it() {
    let catalog = McpCatalog::from_request(&request_with_server());
    let call = catalog
        .to_proto(
            "call-2",
            READ_RESOURCE_TOOL,
            &json!({ "uri": "linear://team/core" }),
        )
        .expect("the reader");

    let Some(tool_call::Tool::ReadMcpResource(read)) = call.tool else {
        panic!("expected a ReadMcpResource");
    };
    assert_eq!(read.uri, "linear://team/core");
    assert_eq!(read.server_id, SERVER_ID);

    assert!(
        catalog
            .to_proto("call-3", READ_RESOURCE_TOOL, &json!({}))
            .is_none()
    );
}

#[test]
#[allow(deprecated)]
fn flat_lists_from_an_older_client_become_one_unnamed_server() {
    let request = api::Request {
        mcp_context: Some(api::request::McpContext {
            tools: vec![mcp_tool("ping", "Ping.")],
            ..Default::default()
        }),
        ..Default::default()
    };
    let catalog = McpCatalog::from_request(&request);

    let schemas = catalog.schemas(&[]);
    assert_eq!(schemas[0].name, "mcp__ping");
    assert_eq!(schemas[0].description, "Ping.");

    let call = catalog
        .to_proto("c", "mcp__ping", &json!({}))
        .expect("offered");
    let Some(tool_call::Tool::CallMcpTool(mcp)) = call.tool else {
        panic!("expected a CallMcpTool");
    };
    assert!(mcp.server_id.is_empty());
}

#[test]
fn the_prompt_lists_servers_and_resources() {
    let section = McpCatalog::from_request(&request_with_server()).prompt_section();

    assert!(section.contains("- Linear: Issue tracker"));
    assert!(section.contains("`linear://team/core` Core team (Linear): The core team"));
    assert!(section.contains(READ_RESOURCE_TOOL));
}

#[test]
fn a_long_resource_list_is_cut() {
    let mut request = request_with_server();
    request.mcp_context.as_mut().unwrap().servers[0].resources = (0..60)
        .map(|index| api::request::mcp_context::McpResource {
            uri: format!("file://{index}"),
            ..Default::default()
        })
        .collect();

    let section = McpCatalog::from_request(&request).prompt_section();
    assert!(section.contains("file://49"));
    assert!(!section.contains("file://50"));
    assert!(section.contains("and 10 more"));
}

#[test]
fn json_survives_a_trip_through_a_proto_struct() {
    let value = json!({
        "text": "hi",
        "count": 3,
        "ratio": 0.5,
        "flag": true,
        "nothing": null,
        "list": [1, "two", { "three": 3 }],
        "nested": { "a": "b" },
    });
    assert_eq!(struct_to_json(&json_to_struct(&value)), value);
}

#[test]
fn arguments_that_are_not_an_object_become_an_empty_struct() {
    assert!(json_to_struct(&json!("text")).fields.is_empty());
}
