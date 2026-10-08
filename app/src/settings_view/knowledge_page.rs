//! The "Knowledge" settings page, shown under the Agents umbrella.

use markdown_parser::{FormattedText, FormattedTextFragment, FormattedTextLine};
use warp_core::features::FeatureFlag;
use warp_core::settings::ToggleableSetting as _;
use warpui::elements::{
    Container, Element, Flex, FormattedTextElement, HighlightedHyperlink, MouseStateHandle,
    ParentElement,
};
use warpui::keymap::ContextPredicate;
use warpui::ui_components::switch::SwitchStateHandle;
use warpui::{
    Action, AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle, id,
};

use super::ai_shared::{render_ai_setting_toggle, styles};
use super::settings_page::{
    CONTENT_FONT_SIZE, MatchData, PageType, SettingsPageMeta, SettingsPageViewHandle,
    SettingsWidget, render_full_pane_width_ai_button,
};
use super::{SettingsAction, SettingsSection, ToggleSettingActionPair, flags};
use crate::appearance::Appearance;
use crate::settings::AISettings;
use crate::util::bindings;

const PAGE_TITLE: &str = "Knowledge";

pub struct KnowledgePageView {
    page: PageType<Self>,
}

impl KnowledgePageView {
    pub fn new(_ctx: &mut ViewContext<Self>) -> Self {
        Self {
            page: Self::build_page(),
        }
    }

    fn build_page() -> PageType<Self> {
        let mut widgets: Vec<Box<dyn SettingsWidget<View = Self>>> = Vec::new();
        if FeatureFlag::AIRules.is_enabled() {
            widgets.push(Box::new(RulesWidget::default()));
            widgets.push(Box::new(ManageRulesWidget::default()));
        }
        PageType::new_uncategorized(widgets, Some(PAGE_TITLE))
    }
}

impl Entity for KnowledgePageView {
    type Event = KnowledgePageEvent;
}

impl View for KnowledgePageView {
    fn ui_name() -> &'static str {
        "KnowledgePage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

pub enum KnowledgePageEvent {
    OpenAIFactCollection,
}

#[derive(Debug, Clone)]
pub enum KnowledgePageAction {
    ToggleRules,
    OpenAIFactCollection,
}

impl TypedActionView for KnowledgePageView {
    type Action = KnowledgePageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            KnowledgePageAction::ToggleRules => {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    let _ = settings.memory_enabled.toggle_and_save_value(ctx);
                });
                ctx.notify();
            }
            KnowledgePageAction::OpenAIFactCollection => {
                ctx.emit(KnowledgePageEvent::OpenAIFactCollection)
            }
        }
    }
}

impl SettingsPageMeta for KnowledgePageView {
    fn section() -> SettingsSection {
        SettingsSection::Knowledge
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        FeatureFlag::AgentMode.is_enabled()
    }

    fn update_filter(&mut self, query: &str, ctx: &mut ViewContext<Self>) -> MatchData {
        self.page.update_filter(query, ctx)
    }

    fn scroll_to_widget(&mut self, widget_id: &'static str) {
        self.page.scroll_to_widget(widget_id)
    }

    fn clear_highlighted_widget(&mut self) {
        self.page.clear_highlighted_widget();
    }
}

impl From<ViewHandle<KnowledgePageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<KnowledgePageView>) -> Self {
        SettingsPageViewHandle::Knowledge(view_handle)
    }
}

pub fn init_actions_from_parent_view<T: Action + Clone>(
    app: &mut AppContext,
    context: &ContextPredicate,
    builder: fn(SettingsAction) -> T,
) {
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![
            ToggleSettingActionPair::new(
                "Rules",
                builder(SettingsAction::Knowledge(KnowledgePageAction::ToggleRules)),
                &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
                flags::AI_RULES_FLAG,
            )
            .with_group(bindings::BindingGroup::WarpAi)
            .with_enabled(|| FeatureFlag::AIRules.is_enabled()),
        ],
        app,
    );
}

#[derive(Default)]
struct RulesWidget {
    rules_toggle: SwitchStateHandle,
}

impl SettingsWidget for RulesWidget {
    type View = KnowledgePageView;

    fn search_terms(&self) -> &str {
        "fact memory memories rules conventions"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);
        let toggle = render_ai_setting_toggle(
            "Rules",
            KnowledgePageAction::ToggleRules,
            *ai_settings.memory_enabled,
            ai_settings.is_any_ai_enabled(),
            self.rules_toggle.clone(),
            app,
        );

        let rules_description = vec![FormattedTextFragment::plain_text(
            "Rules help the Warp Agent follow your conventions, whether for codebases or specific workflows.",
        )];
        let description = Container::new(
            FormattedTextElement::new(
                FormattedText::new([FormattedTextLine::Line(rules_description)]),
                CONTENT_FONT_SIZE,
                appearance.ui_font_family(),
                appearance.ui_font_family(),
                styles::description_font_color(ai_settings.is_any_ai_enabled(), app).into(),
                HighlightedHyperlink::default(),
            )
            .finish(),
        )
        .with_margin_top(styles::DESCRIPTION_NEGATIVE_MARGIN_OFFSET)
        .with_margin_bottom(styles::DESCRIPTION_MARGIN_BOTTOM)
        .with_margin_right(styles::TOGGLE_WIDTH_MARGIN)
        .finish();

        Flex::column()
            .with_child(toggle)
            .with_child(description)
            .finish()
    }
}

#[derive(Default)]
struct ManageRulesWidget {
    manage_rules_button: MouseStateHandle,
}

impl SettingsWidget for ManageRulesWidget {
    type View = KnowledgePageView;

    fn search_terms(&self) -> &str {
        "manage rules rule collection"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        render_full_pane_width_ai_button(
            "Manage rules",
            AISettings::as_ref(app).is_any_ai_enabled(),
            self.manage_rules_button.clone(),
            KnowledgePageAction::OpenAIFactCollection,
            appearance,
        )
    }
}
