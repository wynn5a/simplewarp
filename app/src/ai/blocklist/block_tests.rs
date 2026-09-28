use std::path::PathBuf;

use ai::agent::action::RunAgentsAgentRunConfig;
use ai::skills::SkillReference;
use settings::Setting;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warp_util::path::LineAndColumnArg;
use warpui::{App, SingletonEntity};

use super::{
    AIBlockEvent, CollapsibleElementState, CollapsibleExpansionState,
    default_collapsible_state_for_orchestration_action,
    default_collapsible_state_for_orchestration_message, open_code_action_event,
    received_message_collapsible_id,
};
use crate::ai::agent::{AIAgentActionType, StartAgentExecutionMode};
use crate::ai::blocklist::action_model::{
    compose_run_agents_child_prompt, run_agents_to_start_agent_mode,
};
use crate::code::editor_management::CodeSource;
use crate::settings::{AISettings, OrchestrationMessageDisplayMode};
use crate::test_util::settings::initialize_settings_for_tests;

#[test]
fn reasoning_auto_collapses_when_user_has_not_manually_toggled() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        let mut state = CollapsibleElementState::default();
        app.update(|ctx| {
            state.finish_reasoning(ctx);
        });

        assert!(matches!(
            state.expansion_state,
            CollapsibleExpansionState::Collapsed
        ));
    });
}

#[test]
fn collapsed_initializer_starts_collapsed() {
    let state = CollapsibleElementState::collapsed();

    assert!(matches!(
        state.expansion_state,
        CollapsibleExpansionState::Collapsed
    ));
}

#[test]
fn orchestration_show_and_collapse_collapses_after_finish() {
    let mut state = default_collapsible_state_for_orchestration_message(
        OrchestrationMessageDisplayMode::ShowAndCollapse,
    );

    state.finish_orchestration_message(OrchestrationMessageDisplayMode::ShowAndCollapse);

    assert!(matches!(
        state.expansion_state,
        CollapsibleExpansionState::Collapsed
    ));
}

#[test]
fn orchestration_always_show_stays_expanded_after_finish() {
    let mut state = default_collapsible_state_for_orchestration_message(
        OrchestrationMessageDisplayMode::AlwaysShow,
    );

    state.finish_orchestration_message(OrchestrationMessageDisplayMode::AlwaysShow);

    assert!(matches!(
        state.expansion_state,
        CollapsibleExpansionState::Expanded {
            is_finished: true,
            scroll_pinned_to_bottom: false
        }
    ));
}

#[test]
fn orchestration_send_message_starts_collapsed() {
    let state = default_collapsible_state_for_orchestration_action(
        &AIAgentActionType::SendMessageToAgent {
            addresses: vec!["child-agent".to_string()],
            subject: "Status".to_string(),
            message: "Body".to_string(),
        },
        OrchestrationMessageDisplayMode::AlwaysCollapse,
    )
    .expect("send-message actions should get a collapsible state");

    assert!(matches!(
        state.expansion_state,
        CollapsibleExpansionState::Collapsed
    ));
}

#[test]
fn non_orchestration_actions_do_not_get_collapsible_state_defaults() {
    assert!(
        default_collapsible_state_for_orchestration_action(
            &AIAgentActionType::OpenCodeReview,
            OrchestrationMessageDisplayMode::AlwaysCollapse,
        )
        .is_none()
    );
}

#[test]
fn open_code_action_routes_links_to_configured_editor_and_non_links_to_warp() {
    let linked_source = CodeSource::Link {
        path: PathBuf::from("/workspace/project/src/main.rs"),
        range_start: Some(LineAndColumnArg {
            line_num: 42,
            column_num: Some(7),
        }),
        range_end: None,
    };

    assert!(matches!(
        open_code_action_event(
            &linked_source,
            crate::util::file::external_editor::settings::EditorLayout::SplitPane,
        ),
        AIBlockEvent::OpenDetectedFilePath {
            absolute_path,
            line_and_column_num: Some(LineAndColumnArg {
                line_num: 42,
                column_num: Some(7),
            }),
            target_override: None,
        } if absolute_path.as_path() == std::path::Path::new("/workspace/project/src/main.rs")
    ));

    let skill_source = CodeSource::Skill {
        reference: SkillReference::Path(LocalOrRemotePath::Local(PathBuf::from(
            "/workspace/project/.warp/skills/example/SKILL.md",
        ))),
        location: LocalOrRemotePath::Local(PathBuf::from(
            "/workspace/project/.warp/skills/example/SKILL.md",
        )),
        origin: crate::ai::skills::SkillOpenOrigin::ReadSkill,
    };

    assert!(matches!(
        open_code_action_event(
            &skill_source,
            crate::util::file::external_editor::settings::EditorLayout::NewTab,
        ),
        AIBlockEvent::OpenCodeInWarp {
            source,
            layout: crate::util::file::external_editor::settings::EditorLayout::NewTab,
        } if source == skill_source
    ));
}
#[test]
fn orchestration_show_and_collapse_starts_sent_messages_expanded() {
    let state = default_collapsible_state_for_orchestration_action(
        &AIAgentActionType::SendMessageToAgent {
            addresses: vec!["child-agent".to_string()],
            subject: "Status".to_string(),
            message: "Body".to_string(),
        },
        OrchestrationMessageDisplayMode::ShowAndCollapse,
    )
    .expect("send-message actions should get a collapsible state");

    assert!(matches!(
        state.expansion_state,
        CollapsibleExpansionState::Expanded {
            is_finished: false,
            scroll_pinned_to_bottom: true
        }
    ));
}

#[test]
fn orchestration_always_show_starts_sent_messages_expanded() {
    let state = default_collapsible_state_for_orchestration_action(
        &AIAgentActionType::SendMessageToAgent {
            addresses: vec!["child-agent".to_string()],
            subject: "Status".to_string(),
            message: "Body".to_string(),
        },
        OrchestrationMessageDisplayMode::AlwaysShow,
    )
    .expect("send-message actions should get a collapsible state");

    assert!(matches!(
        state.expansion_state,
        CollapsibleExpansionState::Expanded {
            is_finished: false,
            scroll_pinned_to_bottom: true
        }
    ));
}

#[test]
fn orchestration_received_messages_follow_initial_message_display_mode() {
    let show_and_collapse = default_collapsible_state_for_orchestration_message(
        OrchestrationMessageDisplayMode::ShowAndCollapse,
    );
    assert!(matches!(
        show_and_collapse.expansion_state,
        CollapsibleExpansionState::Expanded {
            is_finished: false,
            scroll_pinned_to_bottom: true
        }
    ));
    let collapsed = default_collapsible_state_for_orchestration_message(
        OrchestrationMessageDisplayMode::AlwaysCollapse,
    );
    assert!(matches!(
        collapsed.expansion_state,
        CollapsibleExpansionState::Collapsed
    ));
    let expanded = default_collapsible_state_for_orchestration_message(
        OrchestrationMessageDisplayMode::AlwaysShow,
    );

    assert!(matches!(
        expanded.expansion_state,
        CollapsibleExpansionState::Expanded {
            is_finished: false,
            scroll_pinned_to_bottom: true
        }
    ));
}

#[test]
fn always_show_thinking_stays_expanded_after_finish() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .thinking_display_mode
                .set_value(crate::settings::ThinkingDisplayMode::AlwaysShow, ctx)
                .unwrap();
        });

        let mut state = CollapsibleElementState::default();
        app.update(|ctx| {
            state.finish_reasoning(ctx);
        });

        assert!(matches!(
            state.expansion_state,
            CollapsibleExpansionState::Expanded {
                is_finished: true,
                scroll_pinned_to_bottom: false
            }
        ));
    });
}

#[test]
fn manual_collapse_while_streaming_stays_collapsed_after_finish() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        let mut state = CollapsibleElementState::default();

        state.toggle_expansion();
        app.update(|ctx| {
            state.finish_reasoning(ctx);
        });

        assert!(matches!(
            state.expansion_state,
            CollapsibleExpansionState::Collapsed
        ));
    });
}

#[test]
fn manual_reexpand_while_streaming_stays_expanded_after_finish() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        let mut state = CollapsibleElementState::default();

        state.toggle_expansion();
        state.toggle_expansion();
        app.update(|ctx| {
            state.finish_reasoning(ctx);
        });

        assert!(matches!(
            state.expansion_state,
            CollapsibleExpansionState::Expanded {
                is_finished: true,
                scroll_pinned_to_bottom: false
            }
        ));
    });
}

#[test]
fn received_message_collapsible_id_prefixes_row_ids() {
    let first = received_message_collapsible_id("message-1");
    let second = received_message_collapsible_id("message-2");

    assert_eq!(&*first, "received-message:message-1");
    assert_eq!(&*second, "received-message:message-2");
    assert_ne!(first, second);
}

#[test]
fn compose_child_prompt_concatenates_when_both_non_empty() {
    let composed = compose_run_agents_child_prompt("base", "do X");
    assert_eq!(composed, "base\n\ndo X");
}

#[test]
fn compose_child_prompt_uses_base_only_when_per_agent_empty() {
    let composed = compose_run_agents_child_prompt("base", "");
    assert_eq!(composed, "base");
}

#[test]
fn compose_child_prompt_uses_per_agent_only_when_base_empty() {
    let composed = compose_run_agents_child_prompt("", "do X");
    assert_eq!(composed, "do X");
}

#[test]
fn compose_child_prompt_returns_empty_when_both_empty() {
    let composed = compose_run_agents_child_prompt("", "");
    assert_eq!(composed, "");
}

#[test]
fn compose_child_prompt_treats_whitespace_only_base_as_empty() {
    let composed = compose_run_agents_child_prompt("   \n", "do X");
    assert_eq!(composed, "do X");
}

fn agent_cfg() -> RunAgentsAgentRunConfig {
    RunAgentsAgentRunConfig {
        name: "child".to_string(),
        prompt: "do X".to_string(),
        title: "Child".to_string(),
        agent_identity_uid: String::new(),
        model_id: String::new(),
    }
}

#[test]
fn local_arm_rejects_agent_identity_uid() {
    let mut cfg = agent_cfg();
    cfg.agent_identity_uid = "sa-uid-1".to_string();
    let err = run_agents_to_start_agent_mode("", "", &cfg)
        .expect_err("Local + agent_identity_uid must be rejected");
    assert!(err.contains("agent_identity_uid requires remote execution"));
}

#[test]
fn local_arm_rejects_disabled_codex() {
    let err = run_agents_to_start_agent_mode("codex", "auto", &agent_cfg())
        .expect_err("Local+codex must be rejected while disabled");
    assert_eq!(err, "Local Codex child agents are temporarily disabled.");
}

#[test]
fn local_arm_allows_claude() {
    let mode = run_agents_to_start_agent_mode("claude", "auto", &agent_cfg())
        .expect("Local+claude should convert");
    assert!(matches!(
        mode,
        StartAgentExecutionMode::Local {
            harness_type: Some(ref harness_type),
            model_id: Some(ref model_id),
        } if harness_type == "claude" && model_id == "auto"
    ));
}

#[test]
fn should_show_agent_mode_ask_user_question_speedbump_defaults_to_true() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        AISettings::handle(&app).read(&app, |settings, _ctx| {
            assert!(*settings.should_show_agent_mode_ask_user_question_speedbump);
        });
    });
}

#[test]
fn should_show_agent_mode_ask_user_question_speedbump_round_trips_to_false() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .should_show_agent_mode_ask_user_question_speedbump
                .set_value(false, ctx)
                .unwrap();
        });
        AISettings::handle(&app).read(&app, |settings, _ctx| {
            assert!(!*settings.should_show_agent_mode_ask_user_question_speedbump);
        });
    });
}
