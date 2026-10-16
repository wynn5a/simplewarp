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

#[test]
fn a_transfer_carries_its_reason() {
    let call = to_proto(
        "call-1",
        "transfer_control_to_user",
        &json!({ "reason": "Enter your sudo password" }),
    )
    .expect("a known tool");

    let tool_call::Tool::TransferShellCommandControlToUser(transfer) = call.tool.unwrap() else {
        panic!("expected a transfer_control_to_user call");
    };
    assert_eq!(transfer.reason, "Enter your sudo password");
}

fn github_comment() -> Value {
    json!({
        "id": 1001,
        "author": "reviewer",
        "body": "Handle the error here.",
        "updated_at": "2026-10-01T12:00:00Z",
        "html_url": "https://github.com/o/r/pull/1#discussion_r1001",
        "path": "src/main.rs",
        "diff_hunk": "@@ -1,3 +1,4 @@\n fn main() {\n+    run();\n }",
        "start_line": 2,
        "line": 3,
        "side": "LEFT",
    })
}

#[test]
fn review_comments_keep_their_location() {
    let call = to_proto(
        "call-1",
        "insert_review_comments",
        &json!({
            "repo_path": "/repo",
            "base_branch": "main",
            "comments": [github_comment()],
        }),
    )
    .expect("a known tool");

    let tool_call::Tool::InsertReviewComments(insert) = call.tool.unwrap() else {
        panic!("expected an insert_review_comments call");
    };
    assert_eq!(insert.repo_path, "/repo");
    assert_eq!(insert.base_branch, "main");
    let comment = &insert.comments[0];
    assert_eq!(comment.comment_id, "1001");
    assert_eq!(comment.author, "reviewer");
    let location = comment.location.as_ref().expect("a file comment");
    assert_eq!(location.file_path, "src/main.rs");
    let line = location.line.as_ref().expect("a line comment");
    assert_eq!(
        line.range,
        Some(api::FileContentLineRange { start: 2, end: 3 })
    );
    assert_eq!(line.side(), insert_review_comments::CommentSide::Old);
}

#[test]
fn a_comment_without_a_path_is_a_general_comment() {
    let call = to_proto(
        "call-1",
        "insert_review_comments",
        &json!({
            "repo_path": "/repo",
            "comments": [{
                "id": "c1",
                "author": "reviewer",
                "body": "Looks good overall.",
                "updated_at": "2026-10-01T12:00:00Z",
                "in_reply_to_id": "c0",
            }],
        }),
    )
    .expect("a known tool");

    let tool_call::Tool::InsertReviewComments(insert) = call.tool.unwrap() else {
        panic!("expected an insert_review_comments call");
    };
    assert!(insert.comments[0].location.is_none());
    assert_eq!(insert.comments[0].parent_comment_id, "c0");
}

#[test]
fn review_comments_with_no_usable_comment_are_dropped() {
    let call = to_proto(
        "call-1",
        "insert_review_comments",
        &json!({ "repo_path": "/repo", "comments": [{ "body": "no id" }] }),
    );
    assert!(call.is_none());
}
