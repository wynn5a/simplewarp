use std::collections::HashMap;

use pathfinder_color::ColorU;
use smol_str::SmolStr;
use warp_util::path::EscapeChar;

use super::CLIAgent;
use crate::ui_components::icons::Icon;

/// Helper to build an alias map from pairs.
fn aliases(pairs: &[(&str, &str)]) -> HashMap<SmolStr, String> {
    pairs
        .iter()
        .map(|(k, v)| (SmolStr::new(k), v.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Helpers for prompt-building tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// build_review_prompt tests
// ---------------------------------------------------------------------------

#[test]
fn test_detect_known_agents() {
    for (command, expected) in [
        ("claude", CLIAgent::Claude),
        ("gemini", CLIAgent::Gemini),
        ("codex", CLIAgent::Codex),
        ("amp", CLIAgent::Amp),
        ("droid", CLIAgent::Droid),
        ("opencode", CLIAgent::OpenCode),
        ("copilot", CLIAgent::Copilot),
        ("agent", CLIAgent::CursorCli),
        ("goose", CLIAgent::Goose),
        ("vibe", CLIAgent::Vibe),
        ("agy", CLIAgent::Antigravity),
        ("omp", CLIAgent::OhMyPi),
        ("warp", CLIAgent::WarpTui),
        ("warp-dev", CLIAgent::WarpTui),
        ("./script/run-tui", CLIAgent::WarpTui),
    ] {
        assert_eq!(
            CLIAgent::detect(command, None, None),
            Some(expected),
            "failed to detect {command}",
        );
    }
}

#[test]
fn test_detect_with_arguments() {
    assert_eq!(
        CLIAgent::detect("claude --model opus", None, None),
        Some(CLIAgent::Claude),
    );
    assert_eq!(
        CLIAgent::detect("gemini chat", None, None),
        Some(CLIAgent::Gemini),
    );
}

#[test]
fn test_detect_vibe_acp_binary() {
    // The mistral-vibe package ships a `vibe-acp` ACP-mode binary alongside
    // the user-facing `vibe` TUI. Both must be detected as the same agent.
    assert_eq!(
        CLIAgent::detect("vibe-acp", None, None),
        Some(CLIAgent::Vibe),
    );
    assert_eq!(
        CLIAgent::detect("vibe-acp --some-flag", None, None),
        Some(CLIAgent::Vibe),
    );
    // Distinct binary names should not bleed into Vibe.
    assert_eq!(CLIAgent::detect("vibe-other", None, None), None);
}

#[test]
fn test_detect_with_leading_whitespace() {
    assert_eq!(
        CLIAgent::detect("  claude", None, None),
        Some(CLIAgent::Claude),
    );
    assert_eq!(
        CLIAgent::detect("\tclaude --help", None, None),
        Some(CLIAgent::Claude),
    );
}

#[test]
fn test_detect_no_match() {
    assert_eq!(CLIAgent::detect("ls -la", None, None), None);
    assert_eq!(CLIAgent::detect("vim", None, None), None);
    assert_eq!(CLIAgent::detect("claude_wrapper", None, None), None);
}

#[test]
fn test_detect_with_alias() {
    let map = aliases(&[("c", "claude")]);
    assert_eq!(
        CLIAgent::detect("c", None, Some(&map)),
        Some(CLIAgent::Claude),
    );
    assert_eq!(
        CLIAgent::detect("c --help", None, Some(&map)),
        Some(CLIAgent::Claude),
    );

    let map = aliases(&[("o", "omp")]);
    assert_eq!(
        CLIAgent::detect("o", None, Some(&map)),
        Some(CLIAgent::OhMyPi),
    );
}

#[test]
fn test_detect_alias_not_matching() {
    let map = aliases(&[("c", "cat")]);
    assert_eq!(CLIAgent::detect("c", None, Some(&map)), None);
}

#[test]
fn test_detect_alias_multi_word_value() {
    // Alias whose value starts with "gemini" but has extra words
    let map = aliases(&[("g", "gemini chat --verbose")]);
    assert_eq!(
        CLIAgent::detect("g", None, Some(&map)),
        Some(CLIAgent::Gemini),
    );
}

#[test]
fn test_detect_with_env_var_prefix() {
    assert_eq!(
        CLIAgent::detect("EXAMPLE=true opencode", Some(EscapeChar::Backslash), None,),
        Some(CLIAgent::OpenCode),
    );
    assert_eq!(
        CLIAgent::detect("FOO=1 omp", Some(EscapeChar::Backslash), None),
        Some(CLIAgent::OhMyPi),
    );
}

#[test]
fn test_detect_with_multiple_env_vars() {
    assert_eq!(
        CLIAgent::detect(
            "FOO=1 BAR=2 opencode --flag",
            Some(EscapeChar::Backslash),
            None,
        ),
        Some(CLIAgent::OpenCode),
    );
}

#[test]
fn test_detect_with_alias_and_env_var() {
    let map = aliases(&[("oc", "EXAMPLE=1 opencode")]);
    assert_eq!(
        CLIAgent::detect("oc --flag", Some(EscapeChar::Backslash), Some(&map)),
        Some(CLIAgent::OpenCode),
    );
}

#[test]
fn test_detect_aifx_agent_run_claude_is_not_an_agent() {
    assert_eq!(CLIAgent::detect("aifx agent run claude", None, None), None,);
}

#[test]
fn test_serialized_name_round_trips_known_agents() {
    for agent in enum_iterator::all::<CLIAgent>() {
        let name = agent.to_serialized_name();
        if agent == CLIAgent::Unknown {
            assert_eq!(name, "Unknown");
        } else {
            assert!(!name.is_empty(), "empty serialized name for {agent:?}");
        }
        assert_eq!(
            CLIAgent::from_serialized_name(&name),
            agent,
            "round-trip failed for {agent:?} with serialized name {name:?}",
        );
    }
}

#[test]
fn test_from_serialized_name_falls_back_to_unknown() {
    assert_eq!(CLIAgent::from_serialized_name(""), CLIAgent::Unknown);
    assert_eq!(
        CLIAgent::from_serialized_name("nonexistent"),
        CLIAgent::Unknown
    );
}

#[test]
fn test_oh_my_pi_supports_bash_mode() {
    assert!(CLIAgent::OhMyPi.supports_bash_mode());
}

#[test]
fn test_warp_tui_matches_binaries_and_launchers() {
    // Direct binary names.
    assert!(CLIAgent::WarpTui.matches_command("warp", None));
    assert!(CLIAgent::WarpTui.matches_command("warp-preview", None));
    assert!(CLIAgent::WarpTui.matches_command("warp-dev", None));
    assert!(CLIAgent::WarpTui.matches_command("warp-tui", None));
    assert!(CLIAgent::WarpTui.matches_command("warp-tui-oss", None));
    // The dev launcher script.
    assert!(CLIAgent::WarpTui.matches_command("./script/run-tui", None));
    assert!(CLIAgent::WarpTui.matches_command("script/run-tui", None));
    // Absolute / relative paths to the binary.
    assert!(CLIAgent::WarpTui.matches_command("/workspace/warp/target/debug/warp-tui", None,));
    assert!(CLIAgent::WarpTui.matches_command("./target/debug/warp-tui", None));
    assert!(CLIAgent::WarpTui.matches_command(
        "/Applications/WarpPreview.app/Contents/MacOS/warp-preview --resume abc",
        None,
    ));
    // With arguments and leading whitespace.
    assert!(CLIAgent::WarpTui.matches_command("  warp --resume abc", None));
}

#[test]
fn test_warp_tui_matches_with_env_var_prefix() {
    // Env-var assignments before the command are skipped when an escape char is
    // provided (mirrors `CLIAgent::detect`).
    assert!(
        CLIAgent::WarpTui.matches_command("WARP_API_KEY=secret warp", Some(EscapeChar::Backslash),)
    );
}

#[test]
fn test_warp_tui_does_not_match_other_commands() {
    assert!(!CLIAgent::WarpTui.matches_command("vim", None));
    assert!(!CLIAgent::WarpTui.matches_command("htop", None));
    assert!(!CLIAgent::WarpTui.matches_command("claude", None));
    // Lookalikes / substrings should not match.
    assert!(!CLIAgent::WarpTui.matches_command("warp-preview-wrapper", None));
    assert!(!CLIAgent::WarpTui.matches_command("mywarp-dev", None));
    assert!(!CLIAgent::WarpTui.matches_command("warp-tui-wrapper", None));
    assert!(!CLIAgent::WarpTui.matches_command("mywarp-tui", None));
    assert!(!CLIAgent::WarpTui.matches_command("", None));
    // `cargo run` is a known non-match (the first token is `cargo`).
    assert!(!CLIAgent::WarpTui.matches_command("cargo run -p warp_tui", None));
}

#[test]
fn test_warp_tui_variant_properties() {
    assert!(CLIAgent::Claude.supports_cli_agent_footer());
    assert_eq!(CLIAgent::WarpTui.command_prefix(), "warp");
    assert_eq!(
        CLIAgent::WarpTui.command_prefixes(),
        &[
            "warp",
            "warp-preview",
            "warp-dev",
            "warp-tui",
            "warp-tui-oss",
            "run-tui",
        ]
    );
    assert_eq!(CLIAgent::WarpTui.display_name(), "Warp TUI");
    assert_eq!(CLIAgent::WarpTui.brand_color(), Some(ColorU::black()));
    assert_eq!(CLIAgent::WarpTui.icon(), Some(Icon::Warp));
    assert_eq!(CLIAgent::WarpTui.brand_icon_color(), ColorU::white());
    assert!(CLIAgent::WarpTui.supported_skill_providers().is_empty());
    assert!(!CLIAgent::WarpTui.supports_bash_mode());
    assert!(!CLIAgent::WarpTui.supports_cli_agent_footer());
    // Serialized name round-trips (also covered by
    // `test_serialized_name_round_trips_known_agents`, asserted explicitly here).
    assert_eq!(
        CLIAgent::from_serialized_name(&CLIAgent::WarpTui.to_serialized_name()),
        CLIAgent::WarpTui
    );
}
