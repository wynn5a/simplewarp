use std::path::PathBuf;

use ai::agent::action::{RunAgentsAgentRunConfig, RunAgentsExecutionMode, RunAgentsRequest};
use ai::agent::action_result::{
    RunAgentsAgentOutcome, RunAgentsAgentOutcomeKind, RunAgentsLaunchedExecutionMode,
    RunAgentsResult,
};
use ai::skills::SkillReference;
use warp_util::local_or_remote_path::LocalOrRemotePath;

use super::RunAgentsEditState;

fn make_request(harness: &str, mode: RunAgentsExecutionMode) -> RunAgentsRequest {
    make_request_with_skills(harness, mode, Vec::new())
}

fn make_request_with_skills(
    harness: &str,
    mode: RunAgentsExecutionMode,
    skills: Vec<SkillReference>,
) -> RunAgentsRequest {
    RunAgentsRequest {
        summary: "summary".to_string(),
        base_prompt: "base".to_string(),
        skills,
        model_id: "auto".to_string(),
        harness_type: harness.to_string(),
        execution_mode: mode,
        agent_run_configs: vec![RunAgentsAgentRunConfig {
            name: "child".to_string(),
            prompt: "do work".to_string(),
            title: "Child agent".to_string(),
            model_id: String::new(),
        }],
        plan_id: String::new(),
    }
}

#[test]
fn local_with_any_harness_does_not_disable_accept() {
    for harness in ["oz", "claude", "gemini", "opencode"] {
        let state =
            RunAgentsEditState::from_request(&make_request(harness, RunAgentsExecutionMode::Local));
        assert!(
            state
                .orchestration_config_state
                .accept_disabled_reason()
                .is_none(),
            "Local + {harness} should allow Accept"
        );
    }
}

#[test]
fn from_request_sanitizes_disabled_local_harness_to_oz() {
    let state =
        RunAgentsEditState::from_request(&make_request("codex", RunAgentsExecutionMode::Local));

    assert_eq!(state.orchestration_config_state.harness_type, "oz");
    assert_eq!(state.orchestration_config_state.model_id, "");
    assert!(
        state
            .orchestration_config_state
            .accept_disabled_reason()
            .is_none()
    );
}

/// Accepting a persisted Remote request in the card launches it locally.
#[test]
fn persisted_remote_request_is_edited_and_accepted_as_local() {
    let state =
        RunAgentsEditState::from_request(&make_request("claude", RunAgentsExecutionMode::Remote));

    assert_eq!(state.orchestration_config_state.harness_type, "claude");
    assert_eq!(
        state.to_request().execution_mode,
        RunAgentsExecutionMode::Local
    );
}

#[test]
fn to_request_round_trips_request_fields() {
    let mut req = make_request_with_skills(
        "claude",
        RunAgentsExecutionMode::Local,
        vec![
            SkillReference::BundledSkillId("writing-pr-descriptions".to_string()),
            SkillReference::Path(LocalOrRemotePath::Local(PathBuf::from(
                "/tmp/skill/SKILL.md",
            ))),
        ],
    );
    req.plan_id = "plan-1".to_string();
    let state = RunAgentsEditState::from_request(&req);
    let round_tripped = state.to_request();
    assert_eq!(round_tripped, req);
}

mod format_terminal_state_tests {
    use super::super::{StatusKind, format_terminal_state};
    use super::*;

    fn launched(name: &str, agent_id: &str) -> RunAgentsAgentOutcome {
        RunAgentsAgentOutcome {
            name: name.to_string(),
            resolved_model_id: String::new(),
            kind: RunAgentsAgentOutcomeKind::Launched {
                agent_id: agent_id.to_string(),
            },
        }
    }

    fn failed(name: &str, error: &str) -> RunAgentsAgentOutcome {
        RunAgentsAgentOutcome {
            name: name.to_string(),
            resolved_model_id: String::new(),
            kind: RunAgentsAgentOutcomeKind::Failed {
                error: error.to_string(),
            },
        }
    }

    fn launched_result(agents: Vec<RunAgentsAgentOutcome>) -> RunAgentsResult {
        RunAgentsResult::Launched {
            model_id: "auto".to_string(),
            harness_type: "oz".to_string(),
            execution_mode: RunAgentsLaunchedExecutionMode::Local,
            agents,
        }
    }

    #[test]
    fn launched_singular_uses_singular_label() {
        let result = launched_result(vec![launched("child", "a-1")]);
        let (label, kind) = format_terminal_state(&result);
        assert_eq!(label, "Spawned 1 agent");
        assert!(matches!(kind, StatusKind::Success));
    }

    #[test]
    fn launched_plural_uses_plural_label() {
        let result = launched_result(vec![
            launched("a", "a-1"),
            launched("b", "a-2"),
            launched("c", "a-3"),
        ]);
        let (label, kind) = format_terminal_state(&result);
        assert_eq!(label, "Spawned 3 agents");
        assert!(matches!(kind, StatusKind::Success));
    }

    #[test]
    fn launched_partial_uses_x_of_y_label_and_mixed_status() {
        let result = launched_result(vec![
            launched("a", "a-1"),
            failed("b", "boom"),
            launched("c", "a-3"),
        ]);
        let (label, kind) = format_terminal_state(&result);
        assert_eq!(label, "Spawned 2 of 3 agents");
        assert!(matches!(kind, StatusKind::Mixed));
    }

    #[test]
    fn all_failed_uses_failure_status_not_mixed() {
        let result = launched_result(vec![
            failed("a", "boom"),
            failed("b", "boom"),
            failed("c", "boom"),
        ]);
        let (label, kind) = format_terminal_state(&result);
        assert_eq!(label, "Failed to spawn 3 agents");
        assert!(matches!(kind, StatusKind::Failure));
    }

    #[test]
    fn single_failed_uses_singular_failure_label() {
        let result = launched_result(vec![failed("a", "boom")]);
        let (label, kind) = format_terminal_state(&result);
        assert_eq!(label, "Failed to spawn agent");
        assert!(matches!(kind, StatusKind::Failure));
    }

    #[test]
    fn failure_with_error_includes_error_text() {
        let (label, kind) = format_terminal_state(&RunAgentsResult::Failure {
            error: "server rejected request".to_string(),
        });
        assert_eq!(
            label,
            "Failed to start orchestration: server rejected request"
        );
        assert!(matches!(kind, StatusKind::Failure));
    }

    #[test]
    fn failure_with_empty_error_uses_short_label() {
        let (label, kind) = format_terminal_state(&RunAgentsResult::Failure {
            error: String::new(),
        });
        assert_eq!(label, "Failed to start orchestration");
        assert!(matches!(kind, StatusKind::Failure));
    }

    #[test]
    fn denied_with_reason_appends_reason() {
        let (label, kind) = format_terminal_state(&RunAgentsResult::Denied {
            reason: "disapproved".to_string(),
        });
        assert!(label.contains("disapproved"));
        assert!(matches!(kind, StatusKind::Cancelled));
    }

    #[test]
    fn denied_without_reason_uses_short_label() {
        let (label, kind) = format_terminal_state(&RunAgentsResult::Denied {
            reason: String::new(),
        });
        assert!(!label.contains("()"));
        assert!(matches!(kind, StatusKind::Cancelled));
    }

    #[test]
    fn cancelled_uses_cancelled_status() {
        let (label, kind) = format_terminal_state(&RunAgentsResult::Cancelled);
        assert_eq!(label, "Spawn agents cancelled");
        assert!(matches!(kind, StatusKind::Cancelled));
    }
}

mod override_from_approved_config_tests {
    use ai::agent::orchestration_config::OrchestrationConfig;

    use super::super::RunAgentsEditState;
    use super::*;

    fn local_config(model: &str, harness: &str) -> OrchestrationConfig {
        OrchestrationConfig {
            model_id: model.to_string(),
            harness_type: harness.to_string(),
        }
    }

    #[test]
    fn overrides_model_and_harness_unconditionally() {
        let mut state =
            RunAgentsEditState::from_request(&make_request("oz", RunAgentsExecutionMode::Local));
        assert_eq!(state.orchestration_config_state.model_id, "auto");
        assert_eq!(state.orchestration_config_state.harness_type, "oz");

        state
            .orchestration_config_state
            .override_from_approved_config(&local_config("claude-4-opus", "claude"));
        assert_eq!(state.orchestration_config_state.model_id, "claude-4-opus");
        assert_eq!(state.orchestration_config_state.harness_type, "claude");
    }

    #[test]
    fn overrides_even_when_request_has_values() {
        let mut state = RunAgentsEditState::from_request(&make_request(
            "claude",
            RunAgentsExecutionMode::Local,
        ));
        state
            .orchestration_config_state
            .override_from_approved_config(&local_config("gpt-5", "codex"));
        assert_eq!(state.orchestration_config_state.model_id, "gpt-5");
        assert_eq!(state.orchestration_config_state.harness_type, "codex");
    }

    #[test]
    fn approved_local_disabled_harness_reports_disabled_reason_after_override() {
        let mut state =
            RunAgentsEditState::from_request(&make_request("oz", RunAgentsExecutionMode::Local));
        state
            .orchestration_config_state
            .override_from_approved_config(&local_config("auto", "codex"));
        assert_eq!(
            state.orchestration_config_state.accept_disabled_reason(),
            Some("Local Codex child agents are temporarily disabled.")
        );
    }
}

mod is_orphaned_by_finished_output_tests {
    use super::super::is_orphaned_by_finished_output;
    use crate::ai::agent::{AIAgentOutput, CancellationReason, RenderableAIError, Shared};
    use crate::ai::blocklist::action_model::AIActionStatus;
    use crate::ai::blocklist::block::model::AIBlockOutputStatus;

    fn partial_output() -> Shared<AIAgentOutput> {
        Shared::new(AIAgentOutput::default())
    }

    fn cancelled_block() -> AIBlockOutputStatus {
        AIBlockOutputStatus::Cancelled {
            partial_output: Some(partial_output()),
            reason: CancellationReason::ManuallyCancelled,
        }
    }

    #[test]
    fn statusless_action_on_cancelled_block_is_orphaned() {
        assert!(is_orphaned_by_finished_output(None, &cancelled_block()));
    }

    #[test]
    fn statusless_action_on_failed_block_is_orphaned() {
        let failed = AIBlockOutputStatus::Failed {
            partial_output: Some(partial_output()),
            error: RenderableAIError::other("boom", false),
        };
        assert!(is_orphaned_by_finished_output(None, &failed));
    }

    #[test]
    fn statusless_action_on_unfinished_or_successful_block_is_not_orphaned() {
        for block_status in [
            AIBlockOutputStatus::Pending,
            AIBlockOutputStatus::PartiallyReceived {
                output: partial_output(),
            },
            AIBlockOutputStatus::Complete {
                output: partial_output(),
            },
        ] {
            assert!(
                !is_orphaned_by_finished_output(None, &block_status),
                "{block_status:?} should not orphan the card"
            );
        }
    }

    /// An action that reached the queue gets a real result when the
    /// conversation is cancelled, so its own status must keep driving the card.
    #[test]
    fn action_with_status_on_cancelled_block_is_not_orphaned() {
        for action_status in [
            AIActionStatus::Preprocessing,
            AIActionStatus::Queued,
            AIActionStatus::Blocked,
            AIActionStatus::RunningAsync,
        ] {
            assert!(
                !is_orphaned_by_finished_output(Some(&action_status), &cancelled_block()),
                "{action_status:?} should not orphan the card"
            );
        }
    }
}
