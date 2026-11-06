//! The "Editor and Code Review" settings page, shown under the Code umbrella.

use std::collections::HashSet;
use std::path::PathBuf;

use lsp::supported_servers::LSPServerType;
use lsp::{LanguageServerId, LspManagerModel, LspManagerModelEvent, LspServerModel};
use warp_core::settings::ToggleableSetting as _;
use warp_errors::report_if_error;
use warpui::elements::{Element, Empty};
use warpui::ui_components::components::UiComponent;
use warpui::ui_components::switch::SwitchStateHandle;
use warpui::{
    AppContext, Entity, ModelHandle, SingletonEntity, TypedActionView, View, ViewContext,
    ViewHandle,
};

use super::language_servers_widget::LanguageServersWidget;
use super::settings_page::{
    MatchData, PageType, SettingsPageMeta, SettingsPageViewHandle, SettingsWidget, render_body_item,
};
use super::{SettingsSection, ToggleState};
use crate::ai::persisted_workspace::{LspTask, PersistedWorkspace};
use crate::appearance::Appearance;
use crate::settings::CodeSettings;
use crate::terminal::general_settings::GeneralSettings;
use crate::workspace::tab_settings::TabSettings;

const PAGE_TITLE: &str = "Editor and Code Review";

pub struct EditorAndCodeReviewPageView {
    page: PageType<Self>,
    /// Language servers whose state changes re-render the page.
    observed_servers: HashSet<LanguageServerId>,
}

impl EditorAndCodeReviewPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let lsp_manager = LspManagerModel::handle(ctx);
        ctx.subscribe_to_model(&lsp_manager, |me, _, event, ctx| {
            match event {
                LspManagerModelEvent::ServerRegistered(_) => me.observe_servers(ctx),
                LspManagerModelEvent::ServerStarted(_)
                | LspManagerModelEvent::ServerStopped(_)
                | LspManagerModelEvent::ServerRemoved { .. } => {}
            }
            ctx.notify();
        });
        ctx.subscribe_to_model(&PersistedWorkspace::handle(ctx), |_, _, _, ctx| {
            ctx.notify()
        });

        let mut page = Self {
            page: Self::build_page(),
            observed_servers: HashSet::new(),
        };
        page.observe_servers(ctx);
        page
    }

    fn observe_servers(&mut self, ctx: &mut ViewContext<Self>) {
        let servers: Vec<ModelHandle<LspServerModel>> = LspManagerModel::as_ref(ctx)
            .workspace_roots()
            .filter_map(|root| LspManagerModel::as_ref(ctx).servers_for_workspace(root))
            .flatten()
            .cloned()
            .collect();
        for server in servers {
            if self.observed_servers.insert(server.as_ref(ctx).id()) {
                ctx.subscribe_to_model(&server, |_, _, _, ctx| ctx.notify());
            }
        }
    }

    fn build_page() -> PageType<Self> {
        let mut widgets: Vec<Box<dyn SettingsWidget<View = Self>>> =
            vec![Box::new(ExternalEditorCodeWidget)];

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
            Box::new(LanguageServersWidget::default()),
        ]);

        PageType::new_uncategorized(widgets, Some(PAGE_TITLE))
    }
}

pub enum EditorAndCodeReviewPageEvent {
    OpenLspLogs { log_path: PathBuf },
}

impl Entity for EditorAndCodeReviewPageView {
    type Event = EditorAndCodeReviewPageEvent;
}

impl View for EditorAndCodeReviewPageView {
    fn ui_name() -> &'static str {
        "EditorAndCodeReviewPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

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
    StartLspServer(ModelHandle<LspServerModel>),
    StopLspServer(ModelHandle<LspServerModel>),
    RestartLspServer(ModelHandle<LspServerModel>),
    OpenLspLogs {
        log_path: PathBuf,
    },
    /// Enabling also spawns the server; disabling stops and unregisters it.
    SetLspServerEnabled {
        workspace_path: PathBuf,
        server_type: LSPServerType,
        enabled: bool,
    },
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
            EditorAndCodeReviewPageAction::StartLspServer(server) => {
                server.update(ctx, |server, ctx| {
                    report_if_error!(server.manual_start(ctx))
                });
            }
            EditorAndCodeReviewPageAction::StopLspServer(server) => {
                server.update(ctx, |server, ctx| report_if_error!(server.stop(true, ctx)));
            }
            EditorAndCodeReviewPageAction::RestartLspServer(server) => {
                server.update(ctx, |server, ctx| server.restart(ctx));
            }
            EditorAndCodeReviewPageAction::OpenLspLogs { log_path } => {
                ctx.emit(EditorAndCodeReviewPageEvent::OpenLspLogs {
                    log_path: log_path.clone(),
                });
            }
            EditorAndCodeReviewPageAction::SetLspServerEnabled {
                workspace_path,
                server_type,
                enabled,
            } => {
                if *enabled {
                    PersistedWorkspace::handle(ctx).update(ctx, |workspace, ctx| {
                        workspace.enable_lsp_server_for_path(workspace_path, *server_type);
                        workspace.execute_lsp_task(
                            LspTask::Spawn {
                                file_path: workspace_path.clone(),
                            },
                            ctx,
                        );
                    });
                } else {
                    LspManagerModel::handle(ctx).update(ctx, |manager, ctx| {
                        manager.remove_server(workspace_path, *server_type, ctx);
                    });
                    PersistedWorkspace::handle(ctx).update(ctx, |workspace, _| {
                        workspace.disable_lsp_server_for_path(workspace_path, *server_type);
                    });
                }
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
        true
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

struct ExternalEditorCodeWidget;

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
                "Automatically saves changes in the SimpleWarp text editor as you type and when the editor loses focus."
                    .into(),
            ),
        )
    }
}
