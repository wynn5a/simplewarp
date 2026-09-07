use super::*;

#[test]
fn an_empty_client_list_offers_every_tool() {
    let schemas = schemas_for(&[]);
    assert_eq!(schemas.len(), SUPPORTED.len());
}

#[test]
fn only_the_client_supported_tools_are_offered() {
    let schemas = schemas_for(&[api::ToolType::RunShellCommand as i32]);
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].name, "run_shell_command");
}

#[test]
fn every_supported_tool_has_a_wire_name_and_a_schema() {
    for tool in SUPPORTED {
        assert!(wire_name(tool).is_some(), "{tool:?} has no wire name");
        assert!(schema_for(tool).is_some(), "{tool:?} has no schema");
    }
}

#[test]
fn a_poll_with_a_wait_becomes_a_duration_delay() {
    let call = to_proto(
        "call-1",
        "read_shell_command_output",
        &json!({ "command_id": "block-9", "max_wait_seconds": 30 }),
    )
    .expect("a known tool");

    let tool_call::Tool::ReadShellCommandOutput(poll) = call.tool.unwrap() else {
        panic!("expected a read_shell_command_output call");
    };
    assert_eq!(poll.command_id, "block-9");
    let Some(tool_call::read_shell_command_output::Delay::Duration(duration)) = poll.delay else {
        panic!("expected a duration delay, got {:?}", poll.delay);
    };
    assert_eq!(duration.seconds, 30);
}

#[test]
fn wait_for_completion_becomes_the_on_completion_delay() {
    let call = to_proto(
        "call-1",
        "read_shell_command_output",
        &json!({ "command_id": "block-9", "wait_for_completion": true }),
    )
    .expect("a known tool");

    let tool_call::Tool::ReadShellCommandOutput(poll) = call.tool.unwrap() else {
        panic!("expected a read_shell_command_output call");
    };
    assert!(matches!(
        poll.delay,
        Some(tool_call::read_shell_command_output::Delay::OnCompletion(_))
    ));
}

#[test]
fn a_poll_without_a_wait_carries_no_delay() {
    let call = to_proto(
        "call-1",
        "read_shell_command_output",
        &json!({ "command_id": "block-9" }),
    )
    .expect("a known tool");

    let tool_call::Tool::ReadShellCommandOutput(poll) = call.tool.unwrap() else {
        panic!("expected a read_shell_command_output call");
    };
    assert!(poll.delay.is_none());
}

#[test]
fn a_poll_without_a_command_id_is_dropped() {
    let call = to_proto(
        "call-1",
        "read_shell_command_output",
        &json!({ "max_wait_seconds": 5 }),
    );
    assert!(call.is_none());
}

#[test]
fn a_write_defaults_to_line_mode() {
    let call = to_proto(
        "call-1",
        "write_to_long_running_shell_command",
        &json!({ "command_id": "block-9", "input": "y" }),
    )
    .expect("a known tool");

    let tool_call::Tool::WriteToLongRunningShellCommand(write) = call.tool.unwrap() else {
        panic!("expected a write call");
    };
    assert_eq!(write.command_id, "block-9");
    assert_eq!(write.input, b"y");
    let kind = write.mode.and_then(|mode| mode.mode).expect("a mode");
    assert!(matches!(
        kind,
        tool_call::write_to_long_running_shell_command::mode::Mode::Line(_)
    ));
}

#[test]
fn a_write_can_ask_for_block_mode() {
    let call = to_proto(
        "call-1",
        "write_to_long_running_shell_command",
        &json!({ "command_id": "block-9", "input": "a\nb", "mode": "block" }),
    )
    .expect("a known tool");

    let tool_call::Tool::WriteToLongRunningShellCommand(write) = call.tool.unwrap() else {
        panic!("expected a write call");
    };
    let kind = write.mode.and_then(|mode| mode.mode).expect("a mode");
    assert!(matches!(
        kind,
        tool_call::write_to_long_running_shell_command::mode::Mode::Block(_)
    ));
}
