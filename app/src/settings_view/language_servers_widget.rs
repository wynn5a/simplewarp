//! The "Language servers" section of the Editor and Code Review settings page: each workspace's
//! enabled and disabled LSP servers, with their live status and controls.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lsp::supported_servers::LSPServerType;
use lsp::{LspManagerModel, LspServerModel, LspState};
use pathfinder_color::ColorU;
use warp_core::ui::theme::{AnsiColorIdentifier, WarpTheme};
use warp_util::path::user_friendly_path;
use warpui::elements::{
    ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Element, Flex, MainAxisAlignment,
    MainAxisSize, MouseStateHandle, ParentElement, Radius, Rect, Shrinkable,
};
use warpui::fonts::Weight;
use warpui::platform::Cursor;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::ui_components::switch::SwitchStateHandle;
use warpui::{AppContext, ModelHandle, SingletonEntity};

use super::code_editor_review_page::{EditorAndCodeReviewPageAction, EditorAndCodeReviewPageView};
use super::settings_page::{SettingsWidget, render_sub_header};
use crate::ai::persisted_workspace::{EnablementState, PersistedWorkspace};
use crate::appearance::Appearance;
use crate::code::lsp_logs::log_file_path;

const SECTION_TITLE: &str = "Language servers";
const EMPTY_TEXT: &str = "No language servers are set up yet. Enable one from the language support prompt in the code review panel or the code editor footer.";
const STATUS_DOT_SIZE: f32 = 8.;
const ROW_SPACING: f32 = 8.;

#[derive(Default)]
struct RowMouseStates {
    start_or_stop: MouseStateHandle,
    restart: MouseStateHandle,
    view_logs: MouseStateHandle,
    enabled: SwitchStateHandle,
}

/// Widget id for deep-linking to the language servers section.
pub fn language_servers_widget_id() -> &'static str {
    LanguageServersWidget::static_widget_id()
}

#[derive(Default)]
pub(super) struct LanguageServersWidget {
    /// Keyed by workspace root and server type, so each row keeps its handles across renders.
    row_mouse_states: RefCell<HashMap<(PathBuf, LSPServerType), RowMouseStates>>,
}

impl SettingsWidget for LanguageServersWidget {
    type View = EditorAndCodeReviewPageView;

    fn search_terms(&self) -> &str {
        "language servers lsp rust-analyzer gopls manage restart stop start logs enable disable"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let mut column = Flex::column()
            .with_spacing(ROW_SPACING)
            .with_child(render_sub_header(appearance, SECTION_TITLE));

        let workspaces = PersistedWorkspace::as_ref(app).configured_lsp_servers();
        if workspaces.is_empty() {
            column.add_child(render_secondary_text(EMPTY_TEXT, appearance));
            return column.finish();
        }

        let lsp_manager = LspManagerModel::as_ref(app);
        let mut row_mouse_states = self.row_mouse_states.borrow_mut();
        for (workspace_path, servers) in workspaces {
            column.add_child(render_workspace_label(&workspace_path, appearance));
            for (server_type, enablement) in servers {
                let server =
                    lsp_manager
                        .servers_for_workspace(&workspace_path)
                        .and_then(|servers| {
                            servers
                                .iter()
                                .find(|server| server.as_ref(app).server_type() == server_type)
                        });
                let mouse_states = row_mouse_states
                    .entry((workspace_path.clone(), server_type))
                    .or_default();
                column.add_child(render_server_row(
                    &workspace_path,
                    server_type,
                    enablement == EnablementState::Yes,
                    server,
                    mouse_states,
                    appearance,
                    app,
                ));
            }
        }
        column.finish()
    }
}

fn render_secondary_text(text: &str, appearance: &Appearance) -> Box<dyn Element> {
    appearance
        .ui_builder()
        .paragraph(text.to_owned())
        .with_style(UiComponentStyles {
            font_size: Some(12.),
            font_color: Some(appearance.theme().disabled_ui_text_color().into()),
            ..Default::default()
        })
        .build()
        .finish()
}

fn render_workspace_label(workspace_path: &Path, appearance: &Appearance) -> Box<dyn Element> {
    let full_path = workspace_path.to_string_lossy();
    let home = dirs::home_dir().map(|home| home.to_string_lossy().into_owned());
    let label = user_friendly_path(&full_path, home.as_deref()).into_owned();
    Container::new(
        appearance
            .ui_builder()
            .span(label)
            .with_style(UiComponentStyles {
                font_family_id: Some(appearance.monospace_font_family()),
                font_size: Some(12.),
                font_weight: Some(Weight::Bold),
                font_color: Some(appearance.theme().active_ui_text_color().into()),
                ..Default::default()
            })
            .build()
            .finish(),
    )
    .with_margin_top(4.)
    .finish()
}

fn render_server_row(
    workspace_path: &Path,
    server_type: LSPServerType,
    is_enabled: bool,
    server: Option<&ModelHandle<LspServerModel>>,
    mouse_states: &RowMouseStates,
    appearance: &Appearance,
    app: &AppContext,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let ui_builder = appearance.ui_builder();
    let state = server.map(|server| server.as_ref(app).state());
    let (status_color, status_text) = status(is_enabled, server, app, theme);

    let status_dot = ConstrainedBox::new(
        Rect::new()
            .with_background_color(status_color)
            .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.)))
            .finish(),
    )
    .with_width(STATUS_DOT_SIZE)
    .with_height(STATUS_DOT_SIZE)
    .finish();

    let name_and_status = Flex::column()
        .with_spacing(2.)
        .with_child(
            ui_builder
                .span(server_type.binary_name())
                .with_style(UiComponentStyles {
                    font_size: Some(12.),
                    font_color: Some(theme.active_ui_text_color().into()),
                    ..Default::default()
                })
                .build()
                .finish(),
        )
        .with_child(
            ui_builder
                .span(status_text)
                .with_style(UiComponentStyles {
                    font_size: Some(12.),
                    font_color: Some(if matches!(state, Some(LspState::Failed { .. })) {
                        status_color
                    } else {
                        theme.disabled_ui_text_color().into()
                    }),
                    ..Default::default()
                })
                .build()
                .finish(),
        )
        .finish();

    let left = Flex::row()
        .with_spacing(10.)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(status_dot)
        .with_child(Shrinkable::new(1., name_and_status).finish())
        .finish();

    let mut right = Flex::row()
        .with_spacing(ROW_SPACING)
        .with_cross_axis_alignment(CrossAxisAlignment::Center);

    if is_enabled && let (Some(server), Some(state)) = (server, state) {
        let running = matches!(state, LspState::Available { .. } | LspState::Starting);
        let start_or_stop_action = if running {
            EditorAndCodeReviewPageAction::StopLspServer(server.clone())
        } else {
            EditorAndCodeReviewPageAction::StartLspServer(server.clone())
        };
        if !matches!(state, LspState::Stopping { .. }) {
            right.add_child(render_button(
                if running { "Stop" } else { "Start" },
                mouse_states.start_or_stop.clone(),
                start_or_stop_action,
                appearance,
            ));
        }
        if matches!(
            state,
            LspState::Available { .. } | LspState::Starting | LspState::Failed { .. }
        ) {
            right.add_child(render_button(
                "Restart",
                mouse_states.restart.clone(),
                EditorAndCodeReviewPageAction::RestartLspServer(server.clone()),
                appearance,
            ));
        }
        if server.as_ref(app).has_started() || matches!(state, LspState::Failed { .. }) {
            right.add_child(render_button(
                "View logs",
                mouse_states.view_logs.clone(),
                EditorAndCodeReviewPageAction::OpenLspLogs {
                    log_path: log_file_path(server_type, workspace_path),
                },
                appearance,
            ));
        }
    }

    let toggle_action = EditorAndCodeReviewPageAction::SetLspServerEnabled {
        workspace_path: workspace_path.to_path_buf(),
        server_type,
        enabled: !is_enabled,
    };
    right.add_child(
        ui_builder
            .switch(mouse_states.enabled.clone())
            .check(is_enabled)
            .build()
            .on_click(move |ctx, _, _| ctx.dispatch_typed_action(toggle_action.clone()))
            .finish(),
    );

    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Shrinkable::new(1., left).finish())
            .with_child(right.finish())
            .finish(),
    )
    .with_uniform_padding(12.)
    .with_background(theme.surface_2())
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
    .finish()
}

fn render_button(
    label: &str,
    mouse_state: MouseStateHandle,
    action: EditorAndCodeReviewPageAction,
    appearance: &Appearance,
) -> Box<dyn Element> {
    appearance
        .ui_builder()
        .button(ButtonVariant::Secondary, mouse_state)
        .with_style(UiComponentStyles {
            font_size: Some(12.),
            ..Default::default()
        })
        .with_text_label(label.to_owned())
        .build()
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
        .finish()
}

fn status(
    is_enabled: bool,
    server: Option<&ModelHandle<LspServerModel>>,
    app: &AppContext,
    theme: &WarpTheme,
) -> (ColorU, String) {
    let ansi = |color: AnsiColorIdentifier| -> ColorU {
        color.to_ansi_color(&theme.terminal_colors().normal).into()
    };
    let muted = theme.disabled_ui_text_color().into_solid();
    if !is_enabled {
        return (muted, "Disabled".to_owned());
    }
    let Some(server) = server.map(|server| server.as_ref(app)) else {
        return (muted, "Not running".to_owned());
    };
    let (color, text) = match server.state() {
        LspState::Available { .. } if server.has_pending_tasks() => {
            (ansi(AnsiColorIdentifier::Yellow), "Indexing")
        }
        LspState::Available { .. } => (ansi(AnsiColorIdentifier::Green), "Running"),
        LspState::Starting => (ansi(AnsiColorIdentifier::Yellow), "Starting"),
        LspState::Stopping { .. } => (muted, "Stopping"),
        LspState::Stopped { .. } => (muted, "Stopped"),
        LspState::Failed { error } => {
            return (ansi(AnsiColorIdentifier::Red), format!("Failed: {error}"));
        }
    };
    (color, text.to_owned())
}
