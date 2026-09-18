use std::collections::HashMap;
use std::fs;

use tempfile::TempDir;
use uuid::Uuid;

use super::*;

#[test]
fn claude_command_uses_session_id_when_not_resuming() {
    let uuid = Uuid::new_v4();
    let cmd = claude_command("claude", &uuid, "/tmp/prompt.txt", None, None, false);
    assert!(
        cmd.contains(&format!("--session-id {uuid}")),
        "expected --session-id flag in non-resume command, got: {cmd}"
    );
    assert!(
        !cmd.contains("--resume"),
        "non-resume command should not contain --resume, got: {cmd}"
    );
}

#[test]
fn claude_command_uses_resume_flag_when_resuming() {
    let uuid = Uuid::new_v4();
    let cmd = claude_command("claude", &uuid, "/tmp/prompt.txt", None, None, true);
    assert!(
        cmd.contains(&format!("--resume {uuid}")),
        "expected --resume flag in resume command, got: {cmd}"
    );
    assert!(
        !cmd.contains("--session-id"),
        "resume command should not contain --session-id, got: {cmd}"
    );
}

#[test]
fn claude_command_pipes_prompt_path() {
    let uuid = Uuid::new_v4();
    let cmd = claude_command(
        "claude",
        &uuid,
        "/tmp/prompt with spaces.txt",
        None,
        None,
        true,
    );
    assert!(
        cmd.contains("< '/tmp/prompt with spaces.txt'"),
        "expected single-quoted stdin redirect of the prompt path, got: {cmd}"
    );
    assert!(
        cmd.contains("--dangerously-skip-permissions"),
        "expected --dangerously-skip-permissions, got: {cmd}"
    );
}

#[test]
fn serialize_claude_mcp_config_cli_server() {
    let servers = HashMap::from([(
        "test-server".to_string(),
        JSONMCPServer {
            transport_type: JSONTransportType::CLIServer {
                command: "node".to_string(),
                args: vec!["server.js".to_string()],
                env: HashMap::from([("API_KEY".to_string(), "secret".to_string())]),
                working_directory: None,
            },
        },
    )]);
    let json = serialize_claude_mcp_config(&servers).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let server = &parsed["mcpServers"]["test-server"];
    assert_eq!(server["type"], "stdio");
    assert_eq!(server["command"], "node");
    assert_eq!(server["args"][0], "server.js");
    assert_eq!(server["env"]["API_KEY"], "secret");
}

#[test]
fn serialize_claude_mcp_config_cli_server_with_cwd() {
    let servers = HashMap::from([(
        "test-server".to_string(),
        JSONMCPServer {
            transport_type: JSONTransportType::CLIServer {
                command: "node".to_string(),
                args: vec!["server.js".to_string()],
                env: HashMap::new(),
                working_directory: Some("/opt/mcp".to_string()),
            },
        },
    )]);
    let json = serialize_claude_mcp_config(&servers).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let server = &parsed["mcpServers"]["test-server"];
    assert_eq!(server["cwd"], "/opt/mcp");
}

#[test]
fn serialize_claude_mcp_config_cli_server_omits_cwd_when_none() {
    let servers = HashMap::from([(
        "test-server".to_string(),
        JSONMCPServer {
            transport_type: JSONTransportType::CLIServer {
                command: "node".to_string(),
                args: vec![],
                env: HashMap::new(),
                working_directory: None,
            },
        },
    )]);
    let json = serialize_claude_mcp_config(&servers).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let server = &parsed["mcpServers"]["test-server"];
    assert!(server.get("cwd").is_none());
}

#[test]
fn serialize_claude_mcp_config_sse_server() {
    let servers = HashMap::from([(
        "remote".to_string(),
        JSONMCPServer {
            transport_type: JSONTransportType::SSEServer {
                url: "https://mcp.example.com".to_string(),
                headers: HashMap::from([("Authorization".to_string(), "Bearer tok".to_string())]),
            },
        },
    )]);
    let json = serialize_claude_mcp_config(&servers).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let server = &parsed["mcpServers"]["remote"];
    assert_eq!(server["type"], "http");
    assert_eq!(server["url"], "https://mcp.example.com");
    assert_eq!(server["headers"]["Authorization"], "Bearer tok");
}

#[test]
fn prepare_claude_config_creates_config_file_without_api_suffix() {
    let tmp = TempDir::new().unwrap();
    let claude_json_path = tmp.path().join(".claude.json");
    let working_dir = tmp.path().join("workspace/project");

    prepare_claude_config(&claude_json_path, &working_dir, None).unwrap();

    let claude_config: Value =
        serde_json::from_slice(&fs::read(claude_json_path).unwrap()).unwrap();
    assert_eq!(claude_config["hasCompletedOnboarding"], Value::Bool(true));
    assert_eq!(
        claude_config["lspRecommendationDisabled"],
        Value::Bool(true)
    );
    let working_dir_key = working_dir.to_string_lossy().to_string();
    assert_eq!(
        claude_config["projects"][working_dir_key]["hasTrustDialogAccepted"],
        Value::Bool(true)
    );
    assert_eq!(claude_config.get("customApiKeyResponses"), None);
}

#[test]
fn prepare_claude_config_creates_config_file_with_api_suffix() {
    let tmp = TempDir::new().unwrap();
    let claude_json_path = tmp.path().join(".claude.json");
    let working_dir = tmp.path().join("workspace/project");

    prepare_claude_config(
        &claude_json_path,
        &working_dir,
        Some("QLWn-dUnuwQ-hIhDiAAA"),
    )
    .unwrap();

    let claude_config: Value =
        serde_json::from_slice(&fs::read(claude_json_path).unwrap()).unwrap();
    assert_eq!(
        claude_config["customApiKeyResponses"]["approved"],
        serde_json::json!(["QLWn-dUnuwQ-hIhDiAAA"]),
    );
}

#[test]
fn prepare_claude_config_merges_existing_config() {
    let tmp = TempDir::new().unwrap();
    let claude_json_path = tmp.path().join(".claude.json");
    fs::write(
        &claude_json_path,
        r#"{"theme":"dark","projects":{"/existing/project":{"allowedTools":["Bash"],"nested":{"value":2}}},"customApiKeyResponses":{"approved":["existing-suffix-12345"]}}"#,
    )
    .unwrap();

    let working_dir = tmp.path().join("workspace/project");
    prepare_claude_config(
        &claude_json_path,
        &working_dir,
        Some("new-suffix-1234567890"),
    )
    .unwrap();

    let claude_config: Value =
        serde_json::from_slice(&fs::read(claude_json_path).unwrap()).unwrap();
    assert_eq!(claude_config["theme"], "dark");
    assert_eq!(
        claude_config["lspRecommendationDisabled"],
        Value::Bool(true)
    );
    assert_eq!(
        claude_config["projects"]["/existing/project"]["allowedTools"],
        serde_json::json!(["Bash"])
    );
    assert_eq!(
        claude_config["projects"]["/existing/project"]["nested"]["value"],
        2
    );
    // Both existing and new suffixes should be present.
    assert_eq!(
        claude_config["customApiKeyResponses"]["approved"],
        serde_json::json!(["existing-suffix-12345", "new-suffix-1234567890"]),
    );
    let working_dir_key = working_dir.to_string_lossy().to_string();
    assert_eq!(
        claude_config["projects"][working_dir_key]["hasTrustDialogAccepted"],
        Value::Bool(true)
    );
}

#[test]
fn prepare_claude_config_no_duplicate_suffix() {
    let tmp = TempDir::new().unwrap();
    let claude_json_path = tmp.path().join(".claude.json");
    fs::write(
        &claude_json_path,
        r#"{"customApiKeyResponses":{"approved":["QLWn-dUnuwQ-hIhDiAAA"]}}"#,
    )
    .unwrap();

    let working_dir = tmp.path().join("workspace/project");
    prepare_claude_config(
        &claude_json_path,
        &working_dir,
        Some("QLWn-dUnuwQ-hIhDiAAA"),
    )
    .unwrap();

    let claude_config: Value =
        serde_json::from_slice(&fs::read(claude_json_path).unwrap()).unwrap();
    assert_eq!(
        claude_config["customApiKeyResponses"]["approved"],
        serde_json::json!(["QLWn-dUnuwQ-hIhDiAAA"]),
    );
}

#[test]
fn prepare_claude_config_none_suffix_preserves_existing_responses() {
    let tmp = TempDir::new().unwrap();
    let claude_json_path = tmp.path().join(".claude.json");
    fs::write(
        &claude_json_path,
        r#"{"customApiKeyResponses":{"approved":["existing-suffix-12345"],"rejected":["bad-key"]}}"#,
    )
    .unwrap();

    let working_dir = tmp.path().join("workspace/project");
    prepare_claude_config(&claude_json_path, &working_dir, None).unwrap();

    let claude_config: Value =
        serde_json::from_slice(&fs::read(claude_json_path).unwrap()).unwrap();
    assert_eq!(
        claude_config["customApiKeyResponses"]["approved"],
        serde_json::json!(["existing-suffix-12345"]),
    );
    assert_eq!(
        claude_config["customApiKeyResponses"]["rejected"],
        serde_json::json!(["bad-key"]),
    );
}

#[test]
#[serial_test::serial]
fn prepare_claude_environment_config_without_config_dir_uses_home_global_config() {
    let home_dir = TempDir::new().unwrap();
    let old_home = std::env::var_os("HOME");
    let old_config_dir = std::env::var_os("CLAUDE_CONFIG_DIR");
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("HOME", home_dir.path()) };
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") };

    let working_dir = home_dir.path().join("workspace/project");
    prepare_claude_environment_config(&working_dir, &HashMap::new()).unwrap();

    assert!(home_dir.path().join(CLAUDE_JSON_FILE_NAME).exists());
    assert!(
        home_dir
            .path()
            .join(".claude")
            .join(CLAUDE_SETTINGS_FILE_NAME)
            .exists()
    );
    assert!(
        !home_dir
            .path()
            .join(".claude")
            .join(CLAUDE_JSON_FILE_NAME)
            .exists()
    );

    match old_home {
        // TODO: Audit that the environment access only happens in single-threaded code.
        Some(home) => unsafe { std::env::set_var("HOME", home) },
        // TODO: Audit that the environment access only happens in single-threaded code.
        None => unsafe { std::env::remove_var("HOME") },
    }
    match old_config_dir {
        // TODO: Audit that the environment access only happens in single-threaded code.
        Some(dir) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", dir) },
        // TODO: Audit that the environment access only happens in single-threaded code.
        None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
    }
}

#[test]
#[serial_test::serial]
fn prepare_claude_environment_config_with_config_dir_uses_dir_global_config() {
    let home_dir = TempDir::new().unwrap();
    let claude_config_dir = TempDir::new().unwrap();
    let old_home = std::env::var_os("HOME");
    let old_config_dir = std::env::var_os("CLAUDE_CONFIG_DIR");
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("HOME", home_dir.path()) };
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", claude_config_dir.path()) };

    let working_dir = home_dir.path().join("workspace/project");
    prepare_claude_environment_config(&working_dir, &HashMap::new()).unwrap();

    assert!(
        claude_config_dir
            .path()
            .join(CLAUDE_JSON_FILE_NAME)
            .exists()
    );
    assert!(
        claude_config_dir
            .path()
            .join(CLAUDE_SETTINGS_FILE_NAME)
            .exists()
    );
    assert!(!home_dir.path().join(CLAUDE_JSON_FILE_NAME).exists());

    match old_home {
        // TODO: Audit that the environment access only happens in single-threaded code.
        Some(home) => unsafe { std::env::set_var("HOME", home) },
        // TODO: Audit that the environment access only happens in single-threaded code.
        None => unsafe { std::env::remove_var("HOME") },
    }
    match old_config_dir {
        // TODO: Audit that the environment access only happens in single-threaded code.
        Some(dir) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", dir) },
        // TODO: Audit that the environment access only happens in single-threaded code.
        None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
    }
}

#[test]
#[serial_test::serial]
fn resolve_suffix_from_resolved_env_vars() {
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var(ANTHROPIC_API_KEY_ENV) };
    let key = "sk-ant-api03-abcdefghij1234567890ABCDEFGHIJ1234567890abcdefghij1234567890QLWn-dUnuwQ-hIhDiAAA";
    let resolved = HashMap::from([(OsString::from("ANTHROPIC_API_KEY"), OsString::from(key))]);
    let suffix = resolve_anthropic_api_key_suffix(&resolved);
    assert_eq!(suffix.as_deref(), Some("QLWn-dUnuwQ-hIhDiAAA"));
}

#[test]
#[serial_test::serial]
fn resolve_suffix_returns_none_for_short_key() {
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var(ANTHROPIC_API_KEY_ENV) };
    let resolved = HashMap::from([(OsString::from("ANTHROPIC_API_KEY"), OsString::from("short"))]);
    assert_eq!(resolve_anthropic_api_key_suffix(&resolved), None);
}

#[test]
#[serial_test::serial]
fn resolve_suffix_returns_none_when_empty() {
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var(ANTHROPIC_API_KEY_ENV) };
    assert_eq!(resolve_anthropic_api_key_suffix(&HashMap::new()), None);
}

#[test]
fn prepare_claude_settings_creates_settings_file() {
    let tmp = TempDir::new().unwrap();
    let claude_settings_path = tmp.path().join(".claude/settings.json");

    prepare_claude_settings(&claude_settings_path).unwrap();

    let claude_settings: Value =
        serde_json::from_slice(&fs::read(claude_settings_path).unwrap()).unwrap();
    assert_eq!(
        claude_settings["skipDangerousModePermissionPrompt"],
        Value::Bool(true)
    );
}

#[test]
fn prepare_claude_settings_merges_existing_settings() {
    let tmp = TempDir::new().unwrap();
    let claude_settings_path = tmp.path().join("settings.json");
    fs::write(
        &claude_settings_path,
        r#"{"editor":"vim","nested":{"value":1}}"#,
    )
    .unwrap();

    prepare_claude_settings(&claude_settings_path).unwrap();

    let claude_settings: Value =
        serde_json::from_slice(&fs::read(claude_settings_path).unwrap()).unwrap();
    assert_eq!(claude_settings["editor"], "vim");
    assert_eq!(claude_settings["nested"]["value"], 1);
    assert_eq!(
        claude_settings["skipDangerousModePermissionPrompt"],
        Value::Bool(true)
    );
}
