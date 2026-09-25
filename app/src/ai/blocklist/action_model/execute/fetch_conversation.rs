use ai::agent::action_result::FetchConversationResult;
use futures::FutureExt;
use futures::future::BoxFuture;
use warp_errors::report_error;
use warpui::{Entity, ModelContext, SingletonEntity};

use super::{ActionExecution, AnyActionExecution, ExecuteActionInput, PreprocessActionInput};
use crate::BlocklistAIHistoryModel;
use crate::ai::agent::api::ServerConversationToken;
use crate::ai::agent::conversation::AIConversation;
use crate::ai::agent::{AIAgentActionResultType, AIAgentActionType, conversation_yaml};

pub struct FetchConversationExecutor;

impl FetchConversationExecutor {
    pub fn new() -> Self {
        Self
    }

    pub(super) fn should_autoexecute(
        &self,
        _input: ExecuteActionInput,
        _ctx: &mut ModelContext<Self>,
    ) -> bool {
        true
    }

    pub(super) fn execute(
        &mut self,
        input: ExecuteActionInput,
        ctx: &mut ModelContext<Self>,
    ) -> impl Into<AnyActionExecution> + use<> {
        let ExecuteActionInput { action, .. } = input;
        let AIAgentActionType::FetchConversation { conversation_id } = &action.action else {
            return ActionExecution::<Option<Box<AIConversation>>>::InvalidAction;
        };

        let conversation_id = conversation_id.clone();
        let server_token = ServerConversationToken::new(conversation_id.clone());

        let load_future = BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, _| {
            history.load_conversation_by_server_token(&server_token)
        });

        ActionExecution::new_async(load_future, move |cloud_conversation, _ctx| {
            materialize_conversation(cloud_conversation, &conversation_id)
        })
    }

    pub(super) fn preprocess_action(
        &mut self,
        _input: PreprocessActionInput,
        _ctx: &mut ModelContext<Self>,
    ) -> BoxFuture<'static, ()> {
        futures::future::ready(()).boxed()
    }
}

/// Materializes a loaded conversation's tasks into YAML files on disk.
fn materialize_conversation(
    conversation: Option<Box<AIConversation>>,
    server_conversation_id: &str,
) -> AIAgentActionResultType {
    let Some(conversation) = conversation else {
        log::warn!("FetchConversation: failed to load conversation {server_conversation_id}");
        return AIAgentActionResultType::FetchConversation(FetchConversationResult::Error(
            format!("Failed to load conversation {server_conversation_id}"),
        ));
    };

    let tasks: Vec<warp_multi_agent_api::Task> = conversation
        .all_tasks()
        .filter_map(|task| task.source().cloned())
        .collect();
    log::info!(
        "FetchConversation: materializing {} tasks for conversation {server_conversation_id}",
        tasks.len(),
    );
    match conversation_yaml::materialize_tasks_to_yaml(&tasks) {
        Ok(directory_path) => {
            log::info!(
                "FetchConversation: wrote YAML to {directory_path} \
                 for conversation {server_conversation_id}"
            );
            AIAgentActionResultType::FetchConversation(FetchConversationResult::Success {
                directory_path,
            })
        }
        Err(e) => {
            report_error!(
                anyhow::anyhow!("{e}").context("FetchConversation: failed to materialize YAML")
            );
            AIAgentActionResultType::FetchConversation(FetchConversationResult::Error(format!(
                "Failed to materialize conversation: {e}"
            )))
        }
    }
}

impl Entity for FetchConversationExecutor {
    type Event = ();
}
