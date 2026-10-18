// `panic!` causes the app to crash before debug info can be exported. Use `AssertionOutcome::immediate_failure` instead.
#![deny(clippy::panic)]
// `assert!` causes the app to crash before debug info can be exported. Use `integration_assert!` instead.
#![deny(clippy::assertions_on_constants)]

use warpui::integration::{AssertionCallback, AssertionOutcome};
use warpui::{EntityId, SingletonEntity as _, integration_assert};

use crate::BlocklistAIHistoryModel;
use crate::ai::agent::conversation::{AIConversation, AIConversationId, ConversationStatus};
use crate::ai::agent::{
    AIAgentActionType, AIAgentExchange, AIAgentOutputMessageType, AIAgentOutputStatus,
    AIAgentTextSection, FinishedAIAgentOutput,
};
use crate::integration_testing::view_getters::terminal_view;

type TextAssertion = Box<dyn Fn(&str) -> bool + 'static>;
type ActionAssertion = Box<dyn Fn(&AIAgentActionType) -> bool + 'static>;

// Make an assertion on the text output in the latest exchange.
// This assumes the AI block has completed successfully.
pub fn assert_latest_exchange_text(
    assertion: impl Fn(&str) -> bool + 'static + Clone,
) -> AssertionCallback {
    Box::new(move |app, window_id| {
        let terminal_view = terminal_view(app, window_id, 0, 0);
        BlocklistAIHistoryModel::handle(app).update(app, |history_model, _| {
            let exchange_count = get_exchange_count(terminal_view.id(), history_model);
            exchange_succeeds_with_expected_output(
                Some(Box::new(assertion.clone())),
                None,
                ConversationTarget::Active,
                terminal_view.id(),
                exchange_count - 1,
                history_model,
            )
        })
    })
}

/// Specifies which conversations to check for task completion.
#[derive(Debug, Clone, Copy)]
pub enum ConversationTarget {
    /// Check only the active conversation in the terminal view
    Active,
    /// Get the only conversation in this terminal view (active/passive) or throw if multiple conversations were found
    Only,
    /// Check a specific conversation by ID in the terminal view
    ById(AIConversationId),
}

/// Check if a conversation has ended with an API error in its latest exchange.
fn check_for_api_error_in_latest_exchange(
    conversation: &AIConversation,
) -> Option<AssertionOutcome> {
    if let Some(latest_exchange) = conversation.latest_exchange()
        && let AIAgentOutputStatus::Finished {
            finished_output: FinishedAIAgentOutput::Error { error, .. },
        } = &latest_exchange.output_status
    {
        return Some(AssertionOutcome::immediate_failure(format!(
            "Conversation ended with API error: {error:?}"
        )));
    }
    None
}

fn get_conversation(
    conversation_target: ConversationTarget,
    terminal_view_id: EntityId,
    history_model: &BlocklistAIHistoryModel,
) -> Result<&AIConversation, AssertionOutcome> {
    match &conversation_target {
        ConversationTarget::Active => {
            if let Some(active_conv) = history_model.active_conversation(terminal_view_id) {
                Ok(active_conv)
            } else {
                Err(AssertionOutcome::failure(
                    "No active conversation".to_owned(),
                ))
            }
        }
        ConversationTarget::Only => {
            // Get all conversations (including passive ones)
            let mut conversations: Vec<_> = history_model
                .all_live_conversations_for_terminal_surface(terminal_view_id)
                .collect();
            match conversations.len() {
                1 => conversations.pop().ok_or(AssertionOutcome::failure(
                    "Expected one conversation, but vector was empty".to_string(),
                )),
                0 => Err(AssertionOutcome::failure(
                    "No conversations were found".to_string(),
                )),
                _ => Err(AssertionOutcome::failure(
                    "Multiple conversations were found".to_string(),
                )),
            }
        }
        ConversationTarget::ById(conversation_id) => {
            if let Some(conversation) = history_model.conversation(conversation_id) {
                Ok(conversation)
            } else {
                Err(AssertionOutcome::failure(format!(
                    "Conversation with ID {conversation_id:?} not found in terminal view"
                )))
            }
        }
    }
}

/// Fails immediately on any error response.
pub fn assert_latest_task_is_done(
    max_number_of_turns: Option<usize>,
    conversation_target: ConversationTarget,
    match_fn: impl Fn(&ConversationStatus) -> bool + 'static,
) -> AssertionCallback {
    Box::new(move |app, window_id| {
        let terminal_view = terminal_view(app, window_id, 0, 0);
        BlocklistAIHistoryModel::handle(app).update(app, |history_model, _| {
            let conversation = match get_conversation(conversation_target, terminal_view.id(), history_model) {
                Ok(conversation) => conversation,
                Err(assertion) => return assertion,
            };

            if let Some(max_number_of_turns) = max_number_of_turns {
                integration_assert!(
                    conversation.all_exchanges().len() <= max_number_of_turns,
                    "Conversation exceeded max number of turns before task succeeded: {max_number_of_turns}"
                );
            }

            // Check if the latest exchange ended with an API error
            if let Some(api_error_outcome) = check_for_api_error_in_latest_exchange(conversation) {
                return api_error_outcome;
            }

            // If the task is done and matches our criteria, return success
            if !conversation.status().is_in_progress() {
                let status = conversation.status();
                if match_fn(status) {
                    return AssertionOutcome::Success;
                } else {
                    return AssertionOutcome::failure(format!(
                        "Conversation is done but does not match expected status - is {status:?}",
                    ));
                }
            }

            // If we get here, no conversations had completed tasks matching our criteria
            AssertionOutcome::failure("Task is not done in target conversation".to_owned())
        })
    })
}

/// Assert that the exchange at exchange_index finishes successfully.
/// Optionally make assertions on the text and action of the output.
fn exchange_succeeds_with_expected_output(
    text_assertion: Option<TextAssertion>,
    action_assertion: Option<ActionAssertion>,
    conversation_target: ConversationTarget,
    terminal_view_id: EntityId,
    exchange_index: usize,
    history_model: &BlocklistAIHistoryModel,
) -> AssertionOutcome {
    match get_exchange_output(
        conversation_target,
        terminal_view_id,
        exchange_index,
        history_model,
    ) {
        Ok((text, action_type)) => {
            if let Some(text_assertion) = &text_assertion {
                integration_assert!(
                    text_assertion(&text),
                    "Exchange output text does not match assertion: {text}"
                );
            }
            if let Some(action_assertion) = &action_assertion {
                let Some(action_type) = action_type else {
                    return AssertionOutcome::immediate_failure(
                        "Exchange output has no actions".to_owned(),
                    );
                };
                integration_assert!(
                    action_assertion(&action_type),
                    "Exchange action does not match assertion: {action_type:?}"
                );
            }
            AssertionOutcome::Success
        }
        Err(outcome) => outcome,
    }
}

/// Get the text and action type of the exchange at exchange_index.
/// Returns an error if the exchange is not finished.
fn get_exchange_output(
    conversation_target: ConversationTarget,
    terminal_view_id: EntityId,
    exchange_index: usize,
    history_model: &BlocklistAIHistoryModel,
) -> Result<(String, Option<AIAgentActionType>), AssertionOutcome> {
    let exchange = get_exchange_index_from_conversation(
        conversation_target,
        terminal_view_id,
        exchange_index,
        history_model,
    )?;

    let AIAgentOutputStatus::Finished { finished_output } = &exchange.output_status else {
        return Err(AssertionOutcome::failure(
            "Exchange is not finished".to_owned(),
        ));
    };

    // Once the output is finished, we make plain assertions to immediately fail the test on failure, instead
    // of waiting for something to change.
    match finished_output {
        FinishedAIAgentOutput::Success { output } => {
            let ai_output = output.get();
            let text = ai_output
                .messages
                .iter()
                .filter_map(|message| match &message.message {
                    AIAgentOutputMessageType::Text(text) => Some(text),
                    AIAgentOutputMessageType::Summarization { text, .. } => Some(text),
                    _ => None,
                })
                .flat_map(|text| text.sections.iter())
                .flat_map(|section| match section {
                    AIAgentTextSection::PlainText { text } => Some(text.text().to_string()),
                    AIAgentTextSection::Code { code, language, .. } => {
                        let language_str = language
                            .as_ref()
                            .map_or("text".to_string(), |lang| lang.to_string());
                        Some(format!("```{language_str}\n{code}\n```"))
                    }
                    AIAgentTextSection::Table { table } => Some(table.markdown_source.clone()),
                    AIAgentTextSection::Image { image } => Some(image.markdown_source.clone()),
                    AIAgentTextSection::MermaidDiagram { diagram } => {
                        Some(diagram.markdown_source.clone())
                    }
                })
                .collect::<Vec<_>>()
                .join("");
            let action_type = ai_output
                .actions()
                .next()
                .map(|action| action.action.clone());

            Ok((text, action_type))
        }
        FinishedAIAgentOutput::Error { error, .. } => Err(AssertionOutcome::immediate_failure(
            format!("Exchange failed with error: {error:?}"),
        )),
        FinishedAIAgentOutput::Cancelled { .. } => Err(AssertionOutcome::immediate_failure(
            "Exchange was cancelled".to_owned(),
        )),
    }
}

fn get_exchange_index_from_conversation(
    conversation_target: ConversationTarget,
    terminal_view_id: EntityId,
    exchange_index: usize,
    history_model: &BlocklistAIHistoryModel,
) -> Result<&AIAgentExchange, AssertionOutcome> {
    // Async assertions to wait until this exchange is finished.
    let conversation = get_conversation(conversation_target, terminal_view_id, history_model)?;
    match conversation.root_task_exchanges().nth(exchange_index) {
        Some(res) => Ok(res),
        None => {
            // If the exchange doesn't exist and the task is done (no more exchanges will be created), immediately fail
            if !conversation.status().is_in_progress() {
                return Err(AssertionOutcome::immediate_failure(format!(
                    "Exchange at index {exchange_index} does not exist and task is done"
                )));
            };
            // Otherwise, async assertion to wait for the exchange to be created.
            Err(AssertionOutcome::failure(format!(
                "Failed to get exchange at index {exchange_index}"
            )))
        }
    }
}

fn get_exchange_count(
    terminal_view_id: EntityId,
    history_model: &BlocklistAIHistoryModel,
) -> usize {
    history_model
        .active_conversation(terminal_view_id)
        .map_or(0, |c| c.root_task_exchanges().count())
}
