//! The "Profiles" settings page, shown under the Agents umbrella: the list of agent execution
//! profiles, each opening its own editor.

use settings::ToggleableSetting;
use warpui::elements::{
    ChildView, Container, CrossAxisAlignment, Element, Flex, MainAxisSize, ParentElement,
    Shrinkable,
};
use warpui::keymap::ContextPredicate;
use warpui::{
    Action, AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle, id,
};

use super::ai_shared::{render_ai_setting_description, styles};
use super::execution_profile_view::{ExecutionProfileView, ExecutionProfileViewEvent};
use super::settings_page::{
    MatchData, PageType, SettingsPageMeta, SettingsPageViewHandle, SettingsWidget, build_sub_header,
};
use super::{SettingsAction, SettingsSection, ToggleSettingActionPair, flags};
use crate::ai::execution_profiles::ExecutionProfileId;
use crate::ai::execution_profiles::profiles::{
    AIExecutionProfilesModel, AIExecutionProfilesModelEvent,
};
use crate::appearance::Appearance;
use crate::settings::{AISettings, AISettingsChangedEvent, CodeSettings};
use crate::ui_components::icons::Icon;
use crate::view_components::action_button::{ActionButton, ButtonSize, SecondaryTheme};

pub struct AgentProfilesPageView {
    page: PageType<Self>,
    profile_views: Vec<ViewHandle<ExecutionProfileView>>,
    add_profile_button: ViewHandle<ActionButton>,
}

impl AgentProfilesPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        ctx.subscribe_to_model(
            &AIExecutionProfilesModel::handle(ctx),
            |me, _, event, ctx| {
                match event {
                    AIExecutionProfilesModelEvent::ProfileCreated
                    | AIExecutionProfilesModelEvent::ProfileDeleted => {
                        me.refresh_profile_views(ctx);
                    }
                    AIExecutionProfilesModelEvent::ProfileUpdated(_)
                    | AIExecutionProfilesModelEvent::UpdatedActiveProfile { .. } => (),
                }
                ctx.notify();
            },
        );

        ctx.subscribe_to_model(&AISettings::handle(ctx), |me, _, event, ctx| {
            if let AISettingsChangedEvent::IsAnyAIEnabled = event {
                me.refresh_add_profile_button(ctx);
            }
            ctx.notify();
        });

        let profile_views = Self::create_profile_views(ctx);

        let add_profile_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Add Profile", SecondaryTheme)
                .with_icon(Icon::Plus)
                .with_size(ButtonSize::Small)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AgentProfilesPageAction::CreateProfile);
                })
        });

        let me = Self {
            page: Self::build_page(),
            profile_views,
            add_profile_button,
        };
        me.refresh_add_profile_button(ctx);
        me
    }

    fn build_page() -> PageType<Self> {
        let widgets: Vec<Box<dyn SettingsWidget<View = Self>>> =
            vec![Box::new(AgentsWidget::default())];

        // This page is multi-section: it renders its own subheader-sized
        // section titles inside each widget, so it gets no page-level title.
        PageType::new_uncategorized(widgets, None)
    }

    fn refresh_add_profile_button(&self, ctx: &mut ViewContext<Self>) {
        let is_any_ai_enabled = AISettings::as_ref(ctx).is_any_ai_enabled();
        self.add_profile_button.update(ctx, |button, ctx| {
            button.set_disabled(!is_any_ai_enabled, ctx);
        });
    }

    fn create_profile_views(ctx: &mut ViewContext<Self>) -> Vec<ViewHandle<ExecutionProfileView>> {
        let profiles_model = AIExecutionProfilesModel::as_ref(ctx);
        let profile_ids = profiles_model.get_all_profile_ids();

        profile_ids
            .iter()
            .map(|profile_id| {
                let profile_id = profile_id.clone();
                let profile_view = ctx.add_typed_action_view(|ctx| {
                    ExecutionProfileView::new(profile_id.clone(), ctx)
                });
                let profile_id_for_event = profile_id.clone();

                ctx.subscribe_to_view(&profile_view, move |_me, _, event, ctx| match event {
                    ExecutionProfileViewEvent::EditProfile => {
                        ctx.emit(AgentProfilesPageEvent::OpenExecutionProfileEditor(
                            profile_id_for_event.clone(),
                        ));
                    }
                });

                profile_view
            })
            .collect()
    }

    fn refresh_profile_views(&mut self, ctx: &mut ViewContext<Self>) {
        let new_profile_views = Self::create_profile_views(ctx);
        self.profile_views = new_profile_views;
    }
}

impl View for AgentProfilesPageView {
    fn ui_name() -> &'static str {
        "AgentProfilesPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

pub enum AgentProfilesPageEvent {
    OpenExecutionProfileEditor(ExecutionProfileId),
}

impl Entity for AgentProfilesPageView {
    type Event = AgentProfilesPageEvent;
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentProfilesPageAction {
    ToggleCodebaseContext,
    CreateProfile,
}

impl TypedActionView for AgentProfilesPageView {
    type Action = AgentProfilesPageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            AgentProfilesPageAction::ToggleCodebaseContext => {
                match CodeSettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings.codebase_context_enabled.toggle_and_save_value(ctx)
                }) {
                    Ok(_new_value) => {}
                    Err(e) => {
                        log::warn!("Failed to set value for Codebase Context: {e:?}");
                    }
                }
                ctx.notify();
            }
            AgentProfilesPageAction::CreateProfile => {
                let new_profile_id = AIExecutionProfilesModel::handle(ctx)
                    .update(ctx, |model, ctx| model.create_profile(ctx));

                if let Some(profile_id) = new_profile_id {
                    ctx.emit(AgentProfilesPageEvent::OpenExecutionProfileEditor(
                        profile_id,
                    ));
                }
                ctx.notify();
            }
        }
    }
}

impl SettingsPageMeta for AgentProfilesPageView {
    fn section() -> SettingsSection {
        SettingsSection::AgentProfiles
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        true
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

impl From<ViewHandle<AgentProfilesPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<AgentProfilesPageView>) -> Self {
        SettingsPageViewHandle::AgentProfiles(view_handle)
    }
}

pub fn init_actions_from_parent_view<T: Action + Clone>(
    app: &mut AppContext,
    context: &ContextPredicate,
    builder: fn(SettingsAction) -> T,
) {
    ToggleSettingActionPair::add_toggle_setting_action_pairs_as_bindings(
        vec![ToggleSettingActionPair::new(
            "codebase index",
            builder(SettingsAction::AgentProfiles(
                AgentProfilesPageAction::ToggleCodebaseContext,
            )),
            &(context.clone() & id!(flags::IS_ANY_AI_ENABLED)),
            flags::IS_CODEBASE_INDEXING_ENABLED,
        )],
        app,
    );
}

#[derive(Default)]
struct AgentsWidget {}

impl SettingsWidget for AgentsWidget {
    type View = AgentProfilesPageView;

    fn search_terms(&self) -> &str {
        "ai a.i. agent autonomy profiles allowlist denylist autoexecute permissions models llms planning mcp server"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let ai_settings = AISettings::as_ref(app);
        Container::new(self.render_profiles_section(view, ai_settings, appearance, app))
            .with_margin_bottom(8.)
            .finish()
    }
}

impl AgentsWidget {
    fn render_profiles_section(
        &self,
        view: &AgentProfilesPageView,
        ai_settings: &AISettings,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let is_any_ai_enabled = ai_settings.is_any_ai_enabled();

        let header_and_description = Flex::column()
            .with_child(
                build_sub_header(
                    appearance,
                    "Profiles",
                    Some(styles::header_font_color(is_any_ai_enabled, app)),
                )
                .finish(),
            )
            .with_child(
                Container::new(
                    render_ai_setting_description(
                        "Profiles let you define how your Agent operates — from the actions it can take and when it needs approval, to the models it uses for tasks like coding and planning. You can also scope them to individual projects.",
                        is_any_ai_enabled,
                        app,
                    )
                )
                .with_margin_top(12.)
                .finish()
            )
            .finish();

        let mut profiles_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(Shrinkable::new(1., header_and_description).finish());

        profiles_row.add_child(
            Container::new(view.add_profile_button.as_ref(app).render(app))
                .with_margin_left(16.)
                .finish(),
        );

        let profiles_header = Container::new(profiles_row.finish())
            .with_margin_bottom(12.0)
            .finish();

        let mut profile_elements = vec![profiles_header];

        for profile_view in &view.profile_views {
            profile_elements.push(
                Container::new(ChildView::new(profile_view).finish())
                    .with_margin_bottom(8.)
                    .finish(),
            );
        }

        Flex::column().with_children(profile_elements).finish()
    }
}
