use warpui::ViewContext;

use super::TerminalView;
use crate::terminal::view::CodeDiffAction;

#[derive(Copy, Clone, Debug)]
pub enum PromptSuggestionResolution {
    Accept,
    Reject { ctrl_c: bool },
}

impl From<PromptSuggestionResolution> for CodeDiffAction {
    fn from(value: PromptSuggestionResolution) -> Self {
        match value {
            PromptSuggestionResolution::Accept => CodeDiffAction::Accept,
            PromptSuggestionResolution::Reject { .. } => CodeDiffAction::Reject,
        }
    }
}

impl TerminalView {
    pub(super) fn resolve_passive_suggestion(
        &mut self,
        resolution: PromptSuggestionResolution,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        if self.resolve_prompt_suggestion_diff(resolution, ctx) {
            return true;
        }
        if self.resolve_prompt_suggestion(resolution, ctx) {
            return true;
        }

        false
    }

    pub(super) fn resolve_prompt_suggestion_diff(
        &mut self,
        action: impl Into<CodeDiffAction>,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let Some(ai_block) = self.last_ai_block() else {
            return false;
        };
        let action = action.into();
        ai_block.update(ctx, |ai_block, ctx| {
            ai_block.handle_passive_code_diff_action(action, ctx)
        })
    }
}
