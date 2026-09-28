use warp_errors::report_error;
use warpui::{AppContext, Entity, ModelContext, ModelHandle, SingletonEntity};

use super::current_prompt::CurrentPrompt;
use super::prompt_snapshot::PromptSnapshot;
use super::{ChipResult, ChipValue, ContextChipKind};
use crate::menu::{MenuItem, MenuItemFields};
use crate::settings::WarpPromptSeparator;
use crate::terminal::model::session::Sessions;
use crate::terminal::session_settings::{SessionSettings, ToolbarChipSelection};
use crate::terminal::view::{ContextMenuAction, PromptPart, PromptPosition, TerminalAction};

/// A warp prompt that refreshes chip values on its own.
#[derive(Clone)]
pub struct PromptType {
    prompt: ModelHandle<CurrentPrompt>,
}

impl PromptType {
    pub fn new_dynamic_from_sessions(
        sessions: ModelHandle<Sessions>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        let current_prompt = ctx.add_model(|ctx| CurrentPrompt::new(sessions, ctx));
        Self::new_dynamic(current_prompt, ctx)
    }

    pub fn new_dynamic(
        current_prompt: ModelHandle<CurrentPrompt>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        ctx.observe(&current_prompt, |_, _, ctx| ctx.notify());
        Self {
            prompt: current_prompt,
        }
    }

    pub fn current_prompt(&self) -> &ModelHandle<CurrentPrompt> {
        &self.prompt
    }

    /// Returns menu items for copying parts of the prompt given a prompt snapshot.
    pub fn copy_menu_items(
        &self,
        position: PromptPosition,
        ctx: &AppContext,
    ) -> Vec<MenuItem<TerminalAction>> {
        self.chips(ctx)
            .into_iter()
            .filter_map(|chip_result| {
                if chip_result.value.is_some() && chip_result.kind.is_copyable() {
                    if let Some(chip) = chip_result.kind.to_chip() {
                        Some(
                            MenuItemFields::new(format!("Copy {}", chip.title()))
                                .with_on_select_action(TerminalAction::ContextMenu(
                                    ContextMenuAction::CopyPrompt {
                                        position,
                                        part: PromptPart::ContextChip(chip_result.kind),
                                    },
                                ))
                                .into_item(),
                        )
                    } else {
                        report_error!(
                            "Missing definition for chip",
                            extra: { "chip_kind" => ?chip_result.kind }
                        );
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn latest_chip_value(
        &self,
        chip_kind: &ContextChipKind,
        ctx: &AppContext,
    ) -> Option<ChipValue> {
        self.prompt
            .as_ref(ctx)
            .latest_chip_value(chip_kind)
            .cloned()
    }

    pub fn prompt_as_string(&self, ctx: &AppContext) -> String {
        self.prompt.as_ref(ctx).prompt_as_string(ctx)
    }

    pub fn snapshot(&self, ctx: &AppContext) -> PromptSnapshot {
        PromptSnapshot::from_current_prompt(self.prompt.as_ref(ctx), ctx)
    }

    pub fn chips(&self, ctx: &AppContext) -> Vec<ChipResult> {
        self.snapshot(ctx).chips().clone()
    }

    pub fn agent_view_chips(&self, ctx: &AppContext) -> Vec<ChipResult> {
        let chip_kinds = SessionSettings::as_ref(ctx)
            .agent_footer_chip_selection
            .all_chips();
        self.resolve_chip_kinds(chip_kinds, ctx)
    }

    pub fn agent_view_left_chips(&self, ctx: &AppContext) -> Vec<ChipResult> {
        let chip_kinds = SessionSettings::as_ref(ctx)
            .agent_footer_chip_selection
            .left_chips();
        self.resolve_chip_kinds(chip_kinds, ctx)
    }

    pub fn agent_view_right_chips(&self, ctx: &AppContext) -> Vec<ChipResult> {
        let chip_kinds = SessionSettings::as_ref(ctx)
            .agent_footer_chip_selection
            .right_chips();
        self.resolve_chip_kinds(chip_kinds, ctx)
    }

    pub fn cli_agent_chips(&self, ctx: &AppContext) -> Vec<ChipResult> {
        let chip_kinds = SessionSettings::as_ref(ctx)
            .cli_agent_footer_chip_selection
            .all_chips();
        self.resolve_chip_kinds(chip_kinds, ctx)
    }

    fn resolve_chip_kinds(
        &self,
        chip_kinds: Vec<ContextChipKind>,
        ctx: &AppContext,
    ) -> Vec<ChipResult> {
        chip_kinds
            .into_iter()
            .filter_map(|chip_kind| self.prompt.as_ref(ctx).latest_chip_result(&chip_kind))
            .collect()
    }

    /// Whether same line prompt is enabled for the Warp Prompt.
    pub fn same_line_prompt_enabled(&self, ctx: &AppContext) -> bool {
        self.prompt.as_ref(ctx).same_line_prompt_enabled()
    }

    /// The separator for the Warp prompt.
    pub fn separator(&self, ctx: &AppContext) -> WarpPromptSeparator {
        self.prompt.as_ref(ctx).separator()
    }
}

impl Entity for PromptType {
    type Event = ();
}
