use std::any::Any;
use std::sync::Arc;

use parking_lot::FairMutex;
use warpui::{AppContext, ViewHandle, WindowId};

use super::terminal_manager::{TerminalManager, TerminalViewInit};
use crate::ai::blocklist::{InputConfig, SerializedBlockListItem};
use crate::context_chips::current_prompt::CurrentPrompt;
use crate::context_chips::prompt_type::PromptType;
use crate::terminal::view::ConversationRestorationInNewPaneType;
use crate::terminal::{TerminalManager as TerminalManagerTrait, TerminalModel, TerminalView};

/// Configuration for the `TerminalView` a local terminal manager creates.
pub(crate) struct TerminalViewConfig {
    pub(crate) window_id: WindowId,
    pub(crate) initial_input_config: Option<InputConfig>,
    pub(crate) conversation_restoration: Option<ConversationRestorationInNewPaneType>,
    pub(crate) has_conversation_restoration: bool,
    pub(crate) is_historical: bool,
    pub(crate) should_use_live_appearance: bool,
    pub(crate) has_restored_command_blocks: bool,
}

impl TerminalViewConfig {
    /// Whether the block list should end with a session restoration separator, and if so whether
    /// it marks a historical conversation.
    pub(super) fn restoration_separator(&self) -> Option<bool> {
        ((self.has_conversation_restoration || self.has_restored_command_blocks)
            && !self.should_use_live_appearance)
            .then_some(self.is_historical)
    }
}

/// Resolves the block list a restored `TerminalView` starts with.
pub(crate) fn terminal_view_restored_blocks(
    restored_blocks: Option<&Vec<SerializedBlockListItem>>,
    conversation_restoration: &Option<ConversationRestorationInNewPaneType>,
) -> Option<Vec<SerializedBlockListItem>> {
    restored_blocks
        .filter(|blocks| !blocks.is_empty())
        .cloned()
        .or_else(|| match conversation_restoration {
            Some(ConversationRestorationInNewPaneType::Historical { conversation, .. })
            | Some(ConversationRestorationInNewPaneType::Forked { conversation, .. }) => {
                Some(conversation.to_serialized_blocklist_items())
            }
            Some(ConversationRestorationInNewPaneType::Startup { conversations, .. }) => {
                let mut items: Vec<_> = conversations
                    .iter()
                    .flat_map(|c| c.to_serialized_blocklist_items())
                    .collect();
                // Because there are multiple conversations that may have interleaved timestamps, we need to sort by start_ts
                items.sort_by_key(|item| item.start_ts());
                if items.is_empty() { None } else { Some(items) }
            }
            _ => None,
        })
}

/// Creates the `TerminalView` for a local terminal manager.
pub(super) fn create_terminal_view(
    config: TerminalViewConfig,
    init: TerminalViewInit,
    ctx: &mut AppContext,
) -> ViewHandle<TerminalView> {
    let TerminalViewInit {
        wakeups_rx,
        model_events,
        model,
        sessions,
        size_info,
        colors,
        inactive_pty_reads_rx,
        model_event_sender,
    } = init;
    let TerminalViewConfig {
        window_id,
        initial_input_config,
        conversation_restoration,
        ..
    } = config;
    let current_prompt = ctx.add_model(|ctx| {
        CurrentPrompt::new_with_model_events(sessions.clone(), Some(&model_events), ctx)
    });
    let prompt_type = ctx.add_model(|ctx| PromptType::new_dynamic(current_prompt.clone(), ctx));
    ctx.add_typed_action_view(window_id, |ctx| {
        TerminalView::new(
            wakeups_rx,
            model_events,
            model,
            sessions,
            size_info,
            colors,
            model_event_sender,
            prompt_type.clone(),
            initial_input_config,
            conversation_restoration,
            Some(inactive_pty_reads_rx),
            ctx,
        )
    })
}

impl TerminalManager {
    /// Returns the PTY process id, for integration tests.
    #[cfg(feature = "integration_tests")]
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }
}

impl TerminalManagerTrait for TerminalManager {
    fn model(&self) -> Arc<FairMutex<TerminalModel>> {
        self.model.clone()
    }

    fn on_view_detached(
        &self,
        _detach_type: crate::pane_group::pane::DetachType,
        _app: &mut AppContext,
    ) {
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Send a Shutdown event to each PTY's event loop and waits for the
/// event loop to terminate.
/// This is needed on Windows to ensure all OpenConsole processes are
/// cleaned up before the main thread exits.
#[cfg(windows)]
pub fn shutdown_all_pty_event_loops(ctx: &mut AppContext) {
    let terminal_managers: Vec<warpui::ModelHandle<Box<dyn TerminalManagerTrait>>> =
        ctx.models_of_type();
    terminal_managers.into_iter().for_each(|terminal_manager| {
        terminal_manager.update(ctx, |terminal_manager, _ctx| {
            if let Some(manager) = terminal_manager
                .as_any_mut()
                .downcast_mut::<TerminalManager>()
            {
                manager.shutdown_event_loop();
            }
        })
    })
}
