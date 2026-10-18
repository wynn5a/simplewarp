use std::collections::HashSet;
use std::io::Write;
use std::time::Duration;

use command::blocking::Command;
use warpui::integration::{AssertionOutcome, TestStep};
use warpui::{SingletonEntity, async_assert};

use crate::BlocklistAIHistoryModel;
use crate::ai::agent::conversation::AIConversation;
use crate::ai::agent::{AIAgentActionType, AIAgentOutputStatus, FinishedAIAgentOutput};
use crate::integration_testing::step::{
    new_step_with_default_assertions, new_step_with_default_assertions_for_pane,
};
use crate::integration_testing::view_getters::terminal_view;

pub const AGENT_MODE_RUNNING_STEP_GROUP_NAME: &str = "Agent mode running";

/// Where `capture_impl_artifacts` persists the implementation conversation's
/// git diff. Falls back to a fixed /tmp path when unset.
pub const IMPL_CODE_DIFF_OUTPUT_FILE_ENV_VAR: &str = "IMPL_CODE_DIFF_OUTPUT_FILE";

/// Assumes that the terminal input is currently not in AI input mode.
pub fn enter_agent_view() -> TestStep {
    new_step_with_default_assertions("Enter Agent View")
        .with_keystrokes(&["ctrl-shift-enter"])
        .add_named_assertion(
            "Assert that we are in Agent View and AI input mode",
            move |app, window_id| {
                let terminal_view = terminal_view(app, window_id, 0, 0);
                terminal_view.read(app, |terminal_view, app| {
                    let is_ai_input_mode = terminal_view
                        .input()
                        .read(app, |input, app| input.input_type(app).is_ai());
                    let transcript_scope = {
                        let model = terminal_view.model.lock();
                        *model.block_list().transcript_scope()
                    };
                    async_assert!(
                        is_ai_input_mode && transcript_scope.is_conversation(),
                        "Expected fullscreen Agent View + AI input mode, got transcript_scope={transcript_scope:?}, is_ai_input_mode={is_ai_input_mode}"
                    )
                })
            },
        )
}

/// Assumes that the terminal input is currently in AI input mode.
pub fn exit_agent_view() -> TestStep {
    new_step_with_default_assertions("Exit Agent View")
        .with_keystrokes(&["escape"])
        .add_named_assertion(
            "Assert that we exited Agent View and are not in AI input mode",
            move |app, window_id| {
                let terminal_view = terminal_view(app, window_id, 0, 0);
                terminal_view.read(app, |terminal_view, app| {
                    let is_ai_input_mode = terminal_view
                        .input()
                        .read(app, |input, app| input.input_type(app).is_ai());
                    let transcript_scope = {
                        let model = terminal_view.model.lock();
                        *model.block_list().transcript_scope()
                    };
                    async_assert!(
                        !is_ai_input_mode && !transcript_scope.is_conversation(),
                        "Expected inactive Agent View + non-AI input mode, got transcript_scope={transcript_scope:?}, is_ai_input_mode={is_ai_input_mode}"
                    )
                })
            },
        )
}

// Runs an AI query without waiting for anything.
// This is useful if you expect a specific sequence of responses (e.g. expect a certain command to be requested immediately),
// since it lets you make assertions on responses as they become ready and fail early instead of waiting for the agent to finish all its turns.
pub fn submit_ai_query(query: &str, timeout: Duration) -> TestStep {
    new_step_with_default_assertions_for_pane(&format!("Enter AI query: {query}"), 0, 0)
        .set_timeout(timeout)
        .set_step_group_name(AGENT_MODE_RUNNING_STEP_GROUP_NAME)
        .with_typed_characters(&[query])
        .with_keystrokes(&["enter"])
        .add_named_assertion(
            "Print conversation ID to stdout",
            print_conversation_id_assertion(),
        )
}

/// Persists the implementation conversation's token usage / cost / exchange
/// count to runtime tags (namespaced `impl.*`) and its `git diff` to a durable
/// file. Must run before the judge starts a new conversation with `/new`,
/// which replaces the active conversation — analytics captured at `on_finish`
/// would then reflect the judge's conversation rather than the implementation.
pub fn capture_impl_artifacts() -> TestStep {
    new_step_with_default_assertions("Capture impl conversation artifacts").add_named_assertion(
        "Persist impl token usage + diff before judge swaps active conversation",
        |app, window_id| {
            let terminal_view = terminal_view(app, window_id, 0, 0);
            let Some(pwd) = terminal_view.read(app, |terminal_view, _| terminal_view.pwd()) else {
                return AssertionOutcome::immediate_failure(
                    "Could not get current directory".to_owned(),
                );
            };

            BlocklistAIHistoryModel::handle(app).update(app, |history_model, _| {
                let Some(conversation) = history_model.active_conversation(terminal_view.id())
                else {
                    return AssertionOutcome::immediate_failure(
                        "No active conversation to capture".to_owned(),
                    );
                };

                super::record_pending_runtime_tag(
                    "impl.total_exchanges",
                    conversation.all_exchanges().len().to_string(),
                );
                if let Some(token) = conversation.server_conversation_token() {
                    super::record_pending_runtime_tag(
                        "impl.conversation_token",
                        token.as_str().to_owned(),
                    );
                }
                // Persist the impl `git diff` to a dedicated file so `on_finish`'s
                // CODE_DIFF_OUTPUT_FILE still works for whichever conversation
                // happens to be active at finish time.
                let impl_diff_path = std::env::var(IMPL_CODE_DIFF_OUTPUT_FILE_ENV_VAR)
                    .unwrap_or_else(|_| "/tmp/impl_code_diff.txt".to_owned());
                let edited_files = collect_edited_files(conversation);
                let Ok(mut file) = std::fs::File::create(&impl_diff_path) else {
                    return AssertionOutcome::immediate_failure(format!(
                        "Failed to create impl diff file {impl_diff_path}"
                    ));
                };
                if edited_files.is_empty() {
                    let _ = writeln!(file, "No files were edited by the impl conversation");
                } else {
                    for file_name in edited_files {
                        let output = Command::new("git")
                            .args(["diff", "--", &file_name])
                            .current_dir(&pwd)
                            .output();
                        super::write_git_diff_output_to_file(output, &mut file, &file_name);
                    }
                }

                AssertionOutcome::Success
            })
        },
    )
}

/// Walks the conversation's exchanges and returns the unique set of files the
/// agent requested edits for. Mirrors the corresponding loop inside
/// `output_code_diff_debug_info`.
fn collect_edited_files(conversation: &AIConversation) -> HashSet<String> {
    let mut edited_files = HashSet::new();
    for exchange in conversation.all_exchanges() {
        let AIAgentOutputStatus::Finished { finished_output } = &exchange.output_status else {
            continue;
        };
        let FinishedAIAgentOutput::Success { output } = finished_output else {
            continue;
        };
        for action in output.get().actions() {
            if let AIAgentActionType::RequestFileEdits { file_edits, .. } = &action.action {
                for edit in file_edits {
                    if let Some(file) = edit.file() {
                        edited_files.insert(file.to_owned());
                    }
                }
            }
        }
    }
    edited_files
}

/// Returns an assertion that prints the conversation ID to stdout once available.
/// This assertion will poll until the conversation token is received from the server.
fn print_conversation_id_assertion()
-> impl FnMut(&mut warpui::App, warpui::WindowId) -> warpui::integration::AssertionOutcome {
    |app, window_id| {
        let terminal_view = terminal_view(app, window_id, 0, 0);
        BlocklistAIHistoryModel::handle(app).read(app, |history_model, _| {
            if let Some(conversation) = history_model.active_conversation(terminal_view.id())
                && let Some(token) = conversation.server_conversation_token()
            {
                println!("Conversation ID: {}", token.as_str());
                return AssertionOutcome::Success;
            }
            // If we don't have a conversation token yet, keep polling
            AssertionOutcome::failure("Waiting for conversation token to be available".to_string())
        })
    }
}
