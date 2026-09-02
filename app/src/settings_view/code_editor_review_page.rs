//! The "Editor and Code Review" settings page, shown under the Code umbrella.

use warp_core::features::FeatureFlag;
use warp_core::settings::ToggleableSetting as _;
use warp_errors::report_if_error;
use warpui::elements::Element;
#[cfg(feature = "local_fs")]
use warpui::elements::Empty;
use warpui::ui_components::components::UiComponent;
use warpui::ui_components::switch::SwitchStateHandle;
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle};

use super::settings_page::{
    MatchData, PageType, SettingsPageMeta, SettingsPageViewHandle, SettingsWidget, render_body_item,
};
use super::{LocalOnlyIconState, SettingsSection, ToggleState};
use crate::appearance::Appearance;
use crate::settings::CodeSettings;
use crate::terminal::general_settings::GeneralSettings;
use crate::workspace::tab_settings::TabSettings;
use crate::{TelemetryEvent, send_telemetry_from_ctx};

const PAGE_TITLE: &str = "Editor and Code Review";

pub struct EditorAndCodeReviewPageView {
    page: PageType<Self>,
}

impl EditorAndCodeReviewPageView {
    pub fn new(_ctx: &mut ViewContext<Self>) -> Self {
        Self {
            page: Self::build_page(),
        }
    }

    fn build_page() -> PageType<Self> {
        #[cfg(feature = "local_fs")]
        let mut widgets: Vec<Box<dyn SettingsWidget<View = Self>>> =
            vec![Box::new(ExternalEditorCodeWidget)];
        #[cfg(not(feature = "local_fs"))]
        let mut widgets: Vec<Box<dyn SettingsWidget<View = Self>>> = vec![];

        widgets.extend([
            Box::new(AutoOpenCodeReviewPaneCodeWidget::default())
                as Box<dyn SettingsWidget<View = Self>>,
            Box::new(CodeReviewPanelToggleWidget::default()),
            Box::new(CodeReviewDiffStatsToggleWidget::default()),
            Box::new(ProjectExplorerToggleWidget::default()),
            Box::new(GlobalSearchToggleWidget::default()),
            Box::new(ShowHiddenFilesToggleWidget::default()),
            Box::new(FormatOnSaveToggleWidget::default()),
            Box::new(AutoSaveToggleWidget::default()),
        ]);

        PageType::new_uncategorized(widgets, Some(PAGE_TITLE))
    }
}

impl Entity for EditorAndCodeReviewPageView {
    type Event = ();
}

impl View for EditorAndCodeReviewPageView {
    fn ui_name() -> &'static str {
        "EditorAndCodeReviewPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

/// Every setting on this page is a boolean toggle.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone)]
pub enum EditorAndCodeReviewPageAction {
    ToggleCodeReviewPanel,
    ToggleShowCodeReviewDiffStats,
    ToggleAutoOpenCodeReviewPane,
    ToggleProjectExplorer,
    ToggleGlobalSearch,
    ToggleShowHiddenFiles,
    ToggleFormatOnSave,
    ToggleAutoSave,
}

impl TypedActionView for EditorAndCodeReviewPageView {
    type Action = EditorAndCodeReviewPageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            EditorAndCodeReviewPageAction::ToggleCodeReviewPanel => {
                TabSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.show_code_review_button.toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            EditorAndCodeReviewPageAction::ToggleShowCodeReviewDiffStats => {
                TabSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(
                        settings
                            .show_code_review_diff_stats
                            .toggle_and_save_value(ctx)
                    );
                });
                ctx.notify();
            }
            EditorAndCodeReviewPageAction::ToggleProjectExplorer => {
                CodeSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.show_project_explorer.toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            EditorAndCodeReviewPageAction::ToggleGlobalSearch => {
                CodeSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.show_global_search.toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            EditorAndCodeReviewPageAction::ToggleShowHiddenFiles => {
                CodeSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.show_hidden_files.toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            EditorAndCodeReviewPageAction::ToggleFormatOnSave => {
                CodeSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.format_on_save.toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            EditorAndCodeReviewPageAction::ToggleAutoSave => {
                CodeSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.auto_save.toggle_and_save_value(ctx));
                });
                ctx.notify();
            }
            EditorAndCodeReviewPageAction::ToggleAutoOpenCodeReviewPane => {
                GeneralSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(
                        settings
                            .auto_open_code_review_pane_on_first_agent_change
                            .toggle_and_save_value(ctx)
                    );
                });
                send_telemetry_from_ctx!(
                    TelemetryEvent::FeaturesPageAction {
                        action: "ToggleAutoOpenCodeReviewPane".to_string(),
                        value: format!(
                            "{}",
                            *GeneralSettings::as_ref(ctx)
                                .auto_open_code_review_pane_on_first_agent_change
                        )
                    },
                    ctx
                );
                ctx.notify();
            }
        }
    }
}

impl SettingsPageMeta for EditorAndCodeReviewPageView {
    fn section() -> SettingsSection {
        SettingsSection::EditorAndCodeReview
    }

    fn update_filter(&mut self, query: &str, ctx: &mut ViewContext<Self>) -> MatchData {
        self.page.update_filter(query, ctx)
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        FeatureFlag::FullSourceCodeEmbedding.is_enabled()
    }

    fn scroll_to_widget(&mut self, widget_id: &'static str) {
        self.page.scroll_to_widget(widget_id)
    }

    fn clear_highlighted_widget(&mut self) {
        self.page.clear_highlighted_widget();
    }
}

impl From<ViewHandle<EditorAndCodeReviewPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<EditorAndCodeReviewPageView>) -> Self {
        SettingsPageViewHandle::EditorAndCodeReview(view_handle)
    }
}

#[cfg(feature = "local_fs")]
struct ExternalEditorCodeWidget;

#[cfg(feature = "local_fs")]
impl SettingsWidget for ExternalEditorCodeWidget {
    type View = EditorAndCodeReviewPageView;

    fn search_terms(&self) -> &str {
        "code editor open files markdown AI conversations layout pane tab"
    }

    fn render(
        &self,
        _view: &Self::View,
        _appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        Empty::new().finish()
    }
}

#[derive(Default)]
struct AutoOpenCodeReviewPaneCodeWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for AutoOpenCodeReviewPaneCodeWidget {
    type View = EditorAndCodeReviewPageView;

    fn search_terms(&self) -> &str {
        "oz auto open code review pane panel agent mode change first time accepted diff view conversation"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let general_settings = GeneralSettings::as_ref(app);
        render_body_item::<EditorAndCodeReviewPageAction>(
            "Auto open code review panel".into(),
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*general_settings.auto_open_code_review_pane_on_first_agent_change)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(
                        EditorAndCodeReviewPageAction::ToggleAutoOpenCodeReviewPane,
                    );
                })
                .finish(),
            Some("When this setting is on, the code review panel will open on the first accepted diff of a conversation".into()),
        )
    }
}

#[derive(Default)]
struct CodeReviewPanelToggleWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for CodeReviewPanelToggleWidget {
    type View = EditorAndCodeReviewPageView;

    fn search_terms(&self) -> &str {
        "code review panel right side diff git"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let tab_settings = TabSettings::as_ref(app);

        render_body_item::<EditorAndCodeReviewPageAction>(
            "Show code review button".into(),
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*tab_settings.show_code_review_button)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(EditorAndCodeReviewPageAction::ToggleCodeReviewPanel);
                })
                .finish(),
            Some(
                "Show a button in the top right of the window to toggle the code review panel."
                    .into(),
            ),
        )
    }
}

#[derive(Default)]
struct CodeReviewDiffStatsToggleWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for CodeReviewDiffStatsToggleWidget {
    type View = EditorAndCodeReviewPageView;

    fn search_terms(&self) -> &str {
        "code review diff stats lines added removed counts"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let tab_settings = TabSettings::as_ref(app);

        render_body_item::<EditorAndCodeReviewPageAction>(
            "Show diff stats on code review button".into(),
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*tab_settings.show_code_review_diff_stats)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(
                        EditorAndCodeReviewPageAction::ToggleShowCodeReviewDiffStats,
                    );
                })
                .finish(),
            Some("Show lines added and removed counts on the code review button.".into()),
        )
    }
}

#[derive(Default)]
struct ProjectExplorerToggleWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for ProjectExplorerToggleWidget {
    type View = EditorAndCodeReviewPageView;

    fn search_terms(&self) -> &str {
        "project explorer file tree left panel tools"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let code_settings = CodeSettings::as_ref(app);

        render_body_item::<EditorAndCodeReviewPageAction>(
            "Project explorer".into(),
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*code_settings.show_project_explorer)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(EditorAndCodeReviewPageAction::ToggleProjectExplorer);
                })
                .finish(),
            Some(
                "Adds an IDE-style project explorer / file tree to the left side tools panel."
                    .into(),
            ),
        )
    }
}

#[derive(Default)]
struct GlobalSearchToggleWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for GlobalSearchToggleWidget {
    type View = EditorAndCodeReviewPageView;

    fn search_terms(&self) -> &str {
        "global search file search left panel tools"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let code_settings = CodeSettings::as_ref(app);

        render_body_item::<EditorAndCodeReviewPageAction>(
            "Global file search".into(),
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*code_settings.show_global_search)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(EditorAndCodeReviewPageAction::ToggleGlobalSearch);
                })
                .finish(),
            Some("Adds global file search to the left side tools panel.".into()),
        )
    }
}

#[derive(Default)]
struct ShowHiddenFilesToggleWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for ShowHiddenFilesToggleWidget {
    type View = EditorAndCodeReviewPageView;

    fn search_terms(&self) -> &str {
        "show hidden files dotfiles project explorer file tree"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let code_settings = CodeSettings::as_ref(app);

        render_body_item::<EditorAndCodeReviewPageAction>(
            "Show hidden files in project explorer".into(),
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*code_settings.show_hidden_files)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(EditorAndCodeReviewPageAction::ToggleShowHiddenFiles);
                })
                .finish(),
            Some(
                "Show dotfiles and hidden files (starting with .) in the project explorer.".into(),
            ),
        )
    }
}

#[derive(Default)]
struct FormatOnSaveToggleWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for FormatOnSaveToggleWidget {
    type View = EditorAndCodeReviewPageView;

    fn search_terms(&self) -> &str {
        "format on save lsp language server formatting reformat editor"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let code_settings = CodeSettings::as_ref(app);

        render_body_item::<EditorAndCodeReviewPageAction>(
            "Format on save (requires an active language server)".into(),
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*code_settings.format_on_save)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(EditorAndCodeReviewPageAction::ToggleFormatOnSave);
                })
                .finish(),
            Some(
                "Only applies when a language server is active for the file. Automatically formats the file with the language server on save; other LSP features (hover, go-to-definition, references, diagnostics) are unaffected."
                    .into(),
            ),
        )
    }
}

#[derive(Default)]
struct AutoSaveToggleWidget {
    switch_state: SwitchStateHandle,
}

impl SettingsWidget for AutoSaveToggleWidget {
    type View = EditorAndCodeReviewPageView;

    fn search_terms(&self) -> &str {
        "auto save autosave automatically save editor files on type focus"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let code_settings = CodeSettings::as_ref(app);

        render_body_item::<EditorAndCodeReviewPageAction>(
            "Auto save".into(),
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(*code_settings.auto_save)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(EditorAndCodeReviewPageAction::ToggleAutoSave);
                })
                .finish(),
            Some(
                "Automatically saves changes in the Warp text editor as you type and when the editor loses focus."
                    .into(),
            ),
        )
    }
}
