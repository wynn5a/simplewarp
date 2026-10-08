use std::path::PathBuf;

use warp_core::ui::appearance::Appearance;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::elements::{
    Align, ChildView, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container, Flex,
    MainAxisSize, ParentElement, ScrollbarWidth,
};
use warpui::{
    AppContext, Element, Entity, FocusContext, ModelHandle, SingletonEntity, TypedActionView, View,
    ViewContext, ViewHandle,
};

use super::{AIFact, CloudAIFact, CloudAIFactModel};
use crate::pane_group::focus_state::PaneFocusHandle;
use crate::pane_group::pane::view;
use crate::pane_group::{BackingView, PaneConfiguration, PaneEvent};
use crate::server::ids::SyncId;

pub mod rule;
pub mod rule_editor;
mod style;
use rule::*;
use rule_editor::*;

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum AIFactPage {
    #[default]
    Rules,
    RuleEditor {
        sync_id: Option<SyncId>,
    },
}

impl std::fmt::Display for AIFactPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AIFactPage::Rules => write!(f, "Rules"),
            AIFactPage::RuleEditor { .. } => write!(f, "Rule Editor"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum AIFactViewEvent {
    Pane(PaneEvent),
    OpenSettings,
    OpenFile(LocalOrRemotePath),
    InitializeProject(PathBuf),
}

#[derive(Debug, Clone)]
pub enum AIFactViewAction {
    AddRule,
    UpdatePage(AIFactPage),
}

pub struct AIFactView {
    pane_configuration: ModelHandle<PaneConfiguration>,
    focus_handle: Option<PaneFocusHandle>,
    current_page: AIFactPage,
    rule_view: ViewHandle<RuleView>,
    rule_editor_view: ViewHandle<RuleEditorView>,
    clipped_scroll_state: ClippedScrollStateHandle,
}

impl AIFactView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let pane_configuration = ctx.add_model(|_ctx| PaneConfiguration::new(HEADER_TEXT));

        let rule_view = ctx.add_typed_action_view(RuleView::new);
        ctx.subscribe_to_view(&rule_view, |me, _, event, ctx| {
            me.handle_rule_view_event(event, ctx);
        });

        let rule_editor_view = ctx.add_typed_action_view(RuleEditorView::new);
        ctx.subscribe_to_view(&rule_editor_view, |me, _, event, ctx| {
            me.handle_rule_editor_view_event(event, ctx);
        });

        Self {
            pane_configuration,
            focus_handle: None,
            rule_editor_view,
            rule_view,
            current_page: AIFactPage::default(),
            clipped_scroll_state: Default::default(),
        }
    }

    pub fn pane_configuration(&self) -> ModelHandle<PaneConfiguration> {
        self.pane_configuration.clone()
    }

    pub fn current_page(&self) -> AIFactPage {
        self.current_page
    }

    pub fn focus(&mut self, ctx: &mut ViewContext<Self>) {
        match self.current_page {
            AIFactPage::Rules => ctx.focus(&self.rule_view),
            AIFactPage::RuleEditor { .. } => ctx.focus(&self.rule_editor_view),
        }
    }

    fn handle_rule_view_event(&mut self, event: &RuleViewEvent, ctx: &mut ViewContext<Self>) {
        match event {
            RuleViewEvent::AddRule => {
                self.update_page(AIFactPage::RuleEditor { sync_id: None }, ctx);
            }
            RuleViewEvent::Edit(sync_id) => {
                self.update_page(
                    AIFactPage::RuleEditor {
                        sync_id: Some(*sync_id),
                    },
                    ctx,
                );
            }
            RuleViewEvent::OpenSettings => {
                ctx.emit(AIFactViewEvent::OpenSettings);
            }
            RuleViewEvent::OpenFile(path) => {
                ctx.emit(AIFactViewEvent::OpenFile(path.clone()));
            }
            RuleViewEvent::InitializeProject(path) => {
                ctx.emit(AIFactViewEvent::InitializeProject(path.clone()));
            }
        }
    }

    fn handle_rule_editor_view_event(
        &mut self,
        event: &RuleEditorViewEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        self.update_page(AIFactPage::Rules, ctx);
        match event {
            RuleEditorViewEvent::Add { name, content } => {
                self.rule_view.update(ctx, |rule_view, ctx| {
                    rule_view.add_ai_rule(name.clone(), content.clone(), ctx);
                });
            }
            RuleEditorViewEvent::Edit {
                name,
                content,
                sync_id,
            } => {
                self.rule_view.update(ctx, |rule_view, ctx| {
                    rule_view.edit_ai_rule(name.clone(), content.clone(), *sync_id, ctx);
                });
            }
            RuleEditorViewEvent::Delete { sync_id } => {
                self.rule_view.update(ctx, |rule_view, ctx| {
                    rule_view.delete_ai_rule(*sync_id, ctx);
                });
            }
            _ => {}
        }
    }

    pub fn update_page(&mut self, page: AIFactPage, ctx: &mut ViewContext<Self>) {
        self.current_page = page;
        if let AIFactPage::RuleEditor { sync_id } = page {
            self.rule_editor_view.update(ctx, |rule_editor_view, ctx| {
                rule_editor_view.set_ai_rule(sync_id, ctx);
            });
        }
        self.focus(ctx);
        ctx.notify();
    }
}

impl Entity for AIFactView {
    type Event = AIFactViewEvent;
}

impl View for AIFactView {
    fn ui_name() -> &'static str {
        "AIFactView"
    }

    fn on_focus(&mut self, focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {
        if focus_ctx.is_self_focused() {
            match self.current_page {
                AIFactPage::Rules => ctx.focus(&self.rule_view),
                AIFactPage::RuleEditor { .. } => ctx.focus(&self.rule_editor_view),
            }
        }
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let mut col = Flex::column().with_main_axis_size(MainAxisSize::Min);
        match self.current_page {
            AIFactPage::Rules => col.add_child(ChildView::new(&self.rule_view).finish()),
            AIFactPage::RuleEditor { .. } => {
                col.add_child(ChildView::new(&self.rule_editor_view).finish())
            }
        }

        ClippedScrollable::vertical(
            self.clipped_scroll_state.clone(),
            Align::new(
                Container::new(
                    ConstrainedBox::new(col.finish())
                        .with_max_width(style::PANE_WIDTH)
                        .finish(),
                )
                .with_uniform_padding(style::PANE_PADDING)
                .finish(),
            )
            .top_center()
            .finish(),
            ScrollbarWidth::Auto,
            appearance.theme().nonactive_ui_detail().into(),
            appearance.theme().active_ui_detail().into(),
            warpui::elements::Fill::None,
        )
        .finish()
    }
}

impl TypedActionView for AIFactView {
    type Action = AIFactViewAction;

    fn handle_action(&mut self, action: &AIFactViewAction, ctx: &mut ViewContext<Self>) {
        match action {
            AIFactViewAction::AddRule => {
                self.rule_editor_view.update(ctx, |rule_editor_view, ctx| {
                    rule_editor_view.set_ai_rule(None, ctx);
                });
                self.update_page(AIFactPage::RuleEditor { sync_id: None }, ctx);
            }
            AIFactViewAction::UpdatePage(page) => self.update_page(*page, ctx),
        }
    }
}

impl BackingView for AIFactView {
    type PaneHeaderOverflowMenuAction = AIFactViewAction;
    type CustomAction = ();
    type AssociatedData = ();

    fn handle_pane_header_overflow_menu_action(
        &mut self,
        _action: &Self::PaneHeaderOverflowMenuAction,
        _ctx: &mut warpui::ViewContext<Self>,
    ) {
        self.handle_action(_action, _ctx)
    }

    fn close(&mut self, ctx: &mut warpui::ViewContext<Self>) {
        ctx.emit(AIFactViewEvent::Pane(PaneEvent::Close));
    }

    fn focus_contents(&mut self, ctx: &mut warpui::ViewContext<Self>) {
        self.focus(ctx);
    }

    fn render_header_content(
        &self,
        _ctx: &view::HeaderRenderContext,
        _app: &AppContext,
    ) -> view::HeaderContent {
        view::HeaderContent::simple(HEADER_TEXT)
    }

    fn set_focus_handle(&mut self, focus_handle: PaneFocusHandle, _ctx: &mut ViewContext<Self>) {
        self.focus_handle = Some(focus_handle);
    }
}
