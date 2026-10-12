use std::sync::Arc;

use parking_lot::FairMutex;
use warpui::{Entity, ModelContext, ModelHandle, SingletonEntity};

use super::static_prompt_suggestions::static_suggested_query;
use crate::network::NetworkStatus;
use crate::settings::AISettings;
use crate::terminal::event::{BlockType, UserBlockCompleted};
use crate::terminal::model::block::BlockId;
use crate::terminal::model::terminal_model::TerminalModel;
use crate::terminal::model_events::{ModelEvent, ModelEventDispatcher};
use crate::terminal::view::PromptSuggestion;

#[derive(Clone, Debug)]
pub enum PassiveSuggestionsEvent {
    PromptSuggestionsGenerated {
        prompt_suggestion: PromptSuggestion,
        block_id: BlockId,
    },
}

pub struct PassiveSuggestionsModel {
    terminal_model: Arc<FairMutex<TerminalModel>>,
}

impl PassiveSuggestionsModel {
    pub fn new(
        terminal_model: Arc<FairMutex<TerminalModel>>,
        model_event_dispatcher: &ModelHandle<ModelEventDispatcher>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        ctx.subscribe_to_model(model_event_dispatcher, |me, _, event, ctx| {
            me.handle_model_event(event, ctx);
        });

        Self { terminal_model }
    }

    fn handle_model_event(&mut self, event: &ModelEvent, ctx: &mut ModelContext<Self>) {
        let ModelEvent::AfterBlockCompleted(after_block_completed_event) = event else {
            return;
        };
        let BlockType::User(block_completed) = &after_block_completed_event.block_type else {
            return;
        };
        self.handle_user_block_completed(block_completed, ctx);
    }

    fn handle_user_block_completed(
        &mut self,
        block_completed: &UserBlockCompleted,
        ctx: &mut ModelContext<Self>,
    ) {
        if block_completed.was_part_of_agent_interaction {
            return;
        }

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

        if should_generate_prompt_suggestions(block_completed, ctx) {
            self.generate_prompt_suggestions(block_completed, ctx);
        }
    }

    fn generate_prompt_suggestions(
        &mut self,
        block_completed: &UserBlockCompleted,
        ctx: &mut ModelContext<Self>,
    ) {
        // The only prompt suggestions left come from the static local table, which needs
        // nothing but the block that just finished. Warp's server used to serve richer
        // suggestions here; with it gone there is no other source, so without a static
        // suggestion there is nothing to emit.
        if let Some(suggestion) = fetch_static_prompt_suggestion(block_completed) {
            ctx.emit(PassiveSuggestionsEvent::PromptSuggestionsGenerated {
                prompt_suggestion: suggestion,
                block_id: block_completed.serialized_block.id.clone(),
            });
        }
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
}

fn fetch_static_prompt_suggestion(block: &UserBlockCompleted) -> Option<PromptSuggestion> {
    if !block.serialized_block.exit_code.was_successful() {
        return None;
    }
    static_suggested_query(&block.command)
}
