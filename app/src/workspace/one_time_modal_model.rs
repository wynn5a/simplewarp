use warpui::{Entity, ModelContext, SingletonEntity, WindowId};

use super::view::feature_intro_modal::FeatureIntroId;

/// A generic model for managing one-time modals that should be shown to users only once.
///
/// Initially implemented for the ADE launch modal, but designed to be extensible to support
/// other types of one-time modals in the future. The model holds the canonical state of whether
/// a modal is currently being shown.
pub struct OneTimeModalModel {
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

    pub fn update_target_window_id(&mut self, window_id: WindowId, ctx: &mut ModelContext<Self>) {
        // Feature intros are non-blocking popovers tracked separately from
        // blocking modals. This re-emits when activation binds a selected-but-
        // not-yet-bound intro to a window, or retargets a visible one, so the
        // workspace calls `show_feature_intro_modal` on the right window.
        let was_feature_intro_visible = self.active_feature_intro().is_some();
        let previous_target = self.target_window_id;
        self.target_window_id = Some(window_id);
        let is_feature_intro_visible = self.active_feature_intro().is_some();
        if was_feature_intro_visible != is_feature_intro_visible
            || (is_feature_intro_visible && previous_target != Some(window_id))
        {
            ctx.emit(OneTimeModalEvent::VisibilityChanged {
                is_open: is_feature_intro_visible,
            });
        }
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
