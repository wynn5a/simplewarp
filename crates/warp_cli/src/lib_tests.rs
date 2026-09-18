use std::ffi::OsString;

use clap::Parser;

use super::*;
use crate::agent::{AgentCommand, Harness, OutputFormat};

#[test]
fn identifies_worker_subcommands() {
    assert!(is_worker_invocation("minidump-server"));
    #[cfg(unix)]
    assert!(is_worker_invocation(&terminal_server_subcommand()));
    #[cfg(feature = "plugin_host")]
    assert!(is_worker_invocation("--plugin-host"));
    assert!(!is_worker_invocation("--prompt"));
}
fn set_env_var(name: &str, value: &str) -> Option<OsString> {
    let previous = std::env::var_os(name);
    // Safety: tests that mutate process environment are marked `serial` so we
    // do not race with other environment readers/writers in this crate.
    unsafe { std::env::set_var(name, value) };
    previous
}

fn restore_env_var(name: &str, previous: Option<OsString>) {
    match previous {
        // Safety: tests that mutate process environment are marked `serial` so
        // we do not race with other environment readers/writers in this crate.
        Some(value) => unsafe { std::env::set_var(name, value) },
        // Safety: tests that mutate process environment are marked `serial` so
        // we do not race with other environment readers/writers in this crate.
        None => unsafe { std::env::remove_var(name) },
    }
}

#[test]
fn agent_run_accepts_model() {
    let args = Args::try_parse_from([
        "warp", "agent", "run", "--prompt", "hello", "--model", "gpt-4o",
    ])
    .unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert_eq!(run_args.model.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn model_list_parses() {
    let args = Args::try_parse_from(["warp", "model", "list"]).unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp model list` command");
    };
    let CliCommand::Model(model_cmd) = boxed_cmd.as_ref() else {
        panic!("Expected `warp model` command");
    };

    assert!(matches!(model_cmd, crate::model::ModelCommand::List));
}

#[test]
fn debug_before_subcommand_parses() {
    // Regression test: `warp --debug <subcommand>` should work.
    // Global flags like --debug must not prevent subcommand detection.
    let args = Args::try_parse_from(["warp", "--debug", "whoami"]).unwrap();

    assert!(args.debug());
    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp whoami` command");
    };
    assert!(matches!(boxed_cmd.as_ref(), CliCommand::Whoami));
}

#[test]
fn multiple_global_flags_before_subcommand_parse() {
    // Both --output-format and --debug before the subcommand should work.
    let args =
        Args::try_parse_from(["warp", "--output-format", "json", "--debug", "whoami"]).unwrap();

    assert_eq!(args.global_options.output_format, OutputFormat::Json);
    assert!(args.debug());
    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp whoami` command");
    };
    assert!(matches!(boxed_cmd.as_ref(), CliCommand::Whoami));
}

#[test]
fn logout_parses() {
    let args = Args::try_parse_from(["warp", "logout"]).unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp logout` command");
    };

    assert!(matches!(boxed_cmd.as_ref(), CliCommand::Logout));
}

#[test]
fn agent_run_accepts_file() {
    let args = Args::try_parse_from([
        "warp",
        "agent",
        "run",
        "--prompt",
        "hello",
        "--file",
        "config.yaml",
    ])
    .unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert_eq!(
        run_args.config_file.file.as_ref().and_then(|p| p.to_str()),
        Some("config.yaml")
    );
}

#[test]
fn agent_run_accepts_idle_on_complete_flag() {
    let args = Args::try_parse_from([
        "warp",
        "agent",
        "run",
        "--prompt",
        "hello",
        "--idle-on-complete",
    ])
    .unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert_eq!(
        run_args.idle_on_complete,
        Some(humantime::Duration::from(std::time::Duration::from_secs(
            45 * 60
        )))
    );
}

#[test]
fn agent_run_accepts_idle_on_complete_duration() {
    let args = Args::try_parse_from([
        "warp",
        "agent",
        "run",
        "--prompt",
        "hello",
        "--idle-on-complete",
        "10m",
    ])
    .unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert_eq!(
        run_args.idle_on_complete,
        Some(humantime::Duration::from(std::time::Duration::from_secs(
            10 * 60
        )))
    );
}

#[test]
fn agent_run_accepts_idle_on_fail_flag() {
    let args = Args::try_parse_from([
        "warp",
        "agent",
        "run",
        "--prompt",
        "hello",
        "--idle-on-fail",
    ])
    .unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert_eq!(
        run_args.idle_on_fail,
        Some(humantime::Duration::from(std::time::Duration::from_secs(
            15 * 60
        )))
    );
}

#[test]
fn agent_run_accepts_idle_on_fail_duration() {
    let args = Args::try_parse_from([
        "warp",
        "agent",
        "run",
        "--prompt",
        "hello",
        "--idle-on-fail",
        "10m",
    ])
    .unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert_eq!(
        run_args.idle_on_fail,
        Some(humantime::Duration::from(std::time::Duration::from_secs(
            10 * 60
        )))
    );
}

#[test]
#[serial_test::serial]
fn agent_run_reads_idle_on_fail_from_env() {
    // Cloud workers deliver the window through the environment rather than the flag, so an
    // older pinned CLI ignores an unknown variable instead of rejecting an unknown argument.
    let previous = set_env_var("OZ_IDLE_ON_FAIL", "20m");

    let parsed = Args::try_parse_from(["warp", "agent", "run", "--prompt", "hello"]);

    restore_env_var("OZ_IDLE_ON_FAIL", previous);

    let args = parsed.expect("OZ_IDLE_ON_FAIL should parse");
    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert_eq!(
        run_args.idle_on_fail,
        Some(humantime::Duration::from(std::time::Duration::from_secs(
            20 * 60
        )))
    );
}

#[test]
#[serial_test::serial]
fn agent_run_idle_on_fail_flag_overrides_env() {
    let previous = set_env_var("OZ_IDLE_ON_FAIL", "20m");

    let parsed = Args::try_parse_from([
        "warp",
        "agent",
        "run",
        "--prompt",
        "hello",
        "--idle-on-fail",
        "3m",
    ]);

    restore_env_var("OZ_IDLE_ON_FAIL", previous);

    let args = parsed.expect("explicit flag should parse");
    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert_eq!(
        run_args.idle_on_fail,
        Some(humantime::Duration::from(std::time::Duration::from_secs(
            3 * 60
        )))
    );
}

#[test]
#[serial_test::serial]
fn agent_run_leaves_idle_on_fail_unset_without_flag_or_env() {
    let previous = std::env::var_os("OZ_IDLE_ON_FAIL");
    restore_env_var("OZ_IDLE_ON_FAIL", None);

    let parsed = Args::try_parse_from(["warp", "agent", "run", "--prompt", "hello"]);

    restore_env_var("OZ_IDLE_ON_FAIL", previous);

    let args = parsed.expect("run without retention should parse");
    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert!(run_args.idle_on_fail.is_none());
}

#[test]
fn agent_run_idle_on_fail_is_independent_of_idle_on_complete() {
    // The success and failure lifecycles are separately configured; neither flag implies
    // the other, so a run can keep its session after a failure without keeping it after
    // a successful completion.
    let args = Args::try_parse_from([
        "warp",
        "agent",
        "run",
        "--prompt",
        "hello",
        "--idle-on-fail",
        "5m",
    ])
    .unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert!(run_args.idle_on_fail.is_some());
    assert!(run_args.idle_on_complete.is_none());
}

#[test]
fn agent_run_accepts_snapshot_flags() {
    let args = Args::try_parse_from([
        "warp",
        "agent",
        "run",
        "--prompt",
        "hello",
        "--no-snapshot",
        "--snapshot-upload-timeout",
        "90s",
        "--snapshot-script-timeout",
        "45s",
    ])
    .unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert!(run_args.snapshot.no_snapshot);
    assert_eq!(
        run_args.snapshot.snapshot_upload_timeout,
        Some(humantime::Duration::from(std::time::Duration::from_secs(
            90
        )))
    );
    assert_eq!(
        run_args.snapshot.snapshot_script_timeout,
        Some(humantime::Duration::from(std::time::Duration::from_secs(
            45
        )))
    );
}
#[test]
fn agent_run_requires_a_prompt() {
    let result = Args::try_parse_from(["warp", "agent", "run", "--model", "gpt-4o"]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(err_str.contains("prompt_group") || err_str.contains("required"));
}

#[test]
fn agent_run_accepts_prompt_only() {
    let args = Args::try_parse_from(["warp", "agent", "run", "--prompt", "hello"]).unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert_eq!(run_args.prompt_arg.prompt.as_deref(), Some("hello"));
    assert!(run_args.prompt_arg.saved_prompt.is_none());
    assert!(run_args.skill.is_none());
}

#[test]
fn agent_run_accepts_saved_prompt_only() {
    let args = Args::try_parse_from(["warp", "agent", "run", "--saved-prompt", "sp-123"]).unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert!(run_args.prompt_arg.prompt.is_none());
    assert_eq!(run_args.prompt_arg.saved_prompt.as_deref(), Some("sp-123"));
    assert!(run_args.skill.is_none());
}

#[test]
fn agent_run_accepts_skill_only() {
    let args = Args::try_parse_from(["warp", "agent", "run", "--skill", "my-skill"]).unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert!(run_args.prompt_arg.prompt.is_none());
    assert!(run_args.skill.is_some());
}

#[test]
fn agent_run_accepts_prompt_and_skill() {
    let args = Args::try_parse_from([
        "warp", "agent", "run", "--prompt", "do stuff", "--skill", "my-skill",
    ])
    .unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert_eq!(run_args.prompt_arg.prompt.as_deref(), Some("do stuff"));
    assert!(run_args.skill.is_some());
}

#[test]
fn agent_run_accepts_saved_prompt_and_skill() {
    let args = Args::try_parse_from([
        "warp",
        "agent",
        "run",
        "--saved-prompt",
        "sp-1",
        "--skill",
        "my-skill",
    ])
    .unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert_eq!(run_args.prompt_arg.saved_prompt.as_deref(), Some("sp-1"));
    assert!(run_args.skill.is_some());
}

#[test]
fn agent_run_rejects_prompt_and_saved_prompt() {
    let result = Args::try_parse_from([
        "warp",
        "agent",
        "run",
        "--prompt",
        "hello",
        "--saved-prompt",
        "sp-1",
    ]);
    assert!(result.is_err());
}

#[test]
fn agent_run_accepts_computer_use_flag() {
    let args = Args::try_parse_from([
        "warp",
        "agent",
        "run",
        "--prompt",
        "hello",
        "--computer-use",
    ])
    .unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert!(run_args.computer_use.computer_use);
    assert!(!run_args.computer_use.no_computer_use);
    assert_eq!(run_args.computer_use.computer_use_override(), Some(true));
}

#[test]
fn agent_run_accepts_no_computer_use_flag() {
    let args = Args::try_parse_from([
        "warp",
        "agent",
        "run",
        "--prompt",
        "hello",
        "--no-computer-use",
    ])
    .unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert!(!run_args.computer_use.computer_use);
    assert!(run_args.computer_use.no_computer_use);
    assert_eq!(run_args.computer_use.computer_use_override(), Some(false));
}

#[test]
fn agent_run_rejects_both_computer_use_flags() {
    let result = Args::try_parse_from([
        "warp",
        "agent",
        "run",
        "--prompt",
        "hello",
        "--computer-use",
        "--no-computer-use",
    ]);

    assert!(result.is_err());
}

#[test]
fn agent_run_defaults_to_no_computer_use_override() {
    let args = Args::try_parse_from(["warp", "agent", "run", "--prompt", "hello"]).unwrap();

    let Some(Command::CommandLine(boxed_cmd)) = args.command else {
        panic!("Expected `warp agent run` command");
    };
    let CliCommand::Agent(AgentCommand::Run(run_args)) = boxed_cmd.as_ref() else {
        panic!("Expected `warp agent run` command");
    };

    assert!(!run_args.computer_use.computer_use);
    assert!(!run_args.computer_use.no_computer_use);
    assert_eq!(run_args.computer_use.computer_use_override(), None);
}
#[test]
fn harness_parse_orchestration_harness_accepts_aliases() {
    assert_eq!(
        Harness::parse_orchestration_harness("claude-code"),
        Some(Harness::Claude)
    );
    assert_eq!(
        Harness::parse_orchestration_harness("open_code"),
        Some(Harness::OpenCode)
    );
}

#[test]
fn harness_parse_local_child_harness_rejects_oz() {
    assert_eq!(Harness::parse_local_child_harness("oz"), None);
    assert_eq!(
        Harness::parse_local_child_harness("opencode"),
        Some(Harness::OpenCode)
    );
}

#[test]
fn harness_parse_orchestration_harness_accepts_codex() {
    assert_eq!(
        Harness::parse_orchestration_harness("codex"),
        Some(Harness::Codex)
    );
}

#[test]
fn harness_parse_local_child_harness_accepts_codex() {
    assert_eq!(
        Harness::parse_local_child_harness("codex"),
        Some(Harness::Codex)
    );
}
#[test]
#[serial_test::serial]
fn hidden_server_overrides_parse_from_env() {
    let previous_server_root = set_env_var(SERVER_ROOT_URL_OVERRIDE_ENV, "http://localhost:8080");
    let previous_ws = set_env_var(WS_SERVER_URL_OVERRIDE_ENV, "ws://localhost:8082/graphql/v2");
    let previous_session_sharing = set_env_var(
        SESSION_SHARING_SERVER_URL_OVERRIDE_ENV,
        "ws://127.0.0.1:8081",
    );

    let args = Args::try_parse_from(["warp", "whoami"]).unwrap();

    restore_env_var(SERVER_ROOT_URL_OVERRIDE_ENV, previous_server_root);
    restore_env_var(WS_SERVER_URL_OVERRIDE_ENV, previous_ws);
    restore_env_var(
        SESSION_SHARING_SERVER_URL_OVERRIDE_ENV,
        previous_session_sharing,
    );

    assert_eq!(args.server_root_url(), Some("http://localhost:8080"));
    assert_eq!(args.ws_server_url(), Some("ws://localhost:8082/graphql/v2"));
    assert_eq!(
        args.session_sharing_server_url(),
        Some("ws://127.0.0.1:8081")
    );
}
