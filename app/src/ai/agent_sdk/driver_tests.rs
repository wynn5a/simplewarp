use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::time::Duration;

use futures::channel::oneshot;
use futures::executor::block_on;
use tempfile::TempDir;
use warp_cli::agent::Harness;
use warp_cli::mcp::MCPSpec;
use warp_cli::{OZ_CLI_ENV, OZ_HARNESS_ENV, OZ_PARENT_RUN_ID_ENV, OZ_RUN_ID_ENV};
use warpui::{App, SingletonEntity as _};

use super::{
    AgentDriver, AgentDriverError, CLIAgentSessionStatus, IdleTimeoutSender,
    LEGACY_OZ_PARENT_LISTENER_MANAGED_EXTERNALLY_ENV, LEGACY_OZ_PARENT_STATE_ROOT_ENV,
    OZ_MESSAGE_LISTENER_MANAGED_EXTERNALLY_ENV, OZ_MESSAGE_LISTENER_STATE_ROOT_ENV,
    SDKConversationOutputStatus, idle_window_for_cli_session_status,
    idle_window_for_terminal_status, terminal_status_log_outcome,
};
use crate::ai::agent::{
    AIAgentOutput, AIAgentOutputMessage, ArtifactCreatedData, CancellationReason, MessageId,
    RenderableAIError,
};
use crate::ai::agent_sdk::task_env_vars;
use crate::ai::ambient_agents::AmbientAgentTaskId;
use crate::ai::mcp::parsing::normalize_mcp_json;
use crate::ai::skills::SkillManager;
use crate::test_util::terminal::{add_window_with_terminal, initialize_app_for_terminal_view};

#[test]
fn test_normalize_single_cli_server() {
    let input = r#"{"command": "npx", "args": ["-y", "mcp-server"]}"#;
    let result = normalize_mcp_json(input).unwrap();

    // Should wrap with a generated name
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let parsed = parsed.as_object().unwrap();
    assert_eq!(parsed.len(), 1);
    let (_name, server) = parsed.iter().next().unwrap();
    assert_eq!(server["command"].as_str().unwrap(), "npx");
}

#[test]
fn test_normalize_single_sse_server() {
    let input = r#"{"url": "http://localhost:3000/mcp", "headers": {"API_KEY": "value"}}"#;
    let result = normalize_mcp_json(input).unwrap();

    // Should wrap with a generated name
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let parsed = parsed.as_object().unwrap();
    assert_eq!(parsed.len(), 1);
    let (_name, server) = parsed.iter().next().unwrap();
    assert_eq!(server["url"].as_str().unwrap(), "http://localhost:3000/mcp");
}

#[test]
fn test_normalize_already_wrapped_server() {
    let input = r#"{"my-server": {"command": "npx", "args": []}}"#;
    let result = normalize_mcp_json(input).unwrap();

    // Should return as-is (no command/url at top level)
    assert_eq!(result, input);
}

#[test]
fn test_normalize_mcp_servers_wrapper() {
    let input = r#"{"mcpServers": {"server-name": {"command": "npx", "args": []}}}"#;
    let result = normalize_mcp_json(input).unwrap();

    // Should return as-is (no command/url at top level)
    assert_eq!(result, input);
}

#[test]
fn test_normalize_servers_wrapper() {
    let input = r#"{"servers": {"server-name": {"url": "http://example.com"}}}"#;
    let result = normalize_mcp_json(input).unwrap();

    // Should return as-is (no command/url at top level)
    assert_eq!(result, input);
}

#[test]
fn test_normalize_invalid_json() {
    let input = "not valid json";
    let result = normalize_mcp_json(input);

    assert!(result.is_err());
}

#[test]
fn test_normalize_cli_server_with_env() {
    let input = r#"{"command": "npx", "args": ["-y", "mcp-server"], "env": {"API_KEY": "secret"}}"#;
    let result = normalize_mcp_json(input).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let parsed = parsed.as_object().unwrap();
    assert_eq!(parsed.len(), 1);
    let (_name, server) = parsed.iter().next().unwrap();
    assert_eq!(server["env"]["API_KEY"].as_str().unwrap(), "secret");
}

#[test]
fn test_normalize_sse_server_with_headers() {
    let input =
        r#"{"url": "http://localhost:5000/mcp", "headers": {"Authorization": "Bearer token"}}"#;
    let result = normalize_mcp_json(input).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let parsed = parsed.as_object().unwrap();
    assert_eq!(parsed.len(), 1);
    let (_name, server) = parsed.iter().next().unwrap();
    assert_eq!(
        server["headers"]["Authorization"].as_str().unwrap(),
        "Bearer token"
    );
}

#[test]
fn managed_resolver_keeps_a_locally_installed_uuid() {
    let uuid = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
    let local_installed_uuids = HashSet::from([uuid]);

    let resolved = block_on(AgentDriver::resolve_mcp_specs_with_local_uuids(
        &[MCPSpec::Uuid(uuid)],
        &local_installed_uuids,
    ))
    .unwrap();

    assert_eq!(resolved.local_uuids, vec![uuid]);
    assert!(resolved.ephemeral_installations.is_empty());
}

#[test]
fn managed_resolver_fails_a_uuid_that_is_not_installed_locally() {
    // Resolving a uuid the machine does not have installed meant asking the server for its
    // managed client config. With no server, the spec names a server that cannot be reached,
    // and run setup must say so rather than start without it.
    let uuid = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

    let error = block_on(AgentDriver::resolve_mcp_specs_with_local_uuids(
        &[MCPSpec::Uuid(uuid)],
        &HashSet::new(),
    ))
    .expect_err("a non-local uuid has no resolver in this build");

    assert!(matches!(
        error,
        AgentDriverError::ManagedMcpResolutionFailed { uid, .. } if uid == uuid
    ));
}

#[test]
fn well_known_spec_is_skipped() {
    // Well-known ids were resolved by the server and were always best-effort: a disconnected
    // integration skipped the server rather than failing the run. With no server, every one
    // of them skips.
    let resolved = block_on(AgentDriver::resolve_mcp_specs_with_local_uuids(
        &[MCPSpec::WellKnown("linear".to_string())],
        &HashSet::new(),
    ))
    .unwrap();

    assert!(resolved.local_uuids.is_empty());
    assert!(resolved.ephemeral_installations.is_empty());
}

#[test]
fn a_skipped_well_known_spec_does_not_drop_the_others() {
    let config_json =
        r#"{"mcpServers":{"GitHub MCP":{"command":"npx","env":{"API_TOKEN":"literal"}}}}"#;

    let resolved = block_on(AgentDriver::resolve_mcp_specs_with_local_uuids(
        &[
            MCPSpec::WellKnown("linear".to_string()),
            MCPSpec::Json(config_json.to_string()),
        ],
        &HashSet::new(),
    ))
    .unwrap();

    assert_eq!(resolved.ephemeral_installations.len(), 1);
}

// ── IdleTimeoutSender tests ──────────────────────────────────────────────────────

#[test]
fn idle_timeout_sender_send_now_delivers_value() {
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    idle_timeout.end_run_now(42);
    assert_eq!(rx.try_recv().unwrap(), Some(42));
}

#[test]
fn idle_timeout_sender_send_now_only_delivers_once() {
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    idle_timeout.end_run_now(1);
    idle_timeout.end_run_now(2);
    assert_eq!(rx.try_recv().unwrap(), Some(1));
}

#[test]
fn idle_timeout_sender_send_after_delivers_after_timeout() {
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    idle_timeout.end_run_after(Duration::from_millis(50), 99);

    // Not yet delivered.
    assert_eq!(rx.try_recv().unwrap(), None);

    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(rx.try_recv().unwrap(), Some(99));
}

#[test]
fn idle_timeout_sender_cancel_prevents_delivery() {
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    idle_timeout.end_run_after(Duration::from_millis(50), 99);
    idle_timeout.cancel_idle_timeout();

    std::thread::sleep(Duration::from_millis(100));
    // Sender was not consumed, so the channel is still open but empty.
    assert_eq!(rx.try_recv().unwrap(), None);
}

#[test]
fn idle_timeout_sender_cancel_then_send_now_delivers() {
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    idle_timeout.end_run_after(Duration::from_millis(50), 1);
    idle_timeout.cancel_idle_timeout();
    idle_timeout.end_run_now(2);

    assert_eq!(rx.try_recv().unwrap(), Some(2));
}

#[test]
fn idle_timeout_sender_later_send_after_supersedes_earlier() {
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    // First timer: long timeout.
    idle_timeout.end_run_after(Duration::from_secs(10), 1);
    // Second timer: short timeout. The first is implicitly cancelled.
    idle_timeout.end_run_after(Duration::from_millis(50), 2);

    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(rx.try_recv().unwrap(), Some(2));
}

#[test]
fn idle_timeout_sender_complete_with_optional_idle_none_sends_immediately() {
    // `complete_with_optional_idle(None, value)` routes to `end_run_now` and
    // delivers `value` synchronously.
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    idle_timeout.complete_with_optional_idle(None, 7);
    assert_eq!(rx.try_recv().unwrap(), Some(7));
}

#[test]
fn idle_timeout_sender_complete_with_optional_idle_some_defers_then_delivers() {
    // `complete_with_optional_idle(Some(d), value)` routes to `end_run_after`
    // and defers delivery by `d`.
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    idle_timeout.complete_with_optional_idle(Some(Duration::from_millis(50)), 7);

    // Not delivered yet.
    assert_eq!(rx.try_recv().unwrap(), None);

    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(rx.try_recv().unwrap(), Some(7));
}

#[test]
fn idle_timeout_sender_complete_with_optional_idle_some_then_cancel_invalidates_timer() {
    // Cross-path cancellation: the Stage 2c skip-initial-turn driver path
    // schedules a deferred `Success` via `complete_with_optional_idle(Some(_), _)`
    // *before* the history subscription is wired up; a later
    // `AppendedExchange` in that subscription closure invalidates the timer
    // via `cancel_idle_timeout()`. The shared `Arc<AtomicUsize>` generation
    // counter is what makes that work across the two logical code paths.
    // This test exercises the same sequence in isolation: schedule via the
    // helper, then cancel via the unrelated `cancel_idle_timeout` entry point,
    // and verify the value is never delivered.
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    idle_timeout.complete_with_optional_idle(Some(Duration::from_millis(50)), 7);
    idle_timeout.cancel_idle_timeout();

    std::thread::sleep(Duration::from_millis(100));
    // Sender was never consumed by the cancelled timer, so the channel is
    // still open but empty.
    assert_eq!(rx.try_recv().unwrap(), None);
}

// ── Terminal-status idle window routing ──────────────────────────────────────────

fn error_status() -> SDKConversationOutputStatus {
    SDKConversationOutputStatus::Error {
        error: RenderableAIError::InternalWarpError,
    }
}

#[test]
fn terminal_error_exits_immediately() {
    let window =
        idle_window_for_terminal_status(&error_status(), Some(Duration::from_secs(45 * 60)));

    assert_eq!(
        window, None,
        "--idle-on-complete must not apply to a terminal error"
    );
}

#[test]
fn non_error_completion_defers_by_idle_on_complete() {
    let cases = [
        ("success", SDKConversationOutputStatus::Success),
        (
            "blocked",
            SDKConversationOutputStatus::Blocked {
                blocked_action: "approve".to_string(),
            },
        ),
        (
            "cancelled",
            SDKConversationOutputStatus::Cancelled {
                reason: CancellationReason::ManuallyCancelled,
            },
        ),
    ];

    for (label, status) in cases {
        let window = idle_window_for_terminal_status(&status, Some(Duration::from_secs(45 * 60)));

        assert_eq!(
            window,
            Some(Duration::from_secs(45 * 60)),
            "unexpected window for {label}"
        );
    }
}

#[test]
fn cli_harness_session_idle_window_follows_idle_on_complete() {
    let idle_on_complete = Some(Duration::from_secs(45 * 60));

    let failed = CLIAgentSessionStatus::Failed {
        error_type: None,
        message: Some("boom".to_string()),
    };
    assert_eq!(
        idle_window_for_cli_session_status(&failed, idle_on_complete),
        None,
        "--idle-on-complete must not apply to a failed CLI session"
    );
    assert_eq!(
        idle_window_for_cli_session_status(&CLIAgentSessionStatus::Success, idle_on_complete),
        idle_on_complete
    );
    assert_eq!(
        idle_window_for_cli_session_status(&CLIAgentSessionStatus::InProgress, idle_on_complete),
        None
    );
    assert_eq!(
        idle_window_for_cli_session_status(&CLIAgentSessionStatus::Cancelled, idle_on_complete),
        idle_on_complete,
        "a Ctrl-C cancellation is a non-error completion, like Success or Blocked"
    );
}

#[test]
fn terminal_status_log_outcome_labels_are_low_cardinality() {
    assert_eq!(
        terminal_status_log_outcome(&SDKConversationOutputStatus::Success),
        "non_error_completion"
    );
    assert_eq!(terminal_status_log_outcome(&error_status()), "error");
}

#[test]
fn task_env_vars_include_parent_run_id_when_present() {
    let task_id: AmbientAgentTaskId = "550e8400-e29b-41d4-a716-446655440000".parse().unwrap();
    let env_vars = task_env_vars(Some(&task_id), Some("parent-run-123"), Harness::Claude);

    assert_eq!(
        env_vars.get(&OsString::from(OZ_RUN_ID_ENV)),
        Some(&OsString::from(task_id.to_string()))
    );
    assert_eq!(
        env_vars.get(&OsString::from(OZ_PARENT_RUN_ID_ENV)),
        Some(&OsString::from("parent-run-123"))
    );
    assert_eq!(
        env_vars.get(&OsString::from(OZ_HARNESS_ENV)),
        Some(&OsString::from("claude"))
    );
    assert_eq!(
        env_vars.get(&OsString::from(OZ_MESSAGE_LISTENER_MANAGED_EXTERNALLY_ENV)),
        Some(&OsString::from("1"))
    );
    assert_eq!(
        env_vars.get(&OsString::from(
            LEGACY_OZ_PARENT_LISTENER_MANAGED_EXTERNALLY_ENV
        )),
        Some(&OsString::from("1"))
    );
    assert!(
        env_vars
            .get(&OsString::from(OZ_CLI_ENV))
            .is_some_and(|value| !value.is_empty())
    );
}

#[test]
fn task_env_vars_omit_parent_run_id_when_absent() {
    let task_id: AmbientAgentTaskId = "550e8400-e29b-41d4-a716-446655440001".parse().unwrap();
    let env_vars = task_env_vars(Some(&task_id), None, Harness::Oz);

    assert_eq!(
        env_vars.get(&OsString::from(OZ_RUN_ID_ENV)),
        Some(&OsString::from(task_id.to_string()))
    );
    assert!(!env_vars.contains_key(&OsString::from(OZ_PARENT_RUN_ID_ENV)));
    assert_eq!(
        env_vars.get(&OsString::from(OZ_HARNESS_ENV)),
        Some(&OsString::from("oz"))
    );
    assert!(!env_vars.contains_key(&OsString::from(OZ_MESSAGE_LISTENER_MANAGED_EXTERNALLY_ENV)));
    assert!(!env_vars.contains_key(&OsString::from(
        LEGACY_OZ_PARENT_LISTENER_MANAGED_EXTERNALLY_ENV
    )));
}

#[test]
fn task_env_vars_enable_external_parent_listener_for_claude_runs_without_parent_run_id() {
    let task_id: AmbientAgentTaskId = "550e8400-e29b-41d4-a716-446655440002".parse().unwrap();
    let env_vars = task_env_vars(Some(&task_id), None, Harness::Claude);
    assert_eq!(
        env_vars.get(&OsString::from(OZ_MESSAGE_LISTENER_MANAGED_EXTERNALLY_ENV)),
        Some(&OsString::from("1"))
    );
    assert_eq!(
        env_vars.get(&OsString::from(
            LEGACY_OZ_PARENT_LISTENER_MANAGED_EXTERNALLY_ENV
        )),
        Some(&OsString::from("1"))
    );
}

#[test]
#[serial_test::serial]
fn task_env_vars_propagate_message_listener_state_root_with_legacy_alias() {
    let task_id: AmbientAgentTaskId = "550e8400-e29b-41d4-a716-446655440003".parse().unwrap();
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe {
        std::env::set_var(
            OZ_MESSAGE_LISTENER_STATE_ROOT_ENV,
            "/tmp/message-listener-root",
        )
    };
    let env_vars = task_env_vars(Some(&task_id), None, Harness::Claude);
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var(OZ_MESSAGE_LISTENER_STATE_ROOT_ENV) };

    assert_eq!(
        env_vars.get(&OsString::from(OZ_MESSAGE_LISTENER_STATE_ROOT_ENV)),
        Some(&OsString::from("/tmp/message-listener-root"))
    );
    assert_eq!(
        env_vars.get(&OsString::from(LEGACY_OZ_PARENT_STATE_ROOT_ENV)),
        Some(&OsString::from("/tmp/message-listener-root"))
    );
}

#[test]
fn task_env_vars_can_use_opencode_harness() {
    let task_id: AmbientAgentTaskId = "550e8400-e29b-41d4-a716-446655440004".parse().unwrap();
    let env_vars = task_env_vars(Some(&task_id), Some("parent-run-456"), Harness::OpenCode);

    assert_eq!(
        env_vars.get(&OsString::from(OZ_HARNESS_ENV)),
        Some(&OsString::from("opencode"))
    );
}

#[test]
fn json_format_output_includes_filename_for_file_artifact_created_event() {
    let output = AIAgentOutput {
        messages: vec![AIAgentOutputMessage::artifact_created(
            MessageId::new("message-1".to_string()),
            ArtifactCreatedData::File {
                artifact_uid: "artifact-uid".to_string(),
                filepath: "outputs/report.txt".to_string(),
                filename: "report.txt".to_string(),
                mime_type: "text/plain".to_string(),
                description: Some("Build output for the latest run".to_string()),
                size_bytes: 42,
            },
        )],
        ..Default::default()
    };

    let mut bytes = Vec::new();
    super::output::json::format_output(&output, &mut bytes).expect("json formatting should work");

    let value: serde_json::Value =
        serde_json::from_slice(&bytes).expect("output should be valid json");

    assert_eq!(value["type"], "artifact_created");
    assert_eq!(value["artifact_type"], "file");
    assert_eq!(value["artifact_uid"], "artifact-uid");
    assert_eq!(value["filepath"], "outputs/report.txt");
    assert_eq!(value["filename"], "report.txt");
    assert_eq!(value["mime_type"], "text/plain");
    assert_eq!(value["description"], "Build output for the latest run");
    assert_eq!(value["size_bytes"], 42);
}

/// Write a minimal SKILL.md at `{skills_dir}/{name}/SKILL.md`.
/// This is the flat layout expected by `WARP_SKILL_DIRS` (no `.agents/skills` wrapper).
fn write_flat_skill(skills_dir: &Path, name: &str) {
    let skill_dir = skills_dir.join(name);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Skill {name}.\n---\n\n# {name}\n"),
    )
    .unwrap();
}

/// Verifies that `load_skills_dirs` reads skills from the `WARP_SKILL_DIRS` environment
/// variable and registers them in the personal (home) bucket so they are always in scope,
/// regardless of the current working directory.
#[test]
#[serial_test::serial]
fn warp_skill_dirs_env_loads_skills_as_home_tier() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let temp = TempDir::new().unwrap();
        let working_dir = dunce::canonicalize(temp.path()).unwrap();

        // Create two separate flat skills directories (no .agents/skills prefix).
        let skills_dir_a = working_dir.join("extra-skills-a");
        let skills_dir_b = working_dir.join("extra-skills-b");
        write_flat_skill(&skills_dir_a, "env-skill-a1");
        write_flat_skill(&skills_dir_a, "env-skill-a2");
        write_flat_skill(&skills_dir_b, "env-skill-b1");

        // Point WARP_SKILL_DIRS at both directories.
        let skills_dirs_value = format!("{},{}", skills_dir_a.display(), skills_dir_b.display());
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("WARP_SKILL_DIRS", &skills_dirs_value) };

        let terminal_view = add_window_with_terminal(&mut app, None);
        let driver_handle = app.add_model(|ctx| {
            let terminal_driver =
                super::terminal::TerminalDriver::create_from_existing_view(terminal_view, ctx);
            AgentDriver::new_for_test(working_dir.clone(), terminal_driver, ctx)
        });

        let (done_tx, done_rx) = futures::channel::oneshot::channel::<()>();
        driver_handle.update(&mut app, |_, ctx| {
            let spawner = ctx.spawner();
            ctx.spawn(
                async move {
                    AgentDriver::load_skills_dirs(&spawner).await;
                    let _ = done_tx.send(());
                },
                |_, _, _| {},
            );
        });
        done_rx.await.expect("loading task should complete");

        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("WARP_SKILL_DIRS") };

        // Skills from WARP_SKILL_DIRS are home-tier, so they appear for any working directory.
        let skill_names = SkillManager::handle(&app).read(&app, |manager: &SkillManager, ctx| {
            manager
                .get_skills_for_working_directory(None, ctx)
                .into_iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>()
        });

        assert!(
            skill_names.contains(&"env-skill-a1".to_string()),
            "'env-skill-a1' from WARP_SKILL_DIRS should be loaded; got: {skill_names:?}"
        );
        assert!(
            skill_names.contains(&"env-skill-a2".to_string()),
            "'env-skill-a2' from WARP_SKILL_DIRS should be loaded; got: {skill_names:?}"
        );
        assert!(
            skill_names.contains(&"env-skill-b1".to_string()),
            "'env-skill-b1' from WARP_SKILL_DIRS should be loaded; got: {skill_names:?}"
        );

        // Verify the skills have Home scope (personal tier).
        let scope_check = SkillManager::handle(&app).read(&app, |manager: &SkillManager, ctx| {
            use ai::skills::SkillScope;
            manager
                .get_skills_for_working_directory(None, ctx)
                .into_iter()
                .filter(|s| s.name.starts_with("env-skill-"))
                .all(|s| s.scope == SkillScope::Home)
        });
        assert!(
            scope_check,
            "all WARP_SKILL_DIRS skills must have SkillScope::Home"
        );
    });
}

/// Verifies that relative `WARP_SKILL_DIRS` entries are resolved against the driver's
/// working directory rather than the process's current working directory.
#[test]
#[serial_test::serial]
fn warp_skill_dirs_env_relative_entries_resolve_against_working_dir() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let temp = TempDir::new().unwrap();
        let working_dir = dunce::canonicalize(temp.path()).unwrap();

        // Create a flat skills directory inside the working dir and reference it by
        // relative path only. No `rel-skills` directory exists under the process cwd,
        // so this only loads if resolution is anchored at the driver's working dir.
        write_flat_skill(&working_dir.join("rel-skills"), "env-skill-rel");

        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("WARP_SKILL_DIRS", "rel-skills") };

        let terminal_view = add_window_with_terminal(&mut app, None);
        let driver_handle = app.add_model(|ctx| {
            let terminal_driver =
                super::terminal::TerminalDriver::create_from_existing_view(terminal_view, ctx);
            AgentDriver::new_for_test(working_dir.clone(), terminal_driver, ctx)
        });

        let (done_tx, done_rx) = futures::channel::oneshot::channel::<()>();
        driver_handle.update(&mut app, |_, ctx| {
            let spawner = ctx.spawner();
            ctx.spawn(
                async move {
                    AgentDriver::load_skills_dirs(&spawner).await;
                    let _ = done_tx.send(());
                },
                |_, _, _| {},
            );
        });
        done_rx.await.expect("loading task should complete");

        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("WARP_SKILL_DIRS") };

        let skill_names = SkillManager::handle(&app).read(&app, |manager: &SkillManager, ctx| {
            manager
                .get_skills_for_working_directory(None, ctx)
                .into_iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>()
        });

        assert!(
            skill_names.contains(&"env-skill-rel".to_string()),
            "'env-skill-rel' should load via a relative WARP_SKILL_DIRS entry resolved against the driver's working dir; got: {skill_names:?}"
        );
    });
}
