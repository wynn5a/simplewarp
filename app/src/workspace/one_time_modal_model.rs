use warpui::{Entity, ModelContext, SingletonEntity, WindowId};

use super::view::feature_intro_modal::FeatureIntroId;

/// A generic model for managing one-time modals that should be shown to users only once.
///
/// Initially implemented for the ADE launch modal, but designed to be extensible to support
/// other types of one-time modals in the future. The model holds the canonical state of whether
/// a modal is currently being shown.
pub struct OneTimeModalModel {
    /// Whether the free-AI-removal notice modal is currently being shown.
    is_free_ai_removal_modal_open: bool,
    /// The feature-intro popover currently being shown, if any. Unlike the other
    /// one-time modals this is a non-blocking bottom-right popover, so it is
    /// intentionally excluded from `is_any_modal_open` (which suppresses terminal
    /// focus stealing) to keep the terminal usable while it is visible.
    active_feature_intro: Option<FeatureIntroId>,
    /// The window ID where the currently open one-time modal should be displayed.
    /// This is captured when a modal is first opened and ensures the modal stays on that window.
    target_window_id: Option<WindowId>,
}

impl OneTimeModalModel {
    pub fn new(_ctx: &mut ModelContext<Self>) -> Self {
        Self {
            is_free_ai_removal_modal_open: false,
            active_feature_intro: None,
            target_window_id: None,
        }
    }

    /// Returns the window ID where the currently open one-time modal should be displayed.
    pub fn target_window_id(&self) -> Option<WindowId> {
        self.target_window_id
    }

    /// Returns the feature-intro popover currently being shown, if any.
    pub fn active_feature_intro(&self) -> Option<FeatureIntroId> {
        if self.target_window_id.is_some() {
            self.active_feature_intro
        } else {
            None
        }
    }

    pub fn mark_feature_intro_dismissed(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_active_feature_intro(None, ctx);
    }

    #[cfg(debug_assertions)]
    pub fn force_open_feature_intro(&mut self, id: FeatureIntroId, ctx: &mut ModelContext<Self>) {
        self.set_active_feature_intro(Some(id), ctx);
    }

    fn set_active_feature_intro(
        &mut self,
        intro: Option<FeatureIntroId>,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        if self.active_feature_intro != intro {
            self.active_feature_intro = intro;
            // Bind the popover to the focused window as soon as it opens, since
            // the workspace only renders / populates the view when
            // `target_window_id` matches and `on_active_window_changed` may not
            // have run yet.
            if intro.is_some()
                && self.target_window_id.is_none()
                && let Some(window_id) = ctx.windows().active_window()
            {
                self.target_window_id = Some(window_id);
            }
            ctx.emit(OneTimeModalEvent::VisibilityChanged {
                is_open: intro.is_some(),
            });
            return true;
        }
        false
    }

    /// Returns true if any one-time modal is currently open.
    pub fn is_any_modal_open(&self) -> bool {
        self.is_free_ai_removal_modal_open && self.target_window_id.is_some()
    }

    pub fn update_target_window_id(&mut self, window_id: WindowId, ctx: &mut ModelContext<Self>) {
        let was_any_modal_visible = self.is_any_modal_open();
        // Feature intro is intentionally excluded from `is_any_modal_open`, so
        // track it separately. Without this, activating a window after an intro
        // was selected but before it was bound to one never re-emits, and the
        // workspace never calls `show_feature_intro_modal`.
        let was_feature_intro_visible = self.active_feature_intro().is_some();
        let previous_target = self.target_window_id;
        self.target_window_id = Some(window_id);
        let is_any_modal_visible = self.is_any_modal_open();
        let is_feature_intro_visible = self.active_feature_intro().is_some();
        if was_any_modal_visible != is_any_modal_visible
            || was_feature_intro_visible != is_feature_intro_visible
            || (is_feature_intro_visible && previous_target != Some(window_id))
        {
            ctx.emit(OneTimeModalEvent::VisibilityChanged {
                is_open: is_any_modal_visible || is_feature_intro_visible,
            });
        }
    }

    /// Returns whether the free-AI-removal notice modal is currently open.
    pub fn is_free_ai_removal_modal_open(&self) -> bool {
        self.is_free_ai_removal_modal_open && self.target_window_id.is_some()
    }

    pub fn mark_free_ai_removal_modal_dismissed(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_free_ai_removal_modal_open(false, ctx);
    }

    #[cfg(debug_assertions)]
    pub fn force_open_free_ai_removal_modal(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_free_ai_removal_modal_open(true, ctx);
    }

    fn set_free_ai_removal_modal_open(
        &mut self,
        is_open: bool,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        if self.is_free_ai_removal_modal_open != is_open {
            self.is_free_ai_removal_modal_open = is_open;
            ctx.emit(OneTimeModalEvent::VisibilityChanged { is_open });
            return true;
        }
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OneTimeModalEvent {
    VisibilityChanged { is_open: bool },
}

impl Entity for OneTimeModalModel {
    type Event = OneTimeModalEvent;
}

impl SingletonEntity for OneTimeModalModel {}

#[cfg(test)]
#[path = "one_time_modal_model_tests.rs"]
mod tests;
