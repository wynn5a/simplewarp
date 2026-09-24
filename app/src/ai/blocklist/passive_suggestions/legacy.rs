use std::path::PathBuf;
#[cfg(not(target_family = "wasm"))]
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

#[cfg(not(target_family = "wasm"))]
use command::r#async::Command;
use parking_lot::FairMutex;
use serde_json::json;
use warpui::r#async::{FutureExt as AsyncFutureExt, SpawnedFutureHandle, Timer};
use warpui::{Entity, EntityId, ModelContext, ModelHandle, SingletonEntity};

use super::static_prompt_suggestions::static_suggested_query;
use crate::ai::agent::CancellationReason;
#[cfg(not(target_family = "wasm"))]
use crate::ai::agent::PassiveSuggestionTrigger;
use crate::ai::blocklist::controller::response_stream::ResponseStreamId;
use crate::ai::blocklist::controller::{BlocklistAIController, BlocklistAIControllerEvent};
use crate::ai::blocklist::{BlocklistAIPermissions, read_local_file_context};
use crate::ai::paths::host_native_absolute_path;
use crate::network::NetworkStatus;
use crate::safe_warn;
use crate::settings::AISettings;
use crate::terminal::event::{BlockType, UserBlockCompleted};
use crate::terminal::model::block::BlockId;
use crate::terminal::model::session::SessionType;
use crate::terminal::model::session::active_session::ActiveSession;
use crate::terminal::model::terminal_model::TerminalModel;
use crate::terminal::model_events::{ModelEvent, ModelEventDispatcher};
use crate::terminal::view::PromptSuggestion;
use crate::workspaces::user_workspaces::UserWorkspaces;

const PASSIVE_CODE_DIFF_LONG_FILE_LINE_LIMIT: usize = 2000;
const PASSIVE_CODE_DIFF_LONG_FILE_BYTE_LIMIT: usize = 100_000;
const PASSIVE_CODE_DIFF_TOTAL_LINE_LIMIT: usize = 2500;
const PASSIVE_CODE_DIFF_TOTAL_BYTE_LIMIT: usize = 150_000;
const PASSIVE_CODE_DIFF_FILE_READING_TIMEOUT: Duration = Duration::from_secs(2);
const PASSIVE_CODE_DIFF_AI_QUERY_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Clone, Debug)]
pub enum PassiveSuggestionsEvent {
    PromptSuggestionsGenerated {
        prompt_suggestion: PromptSuggestion,
        block_id: BlockId,
    },
    PassiveCodeDiffFailed,
}

pub struct PassiveSuggestionsModel {
    active_session: ModelHandle<ActiveSession>,
    terminal_model: Arc<FairMutex<TerminalModel>>,
    ai_controller: ModelHandle<BlocklistAIController>,
    terminal_view_id: EntityId,
    unit_test_generation_future_handle: Option<SpawnedFutureHandle>,
    code_diff_preflight_future_handle: Option<SpawnedFutureHandle>,
    code_diff_timeout_future_handle: Option<SpawnedFutureHandle>,
    pending_unit_test_stream_id: Option<ResponseStreamId>,
    pending_code_diff_stream_id: Option<ResponseStreamId>,
}

impl PassiveSuggestionsModel {
    pub fn new(
        active_session: ModelHandle<ActiveSession>,
        terminal_model: Arc<FairMutex<TerminalModel>>,
        ai_controller: ModelHandle<BlocklistAIController>,
        model_event_dispatcher: &ModelHandle<ModelEventDispatcher>,
        terminal_view_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        ctx.subscribe_to_model(model_event_dispatcher, |me, _, event, ctx| {
            me.handle_model_event(event, ctx);
        });
        ctx.subscribe_to_model(&ai_controller, |me, _, event, _ctx| {
            me.handle_controller_event(event, _ctx);
        });

        Self {
            active_session,
            terminal_model,
            ai_controller,
            terminal_view_id,
            unit_test_generation_future_handle: None,
            code_diff_preflight_future_handle: None,
            code_diff_timeout_future_handle: None,
            pending_unit_test_stream_id: None,
            pending_code_diff_stream_id: None,
        }
    }

    pub fn is_passive_code_diff_being_generated(&self) -> bool {
        self.pending_code_diff_stream_id.is_some()
    }

    pub fn abort_pending_requests(
        &mut self,
        ctx: &mut ModelContext<Self>,
    ) -> Vec<ResponseStreamId> {
        let mut aborted_stream_ids = Vec::new();
        if let Some(handle) = self.unit_test_generation_future_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.code_diff_preflight_future_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.code_diff_timeout_future_handle.take() {
            handle.abort();
        }
        if let Some(stream_id) = self.pending_unit_test_stream_id.take() {
            self.ai_controller.update(ctx, |controller, ctx| {
                controller.try_cancel_pending_response_stream(
                    &stream_id,
                    CancellationReason::ManuallyCancelled,
                    ctx,
                );
            });
            aborted_stream_ids.push(stream_id);
        }
        if let Some(stream_id) = self.pending_code_diff_stream_id.take() {
            self.ai_controller.update(ctx, |controller, ctx| {
                controller.try_cancel_pending_response_stream(
                    &stream_id,
                    CancellationReason::ManuallyCancelled,
                    ctx,
                );
            });
            aborted_stream_ids.push(stream_id);
        }
        aborted_stream_ids
    }

    fn handle_model_event(&mut self, event: &ModelEvent, ctx: &mut ModelContext<Self>) {
        match event {
            ModelEvent::AfterBlockStarted { .. } => {
                self.abort_pending_requests(ctx);
            }
            ModelEvent::AfterBlockCompleted(after_block_completed_event) => {
                let BlockType::User(block_completed) = &after_block_completed_event.block_type
                else {
                    return;
                };
                self.handle_user_block_completed(block_completed, ctx);
            }
            _ => {}
        }
    }

    fn handle_controller_event(
        &mut self,
        event: &BlocklistAIControllerEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        match event {
            BlocklistAIControllerEvent::FinishedReceivingOutput { stream_id, .. } => {
                if self
                    .pending_unit_test_stream_id
                    .as_ref()
                    .is_some_and(|pending| pending == stream_id)
                {
                    self.pending_unit_test_stream_id = None;
                }
                if self
                    .pending_code_diff_stream_id
                    .as_ref()
                    .is_some_and(|pending| pending == stream_id)
                {
                    self.pending_code_diff_stream_id = None;
                    if let Some(handle) = self.code_diff_timeout_future_handle.take() {
                        handle.abort();
                    }
                }
            }
            BlocklistAIControllerEvent::SentRequest { stream_id, .. } => {
                if self.pending_unit_test_stream_id.as_ref() == Some(stream_id)
                    || self.pending_code_diff_stream_id.as_ref() == Some(stream_id)
                    || self.pending_code_diff_stream_id.as_ref() == Some(stream_id)
                {
                    return;
                }
                // If a new request is sent that wasn't for passive suggestions, abort any passive suggestions requests.
                self.abort_pending_requests(ctx);
            }
            _ => {}
        }
    }

    fn handle_user_block_completed(
        &mut self,
        block_completed: &UserBlockCompleted,
        ctx: &mut ModelContext<Self>,
    ) {
        if block_completed.was_part_of_agent_interaction {
            return;
        }

        self.abort_pending_requests(ctx);

        // Startup commands run while bootstrapping an Oz cloud environment, so we skip
        // passive prompt suggestion generation for them to avoid unnecessary requests.
        let is_oz_environment_startup_command = self
            .terminal_model
            .lock()
            .block_list()
            .block_at(block_completed.index)
            .is_some_and(|block| block.is_oz_environment_startup_command());
        if is_oz_environment_startup_command {
            return;
        }

        if should_generate_unit_test_suggestion(block_completed, ctx) {
            self.generate_unit_test_suggestion(block_completed.clone(), ctx);
        } else if should_generate_prompt_suggestions(block_completed, ctx) {
            self.generate_prompt_suggestions(block_completed.clone(), ctx);
        }
    }

    fn generate_prompt_suggestions(
        &mut self,
        block_completed: UserBlockCompleted,
        ctx: &mut ModelContext<Self>,
    ) {
        let block_id = block_completed.serialized_block.id.clone();

        // The only prompt suggestions left come from the static local table, which needs
        // nothing but the block that just finished. Warp's server used to serve richer
        // suggestions here; with it gone there is no other source, so without a static
        // suggestion there is nothing to emit.
        if let Some(suggestion) = fetch_static_prompt_suggestion(&block_completed) {
            ctx.emit(PassiveSuggestionsEvent::PromptSuggestionsGenerated {
                prompt_suggestion: suggestion.clone(),
                block_id: block_id.clone(),
            });
            self.maybe_generate_passive_code_diff(suggestion, block_id, ctx);
        }
    }

    fn generate_unit_test_suggestion(
        &mut self,
        block_completed: UserBlockCompleted,
        ctx: &mut ModelContext<Self>,
    ) {
        #[cfg(target_family = "wasm")]
        {
            let (_, _) = (block_completed, ctx);
        }

        #[cfg(not(target_family = "wasm"))]
        {
            let Some(current_dir) = block_completed
                .serialized_block
                .pwd
                .as_ref()
                .map(PathBuf::from)
            else {
                return;
            };

            self.unit_test_generation_future_handle = Some(ctx.spawn(
                async move {
                    let output = Command::new("git")
                        .args(["show", "HEAD"])
                        .current_dir(current_dir)
                        .stdout(Stdio::piped())
                        .output()
                        .await;
                    if let Ok(output) = output {
                        return String::from_utf8_lossy(&output.stdout).to_string();
                    }
                    String::new()
                },
                |me, diff_output: String, ctx| {
                    me.unit_test_generation_future_handle = None;
                    if diff_output.is_empty() {
                        return;
                    }
                    let diff_json = json!({ "diffs": diff_output });
                    let request = me.ai_controller.update(ctx, |controller, ctx| {
                        controller.send_unit_test_suggestions_request(
                            diff_json.to_string(),
                            PassiveSuggestionTrigger::CommandRun,
                            ctx,
                        )
                    });
                    if let Ok((_, stream_id)) = request {
                        me.pending_unit_test_stream_id = Some(stream_id.clone());
                    }
                },
            ));
        }
    }

    fn maybe_generate_passive_code_diff(
        &mut self,
        prompt_suggestion: PromptSuggestion,
        block_id: BlockId,
        ctx: &mut ModelContext<Self>,
    ) {
        if !passive_code_diffs_enabled(ctx) {
            return;
        }
        let query = prompt_suggestion;
        let Some(files) = query.coding_query_context.clone() else {
            return;
        };

        let current_working_directory = self
            .active_session
            .as_ref(ctx)
            .current_working_directory()
            .cloned();
        let shell = self.active_session.as_ref(ctx).shell_launch_data(ctx);

        let can_read_file = BlocklistAIPermissions::as_ref(ctx)
            .can_read_files(
                None,
                files
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
            .is_allowed();
        let should_skip_for_remote = self
            .active_session
            .as_ref(ctx)
            .session_type(ctx)
            .map(|session_type| matches!(session_type, SessionType::WarpifiedRemote { .. }))
            .unwrap_or(true);
        if !can_read_file || should_skip_for_remote {
            ctx.emit(PassiveSuggestionsEvent::PassiveCodeDiffFailed);
            return;
        }

        let query_text = query.prompt;
        self.code_diff_preflight_future_handle = Some(ctx.spawn(
            async move {
                let file_future =
                    read_local_file_context(&files, current_working_directory, shell, None, None);
                let Ok(result) = file_future.with_timeout(PASSIVE_CODE_DIFF_FILE_READING_TIMEOUT).await
                else {
                    return Err(anyhow::anyhow!("File reading timed out"));
                };
                result
            },
            move |me, content, ctx| {
                me.code_diff_preflight_future_handle = None;

                let content = match content {
                    Ok(content) => {
                        if !content.failed_files.is_empty() {
                            safe_warn!(
                                safe: (
                                    "Failed to read {} file(s) when retrieving content for suggested code diffs",
                                    content.failed_files.len()
                                ),
                                full: (
                                    "Failed to read files when retrieving content for suggested code diffs: {:?}",
                                    content.failed_files
                                )
                            );
                            ctx.emit(PassiveSuggestionsEvent::PassiveCodeDiffFailed);
                            return;
                        }
                        content
                    }
                    Err(err) => {
                        log::warn!("Failed to retrieve file content for suggested code diffs: {err}");
                        ctx.emit(PassiveSuggestionsEvent::PassiveCodeDiffFailed);
                        return;
                    }
                };

                let mut total_lines = 0;
                let mut total_bytes = 0;
                let has_large_file = content.file_contexts.iter().any(|file_context| {
                    let file_content = &file_context.content;
                    let line_count = file_content.line_count();
                    let byte_count = file_content.len();

                    total_lines += line_count;
                    total_bytes += byte_count;

                    line_count >= PASSIVE_CODE_DIFF_LONG_FILE_LINE_LIMIT
                        || byte_count >= PASSIVE_CODE_DIFF_LONG_FILE_BYTE_LIMIT
                });
                if has_large_file
                    || total_lines >= PASSIVE_CODE_DIFF_TOTAL_LINE_LIMIT
                    || total_bytes >= PASSIVE_CODE_DIFF_TOTAL_BYTE_LIMIT
                {
                    ctx.emit(PassiveSuggestionsEvent::PassiveCodeDiffFailed);
                    return;
                }

                let result = me.ai_controller.update(ctx, |controller, ctx| {
                    controller.send_passive_code_diff_request(
                        query_text,
                        &block_id,
                        content.file_contexts,
                        ctx,
                    )
                });

                match result {
                    Ok((_, stream_id)) => {
                        me.pending_code_diff_stream_id = Some(stream_id.clone());
                        me.start_code_diff_timeout(stream_id, ctx);
                    }
                    Err(_) => {
                        ctx.emit(PassiveSuggestionsEvent::PassiveCodeDiffFailed);
                    }
                }
            },
        ));
    }

    fn start_code_diff_timeout(
        &mut self,
        stream_id: ResponseStreamId,
        ctx: &mut ModelContext<Self>,
    ) {
        if let Some(handle) = self.code_diff_timeout_future_handle.take() {
            handle.abort();
        }
        self.code_diff_timeout_future_handle = Some(ctx.spawn(
            async move {
                Timer::after(PASSIVE_CODE_DIFF_AI_QUERY_TIMEOUT).await;
                stream_id
            },
            |me, timed_out_stream_id, ctx| {
                me.code_diff_timeout_future_handle = None;
                if me
                    .pending_code_diff_stream_id
                    .as_ref()
                    .is_some_and(|pending| pending == &timed_out_stream_id)
                {
                    log::warn!(
                        "Passive code diff AI request timed out, cancelling stream {timed_out_stream_id:?}"
                    );
                    me.ai_controller.update(ctx, |controller, ctx| {
                        controller.try_cancel_pending_response_stream(
                            &timed_out_stream_id,
                            CancellationReason::ManuallyCancelled,
                            ctx,
                        );
                    });
                    me.pending_code_diff_stream_id = None;
                    ctx.emit(PassiveSuggestionsEvent::PassiveCodeDiffFailed);
                }
            },
        ));
    }
}

impl Entity for PassiveSuggestionsModel {
    type Event = PassiveSuggestionsEvent;
}

fn should_generate_prompt_suggestions(
    block_completed: &UserBlockCompleted,
    ctx: &ModelContext<PassiveSuggestionsModel>,
) -> bool {
    if block_completed.command.trim().is_empty() {
        return false;
    }
    if !NetworkStatus::as_ref(ctx).is_online() {
        return false;
    }

    AISettings::as_ref(ctx).is_prompt_suggestions_enabled(ctx)
        && UserWorkspaces::as_ref(ctx).is_prompt_suggestions_toggleable()
}

fn should_generate_unit_test_suggestion(
    block_completed: &UserBlockCompleted,
    ctx: &ModelContext<PassiveSuggestionsModel>,
) -> bool {
    let enabled = AISettings::as_ref(ctx).is_code_suggestions_enabled(ctx)
        && UserWorkspaces::as_ref(ctx).is_code_suggestions_toggleable();

    enabled
        && block_completed.command.starts_with("git")
        && block_completed.command.contains("commit")
        && block_completed.serialized_block.exit_code.was_successful()
}

fn passive_code_diffs_enabled(ctx: &ModelContext<PassiveSuggestionsModel>) -> bool {
    let ai_settings = AISettings::as_ref(ctx);
    let is_prompt_suggestions_enabled = ai_settings.is_prompt_suggestions_enabled(ctx);
    let is_code_suggestions_enabled = ai_settings.is_code_suggestions_enabled(ctx);
    let is_toggleable = UserWorkspaces::as_ref(ctx).is_code_suggestions_toggleable();
    is_prompt_suggestions_enabled && is_code_suggestions_enabled && is_toggleable
}

fn fetch_static_prompt_suggestion(block: &UserBlockCompleted) -> Option<PromptSuggestion> {
    if !block.serialized_block.exit_code.was_successful() {
        return None;
    }
    static_suggested_query(&block.command)
}
