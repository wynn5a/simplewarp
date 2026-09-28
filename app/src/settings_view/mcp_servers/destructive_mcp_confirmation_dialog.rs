use warpui::elements::{ChildView, Container, Dismiss, Empty};
use warpui::ui_components::components::UiComponent;
use warpui::{
    AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use crate::appearance::Appearance;
use crate::ui_components::dialog::{Dialog, dialog_styles};
use crate::view_components::action_button::{ActionButton, DangerPrimaryTheme, NakedTheme};

const DIALOG_WIDTH: f32 = 450.;
const TITLE_TEXT: &str = "Delete MCP server?";
const DESCRIPTION_TEXT: &str =
    "This will uninstall and remove this MCP server from all your devices.";

pub enum DestructiveMCPConfirmationDialogEvent {
    Cancel,
    Confirm,
}

#[derive(Debug)]
pub enum DestructiveMCPConfirmationDialogAction {
    Cancel,
    Confirm,
}

pub struct DestructiveMCPConfirmationDialog {
    visible: bool,
    cancel_button: ViewHandle<ActionButton>,
    confirm_button: ViewHandle<ActionButton>,
}

impl DestructiveMCPConfirmationDialog {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let cancel_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Cancel", NakedTheme).on_click(|ctx| {
                ctx.dispatch_typed_action(DestructiveMCPConfirmationDialogAction::Cancel);
            })
        });

        let confirm_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Delete MCP", DangerPrimaryTheme).on_click(|ctx| {
                ctx.dispatch_typed_action(DestructiveMCPConfirmationDialogAction::Confirm);
            })
        });

        Self {
            visible: false,
            cancel_button,
            confirm_button,
        }
    }

    pub fn show(&mut self, ctx: &mut ViewContext<Self>) {
        self.visible = true;
        ctx.notify();
    }

    pub fn hide(&mut self, ctx: &mut ViewContext<Self>) {
        self.visible = false;
        ctx.notify();
    }
}

impl Entity for DestructiveMCPConfirmationDialog {
    type Event = DestructiveMCPConfirmationDialogEvent;
}

impl View for DestructiveMCPConfirmationDialog {
    fn ui_name() -> &'static str {
        "DestructiveMCPConfirmationDialog"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        if !self.visible {
            return Empty::new().finish();
        }

        let appearance = Appearance::as_ref(app);
        let dialog = Dialog::new(
            TITLE_TEXT.to_string(),
            Some(DESCRIPTION_TEXT.to_string()),
            dialog_styles(appearance),
        )
        .with_bottom_row_child(ChildView::new(&self.cancel_button).finish())
        .with_bottom_row_child(
            Container::new(ChildView::new(&self.confirm_button).finish())
                .with_margin_left(12.)
                .finish(),
        )
        .with_width(DIALOG_WIDTH)
        .build()
        .finish();

        Dismiss::new(dialog)
            .prevent_interaction_with_other_elements()
            .on_dismiss(|ctx, _app| {
                ctx.dispatch_typed_action(DestructiveMCPConfirmationDialogAction::Cancel)
            })
            .finish()
    }
}

impl TypedActionView for DestructiveMCPConfirmationDialog {
    type Action = DestructiveMCPConfirmationDialogAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            DestructiveMCPConfirmationDialogAction::Cancel => {
                ctx.emit(DestructiveMCPConfirmationDialogEvent::Cancel)
            }
            DestructiveMCPConfirmationDialogAction::Confirm => {
                ctx.emit(DestructiveMCPConfirmationDialogEvent::Confirm)
            }
        }
    }
}
