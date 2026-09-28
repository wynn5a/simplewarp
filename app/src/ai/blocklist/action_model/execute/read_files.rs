use std::path::{Path, PathBuf};

use futures::FutureExt;
use futures::future::BoxFuture;
use warpui::{Entity, EntityId, ModelContext, ModelHandle, SingletonEntity};

use super::{
    ActionExecution, AnyActionExecution, ExecuteActionInput, PreprocessActionInput,
    describe_failed_files, read_local_file_context,
};
use crate::ai::agent::{
    AIAgentAction, AIAgentActionResultType, AIAgentActionType, ReadFilesRequest, ReadFilesResult,
};
use crate::ai::blocklist::BlocklistAIPermissions;
use crate::ai::paths::host_native_absolute_path;
use crate::terminal::model::session::SessionType;
use crate::terminal::model::session::active_session::ActiveSession;

pub struct ReadFilesExecutor {
    active_session: ModelHandle<ActiveSession>,
    terminal_view_id: EntityId,
}

impl ReadFilesExecutor {
    pub fn new(active_session: ModelHandle<ActiveSession>, terminal_view_id: EntityId) -> Self {
        Self {
            active_session,
            terminal_view_id,
        }
    }

    pub(super) fn should_autoexecute(
        &self,
        input: ExecuteActionInput,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        let ExecuteActionInput {
            action:
                AIAgentAction {
                    action: AIAgentActionType::ReadFiles(ReadFilesRequest { locations }),
                    ..
                },
            conversation_id,
        } = input
        else {
            return false;
        };

        // TODO: figure out how to avoid constructing the full paths in `should_execute`
        // and then again in `execute`, and then again on every render.
        let current_working_directory = self
            .active_session
            .as_ref(ctx)
            .current_working_directory()
            .cloned();
        let shell = self.active_session.as_ref(ctx).shell_launch_data(ctx);

        BlocklistAIPermissions::as_ref(ctx)
            .can_read_files_with_conversation(
                &conversation_id,
                locations
                    .iter()
                    .map(|file| {
                        PathBuf::from(host_native_absolute_path(
                            &file.name,
                            &shell,
                            &current_working_directory,
                        ))
                    })
                    .collect(),
                Some(self.terminal_view_id),
                ctx,
            )
            .is_allowed()
    }

    pub(super) fn execute(
        &mut self,
        input: ExecuteActionInput,
        ctx: &mut ModelContext<Self>,
    ) -> impl Into<AnyActionExecution> + use<> {
        let ExecuteActionInput {
            action,
            conversation_id,
            ..
        } = input;
        let AIAgentAction {
            action: AIAgentActionType::ReadFiles(ReadFilesRequest { locations }),
            ..
        } = action
        else {
            return ActionExecution::InvalidAction;
        };

        BlocklistAIPermissions::handle(ctx).update(ctx, |model, _ctx| {
            model.add_temporary_file_read_permissions(
                conversation_id,
                locations.iter().map(|file| Path::new(&file.name)),
            );
        });

        let current_working_directory = self
            .active_session
            .as_ref(ctx)
            .current_working_directory()
            .cloned();
        let shell = self.active_session.as_ref(ctx).shell_launch_data(ctx);

        let locations = locations.clone();

        // File reading requires local access, which remote sessions don't have.
        if matches!(
            self.active_session.as_ref(ctx).session_type(ctx),
            Some(SessionType::WarpifiedRemote)
        ) {
            return ActionExecution::Sync(AIAgentActionResultType::ReadFiles(
                ReadFilesResult::Error(
                    "The file read/edit tool is not available on this remote session. \
                     Try using a different tool."
                        .to_string(),
                ),
            ));
        }

        // Local path.
        ActionExecution::Async {
            execute_future: Box::pin(async move {
                let result = read_local_file_context(
                    &locations,
                    current_working_directory,
                    shell,
                    None,
                    None,
                )
                .await?;
                if result.failed_files.is_empty() {
                    Ok(ReadFilesResult::Success {
                        files: result.file_contexts,
                        failed_files: Vec::new(),
                    })
                } else if result.file_contexts.is_empty() {
                    let failed_files = describe_failed_files(&result.failed_files);
                    Ok(ReadFilesResult::Error(format!(
                        "Failed to read files: {failed_files}"
                    )))
                } else {
                    Ok(ReadFilesResult::Success {
                        files: result.file_contexts,
                        failed_files: result.failed_files,
                    })
                }
            }),
            on_complete: Box::new(|res: Result<ReadFilesResult, anyhow::Error>, _ctx| {
                let action_result = res.unwrap_or_else(|e| ReadFilesResult::Error(e.to_string()));
                AIAgentActionResultType::ReadFiles(action_result)
            }),
        }
    }

    pub(super) fn preprocess_action(
        &mut self,
        _input: PreprocessActionInput,
        _ctx: &mut ModelContext<Self>,
    ) -> BoxFuture<'static, ()> {
        futures::future::ready(()).boxed()
    }
}

impl Entity for ReadFilesExecutor {
    type Event = ();
}
