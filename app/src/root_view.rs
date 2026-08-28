use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::SyncSender;

use anyhow::Result;
use itertools::Itertools;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::{Vector2F, vec2f};
use serde::{Deserialize, Serialize};
use settings::Setting as _;
use url::Url;
use warp_core::context_flag::ContextFlag;
use warp_core::safe_error;
use warp_core::user_preferences::GetUserPreferences as _;
use warp_errors::report_error;
use warpui::elements::{Border, ParentElement, Stack};
use warpui::keymap::{EditableBinding, FixedBinding};
use warpui::platform::{WindowBounds, WindowStyle};
use warpui::presenter::ChildView;
use warpui::rendering::OnGPUDeviceSelected;
use warpui::windowing::WindowManager;
use warpui::{
    AddWindowOptions, AppContext, DisplayId, Element, Entity, EntityId, FocusContext,
    NextNewWindowsHasThisWindowsBoundsUponClose, SingletonEntity, TypedActionView, View,
    ViewContext, ViewHandle, WindowId, id,
};

use crate::ai::agent::api::ServerConversationToken;
use crate::ai::blocklist::SerializedBlockListItem;
use crate::app_state::{AppState, PaneUuid, WindowSnapshot};
use crate::appearance::Appearance;
use crate::auth::AuthStateProvider;
use crate::auth::auth_manager::{AuthManager, AuthManagerEvent};
use crate::auth::auth_state::AuthState;
use crate::auth::auth_view_modal::AuthRedirectPayload;
use crate::autoupdate::{AutoupdateState, AutoupdateStateEvent, RequestType, UpdateReady};
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::{ObjectType, OpenWarpDriveObjectArgs, OpenWarpDriveObjectSettings};
use crate::interval_timer::IntervalTimer;
use crate::launch_configs::launch_config;
use crate::linear::LinearIssueWork;
use crate::pane_group::{NewTerminalOptions, PanesLayout};
use crate::persistence::ModelEvent;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::ids::{ServerId, SyncId};
use crate::server::server_api::auth::UserAuthenticationError;
use crate::server::server_api::{ServerApi, ServerApiProvider, ServerTime};
use crate::server::telemetry::{LaunchConfigUiLocation, TelemetryEvent};
use crate::settings::QuakeModeSettings;
use crate::settings_view::mcp_servers_page::MCPServersSettingsPage;
use crate::settings_view::{OpenTeamsSettingsModalArgs, SettingsSection, flags};
use crate::terminal::available_shells::AvailableShell;
use crate::terminal::general_settings::GeneralSettings;
use crate::terminal::keys_settings::KeysSettings;
use crate::terminal::shell::ShellType;
use crate::terminal::view::{TerminalAction, cell_size_and_padding};
use crate::themes::theme::{AnsiColorIdentifier, Blend, Fill};
use crate::uri::{OpenMCPSettingsArgs, OpenSettingsArgs};
use crate::util::bindings::{self, is_binding_pty_compliant};
use crate::view_components::DismissibleToast;
use crate::window_settings::WindowSettings;
use crate::workspace::{PaneViewLocator, Workspace, WorkspaceAction, WorkspaceRegistry};
use crate::workspaces::team_tester::TeamTesterStatus;
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::{
    ChannelState, GlobalResourceHandles, GlobalResourceHandlesProvider, UpdateQuakeModeEventArg,
    send_telemetry_from_app_ctx, send_telemetry_from_ctx,
};

const WINDOW_TITLE: &str = "Warp";

lazy_static! {
    static ref FALLBACK_WINDOW_SIZE: Vector2F = vec2f(800.0, 600.0);
    static ref QUAKE_STATE: Arc<Mutex<Option<QuakeModeState>>> = Arc::new(Mutex::new(None));
}

/// This is the color of the border wrapping the whole window.
///
/// On MacOS, this is drawn for us by the OS. On other platforms, we must draw it ourselves. Note
/// that this is hard-coded for the default Dark theme. This is because it is only used by the
/// AuthView and OnboardingSurveyModal which do not respect the chosen theme. So, do not use this for Views
/// which respect themes.
pub(crate) fn unthemed_window_border() -> Border {
    if cfg!(all(not(target_os = "macos"), not(target_family = "wasm"))) {
        // The 15% blend of fg into bg is the "ui surface" color.
        Border::all(1.).with_border_fill(Fill::black().blend(&Fill::white().with_opacity(15)))
    } else {
        Border::all(1.).with_border_fill(Fill::black().with_opacity(0))
    }
}

#[derive(Debug, Clone)]
enum WindowState {
    /// Quake mode window is open and visible on the screen.
    Open,
    /// Quake mode window is opening but has not become the key window yet.
    /// This happens when the app is not focused when the quake mode window
    /// is opened.
    PendingOpen,
    /// Quake mode window is open but hidden away from the screen.
    /// In this state, toggling quake mode will show the hidden window rather
    /// than creating a new one.
    Hidden,
}

#[derive(Debug, Clone)]
pub struct QuakeModeState {
    /// State of the opened quake mode window.
    window_state: WindowState,
    window_id: WindowId,
    /// ID of the active screen when we last positioned the quake mode window.
    /// Note that this is not necessarily the screen quake mode lives in if user
    /// set a specific pinned screen.
    active_display_id: DisplayId,
}

/// Configuration for the new quake mode window including the active screen id and the window bound.
struct QuakeModeFrameConfig {
    display_id: DisplayId,
    window_bounds: RectF,
}

/// Trigger of a potential quake window move.
#[derive(Debug)]
enum QuakeModeMoveTrigger {
    /// The screen configuration changed (plug / unplug monitor). We need
    /// to reposition quake mode as it might be in an invalid position.
    ScreenConfigurationChange,
    /// User set "active screen" as the screen to pin to. In this case,
    /// we will attempt to move the quake window if the active screen dimension
    /// changed. If it hasn't change, we will keep the window as is to avoid
    /// meaningless resizing.
    ActiveScreenSetting,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Hash,
    Eq,
    PartialEq,
    Deserialize,
    Serialize,
    Default,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "Screen edge to pin the hotkey window to.",
    rename_all = "snake_case"
)]
pub enum QuakeModePinPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

pub struct OpenFromRestoredArg {
    pub app_state: Option<AppState>,
}

pub struct OpenLaunchConfigArg {
    pub launch_config: launch_config::LaunchConfig,
    pub ui_location: LaunchConfigUiLocation,

    /// Tries to open the launch config into the active window, if any.
    ///
    /// Currently, this is only supported by single-window launch configs
    /// and will open the window tabs into the existing window when true.
    pub open_in_active_window: bool,
}

pub struct OpenPath {
    pub path: PathBuf,
}

// Arguments for actions that run a command that should start a subshell.
pub struct SubshellCommandArg {
    pub command: String,
    pub shell_type: Option<ShellType>,
}

// Arguments for creating an ambient agent environment.
pub struct CreateEnvironmentArg {
    pub repos: Vec<String>,
}

impl CreateEnvironmentArg {
    /// Formats the `/create-environment` slash command invocation.
    pub fn to_query(&self) -> String {
        // Filter repos to accept either valid URLs or POSIX portable pathnames for security.
        //
        // Note: we also allow *absolute* POSIX paths (e.g., /Users/me/repo) as long as every
        // component is portable. This is important for local indexed repos.
        let safe_repos = self
            .repos
            .iter()
            .filter(|repo| {
                // Accept valid URLs (e.g., https://github.com/user/repo)
                Url::parse(repo).is_ok()
                    // Or valid POSIX portable pathnames (e.g., user/repo)
                    || warp_util::path::is_posix_portable_pathname(repo)
                    // Or absolute POSIX paths with portable components (e.g., /Users/me/repo)
                    || repo
                        .strip_prefix('/')
                        .is_some_and(warp_util::path::is_posix_portable_pathname)
            })
            .join(" ");

        if safe_repos.is_empty() {
            // Include a trailing space to trigger slash command syntax highlighting and ghost text.
            "/create-environment ".to_string()
        } else {
            format!("/create-environment {}", safe_repos)
        }
    }
}

pub fn init(app: &mut AppContext) {
    app.register_binding_validator::<RootView>(is_binding_pty_compliant);

    app.add_global_action("root_view:open_from_restored", open_from_restored);
    app.add_global_action("root_view:open_new", open_new);
    app.add_global_action("root_view:open_new_with_shell", open_new_with_shell);
    app.add_global_action("root_view:open_new_from_path", |arg, ctx| {
        let _ = open_new_from_path(arg, ctx);
    });
    app.add_global_action(
        "root_view:open_new_tab_insert_subshell_command_and_bootstrap_if_supported",
        open_new_tab_insert_subshell_command_and_bootstrap_if_supported,
    );
    app.add_global_action("root_view:open_launch_config", open_launch_config);
    app.add_global_action("root_view:send_feedback", send_feedback);
    app.add_global_action(
        "root_view:toggle_quake_mode_window",
        toggle_quake_mode_window,
    );
    app.add_global_action(
        "root_view:show_or_hide_non_quake_mode_windows",
        show_or_hide_non_quake_mode_windows,
    );
    app.add_global_action("root_view:update_quake_mode_state", update_quake_mode_state);
    app.add_global_action(
        "root_view:move_quake_mode_window_from_screen_change",
        move_quake_mode_window_from_screen_change,
    );
    #[cfg(feature = "voice_input")]
    app.add_global_action("root_view:abort_voice_input", abort_voice_input);
    #[cfg(feature = "voice_input")]
    app.add_action(
        "root_view:maybe_stop_active_voice_input",
        RootView::maybe_stop_active_voice_input,
    );
    app.add_action("root_view:log_out", RootView::log_out);
    app.add_action(
        "root_view:handle_incoming_auth_url",
        RootView::handle_incoming_auth_url,
    );
    app.add_action(
        "root_view:add_session_at_path",
        RootView::add_session_at_path,
    );
    app.add_action(
        "root_view:handle_team_intent_link_action",
        RootView::handle_team_intent_link_action,
    );
    app.add_action(
        "root_view:open_team_settings_page",
        RootView::open_team_settings_page,
    );
    app.add_action(
        "root_view:handle_notification_click",
        RootView::handle_notification_click,
    );
    app.add_action(
        "root_view:handle_pane_navigation_event",
        RootView::focus_pane,
    );
    app.add_action(
        "root_view:activate_tab_by_pane_group_id",
        RootView::activate_tab_by_pane_group_id,
    );
    app.add_action("root_view:close_window", RootView::close_window);
    app.add_action("root_view:minimize_window", RootView::minimize_window);
    app.add_action(
        "root_view:toggle_maximize_window",
        RootView::toggle_maximize_window,
    );
    app.add_action("root_view:toggle_fullscreen", RootView::toggle_fullscreen);

    app.add_global_action(
        "root_view:open_conversation_viewer",
        open_conversation_viewer,
    );
    app.add_action(
        "root_view:open_cloud_conversation_in_existing_window",
        RootView::open_cloud_conversation_in_existing_window,
    );

    app.add_global_action("root_view:create_environment", create_environment);
    app.add_global_action(
        "root_view:create_environment_and_run",
        create_environment_and_run,
    );
    app.add_action(
        "root_view:create_environment_in_existing_window",
        RootView::create_environment_in_existing_window,
    );
    app.add_action(
        "root_view:create_environment_in_existing_window_and_run",
        RootView::create_environment_in_existing_window_and_run,
    );
    app.add_global_action(
        "root_view:open_drive_object_new_window",
        open_warp_drive_object,
    );
    app.add_action(
        "root_view:open_drive_object_existing_window",
        RootView::open_warp_drive_object_in_existing_window,
    );

    app.add_global_action(
        "root_view:open_team_settings_with_email_invite_in_new_window",
        open_team_settings_with_email_invite_in_new_window,
    );
    app.add_action(
        "root_view:open_team_settings_with_email_invite_in_existing_window",
        RootView::open_team_settings_with_email_invite_in_existing_window,
    );

    app.add_global_action(
        "root_view:open_settings_page_in_new_window",
        open_settings_page_in_new_window,
    );
    app.add_action(
        "root_view:open_settings_page_in_existing_window",
        RootView::open_settings_page_in_existing_window,
    );

    app.add_global_action(
        "root_view:open_settings_in_new_window",
        open_settings_in_new_window,
    );
    app.add_action(
        "root_view:open_settings_in_existing_window",
        RootView::open_settings_in_existing_window,
    );

    app.add_global_action(
        "root_view:open_mcp_settings_in_new_window",
        open_mcp_settings_in_new_window,
    );
    app.add_action(
        "root_view:open_mcp_settings_in_existing_window",
        RootView::open_mcp_settings_in_existing_window,
    );

    app.add_global_action(
        "root_view:open_codex_in_new_window",
        open_codex_in_new_window,
    );
    app.add_action(
        "root_view:open_codex_in_existing_window",
        RootView::open_codex_in_existing_window,
    );

    app.add_global_action(
        "root_view:open_linear_issue_work_in_new_window",
        open_linear_issue_work_in_new_window,
    );
    app.add_action(
        "root_view:open_linear_issue_work_in_existing_window",
        RootView::open_linear_issue_work_in_existing_window,
    );

    app.add_action("root_view:add_file_pane", RootView::add_file_pane);
    app.add_global_action(
        "root_view:open_new_with_file_notebook",
        open_new_with_file_notebook,
    );

    app.register_fixed_bindings([
        FixedBinding::empty(
            "Hide All Windows",
            RootViewAction::ShowOrHideNonQuakeModeWindows,
            id!("RootView") & id!(flags::ACTIVATION_HOTKEY_FLAG),
        ),
        FixedBinding::empty(
            "Show Dedicated Hotkey Window",
            RootViewAction::ToggleQuakeModeWindow,
            id!("RootView")
                & id!(flags::QUAKE_MODE_ENABLED_CONTEXT_FLAG)
                & !id!(flags::QUAKE_WINDOW_OPEN_FLAG),
        ),
        FixedBinding::empty(
            "Hide Dedicated Hotkey Window",
            RootViewAction::ToggleQuakeModeWindow,
            id!("RootView")
                & id!(flags::QUAKE_MODE_ENABLED_CONTEXT_FLAG)
                & id!(flags::QUAKE_WINDOW_OPEN_FLAG),
        ),
    ]);

    app.register_editable_bindings([
        // Register a binding to toggle fullscreen on Linux and Windows.
        EditableBinding::new(
            "root_view:toggle_fullscreen",
            "Toggle fullscreen",
            RootViewAction::ToggleFullscreen,
        )
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(id!("RootView"))
        .with_linux_or_windows_key_binding("f11"),
    ])
}

fn maybe_register_global_window_shortcuts(
    global_resource_handles: GlobalResourceHandles,
    ctx: &mut AppContext,
) {
    // let keys_settings = KeysSettings::handle(ctx).as_ref(ctx);
    if let Some(key) = KeysSettings::as_ref(ctx)
        .quake_mode_settings
        .keybinding
        .clone()
        .filter(|_| *KeysSettings::as_ref(ctx).quake_mode_enabled)
    {
        ctx.register_global_shortcut(
            key.clone(),
            "root_view:toggle_quake_mode_window",
            global_resource_handles,
        );
    }

    if let Some(key) = KeysSettings::as_ref(ctx)
        .activation_hotkey_keybinding
        .clone()
        .filter(|_| *KeysSettings::as_ref(ctx).activation_hotkey_enabled)
    {
        ctx.register_global_shortcut(
            key.clone(),
            "root_view:show_or_hide_non_quake_mode_windows",
            (),
        )
    }
}

/// Find the root [`Workspace`] view for the active window.
fn active_workspace(ctx: &mut AppContext) -> Option<ViewHandle<Workspace>> {
    let window_id = ctx.windows().active_window()?;
    WorkspaceRegistry::as_ref(ctx).get(window_id, ctx)
}

fn open_launch_config(arg: &OpenLaunchConfigArg, ctx: &mut AppContext) {
    let active_window_workspace = active_workspace(ctx);
    if arg.launch_config.windows.is_empty() {
        open_new(&(), ctx);
    } else if arg.open_in_active_window
        && arg.launch_config.windows.len() == 1
        && active_window_workspace.is_some()
    {
        active_window_workspace
            .expect("already checked if there is a workspace for the active window")
            .update(ctx, |workspace, ctx| {
                workspace.open_launch_config_window(arg.launch_config.windows[0].clone(), ctx)
            });
    } else {
        let mut active_index = None;
        for (idx, window_template) in arg.launch_config.windows.iter().enumerate() {
            if arg
                .launch_config
                .active_window_index
                .map(|window_idx| window_idx == idx)
                .unwrap_or(false)
            {
                active_index = Some(idx);
            } else {
                open_new_with_workspace_source(
                    NewWorkspaceSource::FromTemplate {
                        window_template: window_template.clone(),
                    },
                    ctx,
                );
            }
        }

        if let Some(idx) = active_index {
            let window_template = arg
                .launch_config
                .windows
                .get(idx)
                .expect("Window should exist at idx");

            open_new_with_workspace_source(
                NewWorkspaceSource::FromTemplate {
                    window_template: window_template.clone(),
                },
                ctx,
            );
        }
    }

    send_telemetry_from_app_ctx!(
        TelemetryEvent::OpenLaunchConfig {
            ui_location: crate::server::telemetry::LaunchConfigUiLocation::Uri,
            open_in_active_window: arg.open_in_active_window,
        },
        ctx
    );
}

fn send_feedback(_: &(), ctx: &mut AppContext) {
    match active_workspace(ctx) {
        Some(workspace) => {
            workspace.update(ctx, |workspace, ctx| {
                workspace.handle_action(&WorkspaceAction::SendFeedback, ctx);
            });
        }
        _ => {
            ctx.open_url(&crate::util::links::feedback_form_url());
        }
    }
}

/// Creates a new window with the transferred pane group.
///
/// If `is_tab_drag_preview` is true, the window is created without stealing
/// focus so it can follow the cursor during a tab drag.
///
/// Returns the new window ID.
pub fn create_transferred_window(
    transferred_tab: crate::workspace::view::TransferredTab,
    source_window_id: WindowId,
    window_size: Vector2F,
    window_position: Vector2F,
    is_tab_drag_preview: bool,
    ctx: &mut AppContext,
) -> WindowId {
    let global_resource_handles = GlobalResourceHandlesProvider::handle(ctx)
        .as_ref(ctx)
        .get()
        .clone();
    let window_settings = WindowSettings::handle(ctx).as_ref(ctx);

    let window_bounds = WindowBounds::ExactPosition(RectF::new(window_position, window_size));

    let window_style = if is_tab_drag_preview {
        WindowStyle::PositionedNoFocus
    } else {
        WindowStyle::Normal
    };

    let (new_window_id, _) = ctx.add_window(
        AddWindowOptions {
            window_style,
            window_bounds,
            title: Some(WINDOW_TITLE.to_owned()),
            background_blur_radius_pixels: Some(*window_settings.background_blur_radius),
            background_blur_texture: *window_settings.background_blur_texture,
            on_gpu_driver_selected: on_gpu_driver_selected_callback(),
            ..Default::default()
        },
        |ctx| {
            let mut view = RootView::new(
                global_resource_handles.clone(),
                NewWorkspaceSource::TransferredTab {
                    source_window_id,
                    tab_color: transferred_tab.color,
                    custom_title: transferred_tab.custom_title.clone(),
                    left_panel_open: transferred_tab.left_panel_open,
                    vertical_tabs_panel_open: transferred_tab.vertical_tabs_panel_open,
                    right_panel_open: transferred_tab.right_panel_open,
                    is_right_panel_maximized: transferred_tab.is_right_panel_maximized,
                    is_tab_drag_preview,
                },
                ctx,
            );
            if !is_tab_drag_preview {
                view.focus(ctx);
            }
            view
        },
    );

    let pane_group_id = transferred_tab.pane_group.id();
    ctx.transfer_view_tree_to_window(pane_group_id, source_window_id, new_window_id);

    match WorkspaceRegistry::as_ref(ctx).get(new_window_id, ctx) {
        Some(new_workspace) => {
            new_workspace.update(ctx, |workspace, ctx| {
                workspace.adopt_transferred_pane_group(transferred_tab.pane_group.clone(), ctx);
            });
        }
        _ => {
            log::warn!("Failed to find workspace in newly created window {new_window_id:?}");
        }
    }
    new_window_id
}

#[cfg(feature = "crash_reporting")]
fn on_gpu_driver_selected_callback() -> Option<Box<OnGPUDeviceSelected>> {
    Some(Box::new(|gpu_device_info| {
        crate::crash_reporting::set_gpu_device_info(gpu_device_info)
    }))
}

#[cfg(not(feature = "crash_reporting"))]
fn on_gpu_driver_selected_callback() -> Option<Box<OnGPUDeviceSelected>> {
    None
}

fn open_from_restored(arg: &OpenFromRestoredArg, ctx: &mut AppContext) {
    let global_resource_handles = GlobalResourceHandlesProvider::as_ref(ctx).get().clone();
    IntervalTimer::handle(ctx).update(ctx, |timer, _| {
        timer.mark_interval_end("HANDLING_OPEN_ACTION");
    });

    if let Some(app_state) = &arg.app_state {
        maybe_register_global_window_shortcuts(global_resource_handles.clone(), ctx);

        let (background_blur_radius_pixels, background_blur_texture) = {
            let window_settings = WindowSettings::as_ref(ctx);
            (
                Some(*window_settings.background_blur_radius),
                *window_settings.background_blur_texture,
            )
        };

        // Check whether user has enabled session restoration.
        if *GeneralSettings::as_ref(ctx).restore_session {
            let mut active_index = None;
            let mut normal_window_count = 0;
            for (idx, window) in app_state.windows.iter().enumerate() {
                // If this window is a quake window, hide it by default.
                if window.quake_mode {
                    // If this is Windows, skip restoring the quake window. Creating a hidden window
                    // is not supported on Windows. We can't have the quake window visible on
                    // startup or else it will get mistaken for a normal window.
                    if cfg!(windows) {
                        continue;
                    }
                    let frame_args = quake_mode_config(
                        &KeysSettings::as_ref(ctx)
                            .quake_mode_settings
                            .value()
                            .clone(),
                        ctx,
                    );

                    let (id, _) = ctx.add_window(
                        AddWindowOptions {
                            window_style: WindowStyle::Pin,
                            window_bounds: WindowBounds::ExactPosition(frame_args.window_bounds),
                            title: Some("Warp".to_owned()),
                            fullscreen_state: window.fullscreen_state,
                            background_blur_radius_pixels,
                            background_blur_texture,
                            // Don't use the quake window for positioning new windows.
                            anchor_new_windows_from_closed_position:
                                NextNewWindowsHasThisWindowsBoundsUponClose::No,
                            on_gpu_driver_selected: on_gpu_driver_selected_callback(),
                            window_instance: Some(ChannelState::app_id().to_string() + "-hotkey"),
                        },
                        |ctx| {
                            let mut view = RootView::new(
                                global_resource_handles.clone(),
                                NewWorkspaceSource::Restored {
                                    window_snapshot: window.clone(),
                                    block_lists: app_state.block_lists.clone(),
                                },
                                ctx,
                            );
                            view.focus(ctx);
                            view
                        },
                    );
                    ctx.windows().hide_window(id);

                    let mut quake_mode_state = QUAKE_STATE.lock();
                    *quake_mode_state = Some(QuakeModeState {
                        window_state: WindowState::Hidden,
                        window_id: id,
                        active_display_id: frame_args.display_id,
                    });
                } else {
                    normal_window_count += 1;
                    if app_state
                        .active_window_index
                        .map(|window_idx| window_idx == idx)
                        .unwrap_or(false)
                    {
                        active_index = Some(idx);
                    } else {
                        ctx.add_window(
                            AddWindowOptions {
                                window_bounds: WindowBounds::new(window.bounds),
                                title: Some("Warp".to_owned()),
                                fullscreen_state: window.fullscreen_state,
                                background_blur_radius_pixels,
                                background_blur_texture,
                                on_gpu_driver_selected: on_gpu_driver_selected_callback(),
                                ..Default::default()
                            },
                            |ctx| {
                                let mut view = RootView::new(
                                    global_resource_handles.clone(),
                                    NewWorkspaceSource::Restored {
                                        window_snapshot: window.clone(),
                                        block_lists: app_state.block_lists.clone(),
                                    },
                                    ctx,
                                );
                                view.focus(ctx);
                                view
                            },
                        );
                    }
                }
            }

            // If only the quake mode window was restored (which starts hidden), create a new normal
            // window so that something visible is created on startup.
            if normal_window_count == 0 {
                let window_settings = WindowSettings::as_ref(ctx);
                let options = default_window_options(window_settings, ctx);
                ctx.add_window(options, |ctx| {
                    let mut view = RootView::new(
                        global_resource_handles.clone(),
                        NewWorkspaceSource::Empty {
                            previous_active_window: None,
                            shell: None,
                        },
                        ctx,
                    );
                    view.focus(ctx);
                    view
                });
            }

            // Create the active window last to make sure it is focused on startup.
            if let Some(idx) = active_index {
                let window = app_state
                    .windows
                    .get(idx)
                    .expect("Window should exist at idx");
                ctx.add_window(
                    AddWindowOptions {
                        window_bounds: WindowBounds::new(window.bounds),
                        title: Some("Warp".to_owned()),
                        fullscreen_state: window.fullscreen_state,
                        background_blur_radius_pixels,
                        background_blur_texture,
                        on_gpu_driver_selected: on_gpu_driver_selected_callback(),
                        ..Default::default()
                    },
                    |ctx| {
                        let mut view = RootView::new(
                            global_resource_handles,
                            NewWorkspaceSource::Restored {
                                window_snapshot: window.clone(),
                                block_lists: app_state.block_lists.clone(),
                            },
                            ctx,
                        );
                        view.focus(ctx);
                        view
                    },
                );
            }
        }
    }
}

fn path_if_directory(path: &Path) -> Option<&Path> {
    path.is_dir().then_some(path)
}

/// Opens a new window with the workspace configured according to `source`. Returns the
/// newly-opened window ID and a handle to the root view in that window.
///
/// This is the canonical way to open a new Warp window - all other entrypoints should delegate to
/// it if possible.
pub(crate) fn open_new_with_workspace_source(
    source: NewWorkspaceSource,
    ctx: &mut AppContext,
) -> (WindowId, ViewHandle<RootView>) {
    let global_resource_handles = GlobalResourceHandlesProvider::as_ref(ctx).get().clone();
    let window_settings = WindowSettings::as_ref(ctx);
    let options = default_window_options(window_settings, ctx);
    ctx.add_window(options, |ctx| {
        let mut view = RootView::new(global_resource_handles, source, ctx);
        view.focus(ctx);
        view
    })
}

pub(crate) fn open_new_from_path(
    arg: &OpenPath,
    ctx: &mut AppContext,
) -> (WindowId, ViewHandle<RootView>) {
    open_new_with_workspace_source(
        NewWorkspaceSource::Session {
            options: Box::new(
                NewTerminalOptions::default()
                    .with_initial_directory_opt(path_if_directory(&arg.path).map(Into::into)),
            ),
        },
        ctx,
    )
}

/// Opens a new window to view a persisted view-only cloud conversation.
/// The conversation data is loaded via GraphQL API.
fn open_conversation_viewer(conversation_id: &ServerConversationToken, ctx: &mut AppContext) {
    // Trigger the workspace loading mechanism by dispatching the LoadConversationData event
    // This will open a new window with a loading state, fetch data via GraphQL, and display it
    open_new_with_workspace_source(
        NewWorkspaceSource::FromCloudConversationId {
            conversation_id: conversation_id.clone(),
        },
        ctx,
    );
}

/// Opens a new window and starts the guided `/create-environment` setup flow.
fn create_environment(arg: &CreateEnvironmentArg, ctx: &mut AppContext) {
    let repos = arg.repos.clone();
    let (window_id, root_handle) = open_new_with_workspace_source(
        NewWorkspaceSource::Session {
            options: Box::default(),
        },
        ctx,
    );

    root_handle.update(ctx, |root_view, ctx| {
        root_view.workspace.update(ctx, |workspace, ctx| {
            workspace
                .active_tab_pane_group()
                .update(ctx, |pane_group, ctx| {
                    pane_group.set_title("Create Environment", ctx);

                    if let Some(terminal_view) = pane_group.active_session_view(ctx) {
                        terminal_view.update(ctx, |_, ctx| {
                            ctx.dispatch_typed_action_deferred(
                                TerminalAction::SetupCloudEnvironment(repos.clone()),
                            );
                        });
                    }
                });
        });
    });

    ctx.windows().show_window_and_focus_app(window_id);
}

/// Opens a new window and starts the guided `/create-environment` setup flow immediately.
fn create_environment_and_run(arg: &CreateEnvironmentArg, ctx: &mut AppContext) {
    let repos = arg.repos.clone();
    let (window_id, root_handle) = open_new_with_workspace_source(
        NewWorkspaceSource::Session {
            options: Box::default(),
        },
        ctx,
    );

    root_handle.update(ctx, |root_view, ctx| {
        root_view.workspace.update(ctx, |workspace, ctx| {
            workspace
                .active_tab_pane_group()
                .update(ctx, |pane_group, ctx| {
                    pane_group.set_title("Create Environment", ctx);

                    if let Some(terminal_view) = pane_group.active_session_view(ctx) {
                        terminal_view.update(ctx, |_, ctx| {
                            ctx.dispatch_typed_action_deferred(
                                TerminalAction::SetupCloudEnvironmentAndStart(repos.clone()),
                            );
                        });
                    }
                });
        });
    });

    ctx.windows().show_window_and_focus_app(window_id);
}
fn open_team_settings_with_email_invite_in_new_window(
    arg: &OpenTeamsSettingsModalArgs,
    ctx: &mut AppContext,
) {
    let root_handle = open_new_window_get_handles(None, ctx).1;
    root_handle.update(ctx, |root_view, ctx| {
        let initial_load_complete = UpdateManager::as_ref(ctx).initial_load_complete();
        let email_invite = arg.invite_email.clone();
        root_view.workspace.update(ctx, |_, ctx| {
            let _ = ctx.spawn(initial_load_complete, move |workspace, _, ctx| {
                workspace.show_team_settings_page_with_email_invite(email_invite.as_ref(), ctx)
            });
        });
    });
}

fn open_settings_page_in_new_window(section: &SettingsSection, ctx: &mut AppContext) {
    let root_handle = open_new_window_get_handles(None, ctx).1;
    root_handle.update(ctx, |root_view, ctx| {
        let window_id = ctx.window_id();
        ctx.dispatch_typed_action_for_view(
            window_id,
            root_view.workspace.id(),
            &WorkspaceAction::ShowSettingsPage(*section),
        );
    });
}

/// Maps a `warp://settings` deeplink to the workspace action that opens it.
fn workspace_action_for_open_settings(args: &OpenSettingsArgs) -> WorkspaceAction {
    match args {
        OpenSettingsArgs::Default => WorkspaceAction::ShowSettings,
        OpenSettingsArgs::Search { query } => WorkspaceAction::ShowSettingsPageWithSearch {
            search_query: query.clone(),
            section: None,
        },
        OpenSettingsArgs::Widget { page, widget_id } => WorkspaceAction::ScrollToSettingsWidget {
            page: *page,
            widget_id,
        },
    }
}

fn open_settings_in_new_window(args: &OpenSettingsArgs, ctx: &mut AppContext) {
    let action = workspace_action_for_open_settings(args);
    let root_handle = open_new_window_get_handles(None, ctx).1;
    root_handle.update(ctx, |root_view, ctx| {
        let window_id = ctx.window_id();
        ctx.dispatch_typed_action_for_view(window_id, root_view.workspace.id(), &action);
    });
}

/// MCP servers need to wait for initial load to complete, so we have this action in addition
/// to the general-purpose [`open_settings_page_in_new_window`].
fn open_mcp_settings_in_new_window(args: &OpenMCPSettingsArgs, ctx: &mut AppContext) {
    let autoinstall = args.autoinstall.clone();
    let root_handle = open_new_window_get_handles(None, ctx).1;
    root_handle.update(ctx, |root_view, ctx| {
        let initial_load_complete = UpdateManager::as_ref(ctx).initial_load_complete();
        root_view.workspace.update(ctx, |_, ctx| {
            let _ = ctx.spawn(initial_load_complete, move |workspace, _, ctx| {
                workspace.open_mcp_servers_page(
                    MCPServersSettingsPage::List,
                    autoinstall.as_deref(),
                    ctx,
                )
            });
        });
    });
}

/// Opens a new window and shows the Codex modal.
fn open_codex_in_new_window(_: &(), ctx: &mut AppContext) {
    let root_handle = open_new_window_get_handles(None, ctx).1;
    root_handle.update(ctx, |root_view, ctx| {
        let initial_load_complete = UpdateManager::as_ref(ctx).initial_load_complete();
        root_view.workspace.update(ctx, |_, ctx| {
            let _ = ctx.spawn(initial_load_complete, move |workspace, _, ctx| {
                workspace.open_codex_modal(ctx)
            });
        });
    });
}

/// Opens a new window and enters agent view with the Linear issue work prompt.
fn open_linear_issue_work_in_new_window(args: &LinearIssueWork, ctx: &mut AppContext) {
    let (_, root_handle) = open_new_window_get_handles(None, ctx);
    let args = args.clone();
    root_handle.update(ctx, |root_view, ctx| {
        root_view.workspace.update(ctx, |workspace, ctx| {
            workspace.open_linear_issue_work(&args, ctx);
        });
    });
}

fn open_warp_drive_object(arg: &OpenWarpDriveObjectArgs, ctx: &mut AppContext) {
    // See the matching guard in `open_warp_drive_object_in_existing_window`: a `ServerId`-keyed
    // object can never resolve in this build, so don't open a brand-new window that can only ever
    // stay blank.
    if CloudModel::as_ref(ctx)
        .get_by_uid(&arg.server_id.uid())
        .is_none()
    {
        log::info!(
            "Ignoring warp://drive link for {:?} {:?}: object not available locally",
            arg.object_type,
            arg.server_id
        );
        return;
    }

    match arg.object_type {
        ObjectType::Notebook => open_new_workspace_with_notebook_open(
            SyncId::ServerId(arg.server_id),
            arg.settings.clone(),
            ctx,
        ),
        ObjectType::Workflow => open_new_workspace_with_workflow_open(
            SyncId::ServerId(arg.server_id),
            arg.settings.clone(),
            ctx,
        ),
        _ => log::info!("Open object type {:?} not yet supported", arg.object_type),
    }
}

fn display_object_missing_error_in_window(window_id: WindowId, ctx: &mut AppContext) {
    crate::workspace::ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
        let toast = DismissibleToast::error(String::from("Resource not found or access denied"));
        toast_stack.add_ephemeral_toast(toast, window_id, ctx);
    });
}

fn open_new_workspace_with_notebook_open(
    notebook_id: SyncId,
    settings: OpenWarpDriveObjectSettings,
    ctx: &mut AppContext,
) {
    open_new_with_workspace_source(
        NewWorkspaceSource::NotebookById {
            id: notebook_id,
            settings,
        },
        ctx,
    );
}

fn open_new_workspace_with_workflow_open(
    workflow_id: SyncId,
    settings: OpenWarpDriveObjectSettings,
    ctx: &mut AppContext,
) {
    open_new_with_workspace_source(
        NewWorkspaceSource::WorkflowById {
            id: workflow_id,
            settings,
        },
        ctx,
    );
}

/// Opens a new window with a file-based notebook open.
fn open_new_with_file_notebook(arg: &PathBuf, ctx: &mut AppContext) {
    open_new_with_workspace_source(
        NewWorkspaceSource::NotebookFromFilePath {
            file_path: Some(arg.to_owned()),
        },
        ctx,
    );
}

/// Creates a new window and returns its [`WindowId`] and root view's [`ViewHandle`].
pub(crate) fn open_new_window_get_handles(
    shell: Option<AvailableShell>,
    ctx: &mut AppContext,
) -> (WindowId, ViewHandle<RootView>) {
    let active_window_id = ctx.windows().active_window();
    open_new_with_workspace_source(
        NewWorkspaceSource::Empty {
            previous_active_window: active_window_id,
            shell,
        },
        ctx,
    )
}

/// Opens a new window.
fn open_new(_: &(), ctx: &mut AppContext) {
    open_new_window_get_handles(None, ctx);
}

/// Opens a new window with a specific shell
fn open_new_with_shell(shell: &Option<AvailableShell>, ctx: &mut AppContext) {
    open_new_window_get_handles(shell.to_owned(), ctx);
}

/// Global action that performs a few steps:
/// 1. Open a new tab, or open a window if there is none.
/// 2. Set the terminal input buffer to a command that should open a subshell
/// 3. Set a flag that we should automatically bootstrap that subshell if its we can bootstrap its
/// [`ShellType`].
fn open_new_tab_insert_subshell_command_and_bootstrap_if_supported(
    arg: &SubshellCommandArg,
    ctx: &mut AppContext,
) {
    let root_view_handle: Option<ViewHandle<RootView>> = ctx
        .windows()
        .frontmost_window_id()
        .and_then(|window_id| ctx.root_view(window_id));

    let root_view_handle = match root_view_handle {
        Some(root_view_handle) => {
            root_view_handle.update(ctx, |root_view, ctx| {
                root_view.workspace.update(ctx, |workspace, ctx| {
                    workspace.add_terminal_tab(false /* hide_homepage */, ctx);
                });
            });
            root_view_handle
        }
        None => open_new_window_get_handles(None, ctx).1,
    };

    root_view_handle.update(ctx, |root_view, ctx| {
        root_view.insert_subshell_command_and_bootstrap_if_supported(arg, ctx);
    });
}

/// Returns the common configuration for a new "regular" window (not Quake Mode).
fn default_window_options(window_settings: &WindowSettings, ctx: &AppContext) -> AddWindowOptions {
    let (inherited_bounds, window_style) = ctx.next_window_bounds_and_style();
    let next_bounds =
        bounds_for_opening_at_custom_window_size(inherited_bounds, window_settings, ctx);

    AddWindowOptions {
        window_style,
        window_bounds: next_bounds,
        title: Some("Warp".to_owned()),
        background_blur_radius_pixels: Some(*window_settings.background_blur_radius),
        background_blur_texture: *window_settings.background_blur_texture,
        on_gpu_driver_selected: on_gpu_driver_selected_callback(),
        ..Default::default()
    }
}

/// Returns the bounds to open the next window at taking into account whether
/// the user has configured their settings to open windows at a custom size
/// and whether that feature is flagged on.
fn bounds_for_opening_at_custom_window_size(
    bounds: WindowBounds,
    window_settings: &WindowSettings,
    app: &AppContext,
) -> WindowBounds {
    if *window_settings.open_windows_at_custom_size.value() {
        let font_cache = app.font_cache();
        let appearance = Appearance::as_ref(app);

        let cell_size_and_padding = cell_size_and_padding(
            font_cache,
            appearance.monospace_font_family(),
            appearance.monospace_font_size(),
            appearance.ui_builder().line_height_ratio(),
        );
        let window_size = vec2f(
            *window_settings.new_windows_num_columns.value() as f32
                * cell_size_and_padding.cell_width_px.as_f32()
                + 2. * cell_size_and_padding.padding_x_px.as_f32(),
            *window_settings.new_windows_num_rows.value() as f32
                * cell_size_and_padding.cell_height_px.as_f32()
                + 2. * cell_size_and_padding.padding_y_px.as_f32(),
        );

        match bounds {
            WindowBounds::ExactPosition(rect) => {
                WindowBounds::ExactPosition(RectF::new(rect.origin(), window_size))
            }
            WindowBounds::ExactSize(_) | WindowBounds::Default => {
                WindowBounds::ExactSize(window_size)
            }
        }
    } else {
        bounds
    }
}

pub fn quake_mode_window_is_open() -> bool {
    let quake_mode_state = QUAKE_STATE.lock();

    quake_mode_state
        .as_ref()
        .map(|state| {
            matches!(
                state.window_state,
                WindowState::Open | WindowState::PendingOpen
            )
        })
        .unwrap_or_default()
}

pub fn quake_mode_window_id() -> Option<WindowId> {
    let quake_mode_state = QUAKE_STATE.lock();

    quake_mode_state.as_ref().map(|state| state.window_id)
}

pub fn set_quake_mode(new_state: Option<QuakeModeState>) {
    let mut quake_mode_state = QUAKE_STATE.lock();
    *quake_mode_state = new_state;
}

fn move_quake_mode_window_from_screen_change(settings: &QuakeModeSettings, ctx: &mut AppContext) {
    fit_quake_mode_window_within_active_screen(
        settings,
        QuakeModeMoveTrigger::ScreenConfigurationChange,
        ctx,
    )
}

/// If there exists a quake window, mutate its size and position, i.e. its bounds, to match the
/// bounds specified by the [`QuakeModeSettings`].
pub fn update_quake_window_bounds(quake_settings: &QuakeModeSettings, ctx: &mut AppContext) {
    let config = quake_mode_config(quake_settings, ctx);
    let Some(ref state) = *QUAKE_STATE.lock() else {
        return;
    };
    ctx.windows()
        .set_window_bounds(state.window_id, config.window_bounds);
}

/// Move Quake Mode window to the active screen if it is already open or hidden.
fn fit_quake_mode_window_within_active_screen(
    settings: &QuakeModeSettings,
    trigger: QuakeModeMoveTrigger,
    ctx: &mut AppContext,
) {
    let mut quake_mode_state = QUAKE_STATE.lock();

    if let Some(state) = quake_mode_state.as_mut() {
        let active_id = ctx.windows().active_display_id();

        // When there is no screen config and active screen id change, we don't need to reposition
        // the quake mode window as its position should still be valid.
        if matches!(trigger, QuakeModeMoveTrigger::ActiveScreenSetting)
            && active_id == state.active_display_id
        {
            return;
        }

        let window_bound = settings.resolve_quake_mode_bounds(ctx);
        ctx.windows()
            .set_window_bounds(state.window_id, window_bound);
        state.active_display_id = active_id;
    }
}

fn update_quake_mode_state(arg: &UpdateQuakeModeEventArg, ctx: &mut AppContext) {
    if !KeysSettings::as_ref(ctx)
        .quake_mode_settings
        .hide_window_when_unfocused
    {
        return;
    }

    {
        let mut quake_mode_state = QUAKE_STATE.lock();

        if let Some(state) = quake_mode_state.as_mut() {
            state.window_state = match state.window_state {
                WindowState::PendingOpen => WindowState::Open,
                WindowState::Open => {
                    if arg.active_window_id.is_some_and(|id| id == state.window_id) {
                        WindowState::Open
                    } else {
                        ctx.windows().hide_window(state.window_id);
                        WindowState::Hidden
                    }
                }
                WindowState::Hidden => WindowState::Hidden,
            }
        }
    }
}

// Configuration of the next positioning of the quake mode window.
fn quake_mode_config(settings: &QuakeModeSettings, ctx: &mut AppContext) -> QuakeModeFrameConfig {
    QuakeModeFrameConfig {
        display_id: ctx.windows().active_display_id(),
        window_bounds: settings.resolve_quake_mode_bounds(ctx),
    }
}

fn get_quake_mode_state(ctx: &mut AppContext) -> Option<QuakeModeState> {
    let quake_mode_state = QUAKE_STATE.lock();

    match quake_mode_state.as_ref() {
        Some(state) if ctx.is_window_open(state.window_id) => Some(state.clone()),
        _ => None,
    }
}

fn toggle_quake_mode_window(global_resource_handles: &GlobalResourceHandles, ctx: &mut AppContext) {
    // Get the current state of quake mode.
    let state = get_quake_mode_state(ctx);
    match state {
        None => {
            send_telemetry_from_app_ctx!(TelemetryEvent::OpenQuakeModeWindow, ctx);

            let config = quake_mode_config(
                &KeysSettings::as_ref(ctx)
                    .quake_mode_settings
                    .value()
                    .clone(),
                ctx,
            );

            let window_settings = WindowSettings::as_ref(ctx);

            let active_window_id = ctx.windows().active_window();
            let (id, _) = ctx.add_window(
                AddWindowOptions {
                    window_style: WindowStyle::Pin,
                    window_bounds: WindowBounds::ExactPosition(config.window_bounds),
                    title: Some("Warp".to_owned()),
                    background_blur_radius_pixels: Some(*window_settings.background_blur_radius),
                    background_blur_texture: *window_settings.background_blur_texture,
                    // Ignore the quake window for positioning the next window
                    anchor_new_windows_from_closed_position:
                        warpui::NextNewWindowsHasThisWindowsBoundsUponClose::No,
                    on_gpu_driver_selected: on_gpu_driver_selected_callback(),
                    window_instance: Some(ChannelState::app_id().to_string() + "-hotkey"),
                    ..Default::default()
                },
                |ctx| {
                    let mut view = RootView::new(
                        global_resource_handles.clone(),
                        NewWorkspaceSource::Empty {
                            previous_active_window: active_window_id,
                            shell: None,
                        },
                        ctx,
                    );
                    view.focus(ctx);
                    view
                },
            );

            // Update quake mode state after the call to prevent deadlocking.
            let mut quake_mode_state = QUAKE_STATE.lock();
            *quake_mode_state = Some(QuakeModeState {
                window_state: WindowState::PendingOpen,
                window_id: id,
                active_display_id: config.display_id,
            });
        }
        Some(state) if matches!(state.window_state, WindowState::Hidden) => {
            send_telemetry_from_app_ctx!(TelemetryEvent::OpenQuakeModeWindow, ctx);

            // If quake mode does not have a set pin screen -- move it to the current active screen.
            if KeysSettings::as_ref(ctx)
                .quake_mode_settings
                .pin_screen
                .is_none()
            {
                fit_quake_mode_window_within_active_screen(
                    &KeysSettings::as_ref(ctx)
                        .quake_mode_settings
                        .value()
                        .clone(),
                    QuakeModeMoveTrigger::ActiveScreenSetting,
                    ctx,
                );
            }
            ctx.windows().show_window_and_focus_app(state.window_id);

            // Update quake mode state after the call to prevent deadlocking.
            let mut quake_mode_state = QUAKE_STATE.lock();

            if let Some(state) = quake_mode_state.as_mut() {
                state.window_state = WindowState::PendingOpen;
            }
        }
        Some(state) => {
            ctx.windows().hide_window(state.window_id);

            // Update quake mode state after the call to prevent deadlocking.
            let mut quake_mode_state = QUAKE_STATE.lock();

            if let Some(state) = quake_mode_state.as_mut() {
                state.window_state = WindowState::Hidden;
            }
        }
    };
}

/// This action will show or hide all of Warp's windows except the quake window
///
/// - If Warp is active and has any windows, hide those windows.
/// - If Warp is hidden, show all windows.
/// - If Warp is active but has 0 normal windows, create a new window with a new session.
fn show_or_hide_non_quake_mode_windows(_: &(), ctx: &mut AppContext) {
    let quake_window_id = get_quake_mode_state(ctx).map(|state| state.window_id);
    let non_quake_mode_window_ids = ctx
        .window_ids()
        .filter(|window_id| Some(window_id) != quake_window_id.as_ref());
    if non_quake_mode_window_ids.count() == 0 {
        // If there are no normal windows, this action should create one.
        open_new(&(), ctx);
    }
    let windowing_model = ctx.windows();
    // Now there is at least one window. If a Warp window is active, hide the app.
    // Otherwise, show activate the app to show it in front.
    let active_window_id = windowing_model.active_window();
    match active_window_id {
        Some(_) => windowing_model.hide_app(),
        None => {
            windowing_model.activate_app();
        }
    };
}

#[cfg(feature = "voice_input")]
fn abort_voice_input(_: &(), ctx: &mut AppContext) {
    let voice_input = voice_input::VoiceInput::handle(ctx);
    if voice_input.as_ref(ctx).is_listening() {
        voice_input.update(ctx, |voice_input, _| {
            voice_input.abort_listening();
        });
    }
}

#[derive(Clone)]
pub enum NewWorkspaceSource {
    Empty {
        previous_active_window: Option<WindowId>,
        shell: Option<AvailableShell>,
    },
    FromTemplate {
        window_template: launch_config::WindowTemplate,
    },
    Restored {
        window_snapshot: WindowSnapshot,
        block_lists: Arc<HashMap<PaneUuid, Vec<SerializedBlockListItem>>>,
    },
    Session {
        options: Box<NewTerminalOptions>,
    },
    FromCloudConversationId {
        conversation_id: ServerConversationToken,
    },
    NotebookFromFilePath {
        file_path: Option<PathBuf>,
    },
    NotebookById {
        id: SyncId,
        settings: OpenWarpDriveObjectSettings,
    },
    WorkflowById {
        id: SyncId,
        settings: OpenWarpDriveObjectSettings,
    },
    AgentSession {
        options: Box<NewTerminalOptions>,
        initial_query: Option<String>,
    },
    /// Starts the workspace with the Cloud Agent setup tab.
    AmbientAgent,
    /// Opens a new window pre-scoped to a specific team, chosen via the title-bar team switcher.
    TeamSwitched {
        team_uid: ServerId,
    },
    /// A tab is being transferred from another window via the transferable views framework.
    /// The workspace will create a placeholder tab, which will be replaced by the transferred
    /// PaneGroup after window creation.
    TransferredTab {
        source_window_id: WindowId,
        /// Tab color from the source tab
        tab_color: Option<AnsiColorIdentifier>,
        /// Custom title from the source tab
        custom_title: Option<String>,
        /// Whether the left panel was open in the source tab
        left_panel_open: bool,
        /// Captured from the source window so detached tabs inherit the panel state.
        vertical_tabs_panel_open: bool,
        /// Whether the right panel was open in the source tab
        right_panel_open: bool,
        /// Whether the right panel was maximized in the source tab
        is_right_panel_maximized: bool,
        /// Whether this transferred tab window is currently being used as a drag preview.
        is_tab_drag_preview: bool,
    },
}

impl NewWorkspaceSource {
    pub fn has_horizontal_split(&self) -> bool {
        match self {
            NewWorkspaceSource::Restored {
                window_snapshot, ..
            } => {
                if window_snapshot.tabs.is_empty() {
                    false
                } else {
                    let active_index = window_snapshot.active_tab_index;
                    let active_tab = window_snapshot
                        .tabs
                        .get(active_index)
                        .unwrap_or(&window_snapshot.tabs[0]);
                    active_tab.root.has_horizontal_split()
                }
            }
            _ => false,
        }
    }

    pub fn team_uid(&self, ctx: &AppContext) -> Option<ServerId> {
        let source_window_id = match self {
            Self::Empty {
                previous_active_window,
                ..
            } => *previous_active_window,
            Self::TransferredTab {
                source_window_id, ..
            } => Some(*source_window_id),
            Self::FromTemplate { .. }
            | Self::Session { .. }
            | Self::FromCloudConversationId { .. }
            | Self::NotebookFromFilePath { .. }
            | Self::NotebookById { .. }
            | Self::WorkflowById { .. }
            | Self::AgentSession { .. }
            | Self::AmbientAgent => None,
            Self::TeamSwitched { team_uid } => return Some(*team_uid),
            Self::Restored {
                window_snapshot, ..
            } => {
                if let Some(team_uid) = window_snapshot.team_uid {
                    return Some(team_uid);
                }
                None
            }
        };

        UserWorkspaces::as_ref(ctx).inherited_or_default_team_uid(source_window_id)
    }
}

/// Args needed to construct a `Workspace`.
#[derive(Clone)]
struct WorkspaceArgs {
    global_resource_handles: GlobalResourceHandles,
    server_time: Option<Arc<ServerTime>>,
    workspace_setting: NewWorkspaceSource,
}

/// User preferences key to track whether the user has completed the onboarding slides locally
/// (before login). This is needed because the server-side `is_onboarded` flag requires
/// authentication.
const HAS_COMPLETED_ONBOARDING_KEY: &str = "HasCompletedOnboarding";

/// Returns whether the user has completed the onboarding slides locally (before login).
pub(crate) fn has_completed_local_onboarding(ctx: &AppContext) -> bool {
    ctx.private_user_preferences()
        .read_value(HAS_COMPLETED_ONBOARDING_KEY)
        .unwrap_or_default()
        .and_then(|s| serde_json::from_str::<bool>(&s).ok())
        .unwrap_or(false)
}

pub struct RootView {
    workspace: ViewHandle<Workspace>,
    server_time: Option<Arc<ServerTime>>,
    pub server_api: Arc<ServerApi>,
    pub model_event_sender: Option<SyncSender<ModelEvent>>,
}

impl RootView {
    pub fn new(
        global_resource_handles: GlobalResourceHandles,
        workspace_setting: NewWorkspaceSource,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let window_id = ctx.window_id();
        let team_uid = workspace_setting.team_uid(ctx);
        UserWorkspaces::handle(ctx).update(ctx, |user_workspaces, ctx| {
            user_workspaces.register_window(window_id, team_uid, ctx);
        });
        let server_api_provider = ServerApiProvider::as_ref(ctx);
        let server_api = server_api_provider.get();

        ctx.subscribe_to_model(&AuthManager::handle(ctx), |me, _, event, ctx| {
            me.handle_auth_manager_event(event, ctx);
        });

        let model_event_sender = global_resource_handles.model_event_sender.clone();
        let workspace_args = WorkspaceArgs {
            global_resource_handles,
            server_time: None,
            workspace_setting,
        };

        // SimpleWarp is local-only and never shows a login screen: startup always lands
        // directly in the workspace.
        let workspace = workspace_args.create_workspace(ctx);

        let root_view = Self {
            workspace,
            server_time: None,
            server_api: server_api.clone(),
            model_event_sender,
        };

        let autoupdate_handle = AutoupdateState::handle(ctx);
        ctx.subscribe_to_model(&autoupdate_handle, |root_view, _handle, evt, ctx| {
            if let AutoupdateStateEvent::CheckComplete {
                result,
                request_type: RequestType::Poll,
            } = evt
            {
                root_view.polling_update_check_complete(result, ctx)
            }
        });

        // For users who bypass onboarding (already logged in, or onboarding flags not active),
        // start autoupdate polling immediately. For new users in onboarding, this is a no-op;
        // polling will be started once onboarding completes.
        root_view.start_autoupdate_polling(ctx);

        root_view
    }

    fn start_autoupdate_polling(&self, ctx: &mut ViewContext<Self>) {
        AutoupdateState::handle(ctx).update(ctx, |state, ctx| state.start_polling(ctx));
    }

    /// Used for integration tests.
    pub fn workspace_view(&self) -> Option<&ViewHandle<Workspace>> {
        Some(&self.workspace)
    }

    fn polling_update_check_complete(
        &mut self,
        result: &Result<UpdateReady>,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Ok(UpdateReady::Yes { new_version, .. }) = result {
            log::info!("Update ready for channel version {new_version:?}");
            if new_version.update_by.is_some() {
                log::info!("Update ready, there is an update-by time, checking for server time.");
                let server_api = self.server_api.clone();
                let _ = ctx.spawn(
                    async move { server_api.server_time().await },
                    Self::server_time_updated,
                );
            }
        }
    }

    fn server_time_updated(
        &mut self,
        server_time: Result<ServerTime>,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Ok(server_time) = server_time {
            let server_time = Arc::new(server_time);
            self.server_time = Some(server_time.clone());

            self.workspace.update(ctx, |workspace, ctx| {
                workspace.set_server_time(server_time);
                ctx.notify();
            })
        } else {
            report_error!(anyhow::anyhow!(
                "Error fetching server time {:?}",
                server_time.err()
            ));
        }
    }

    /// "Logging out" in a local-only build has no account to sign out of: it just resets the
    /// local workspace to a fresh, empty one. There is no login screen to send the user to.
    fn log_out(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        self.workspace.update(ctx, |workspace, ctx| {
            workspace.on_log_out(ctx);
        });

        let global_resource_handles = GlobalResourceHandlesProvider::as_ref(ctx).get().clone();
        let workspace_setting = NewWorkspaceSource::Empty {
            previous_active_window: None,
            shell: None,
        };
        let workspace_args = WorkspaceArgs {
            global_resource_handles,
            server_time: None,
            workspace_setting,
        };

        // Destroy the old workspace view handle and create a fresh one in its place.
        self.workspace = workspace_args.create_workspace(ctx);

        ctx.focus_self();
        ctx.notify();
        true
    }

    fn close_window(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        if ContextFlag::CloseWindow.is_enabled() {
            ctx.close_window();
        }
        true
    }

    fn toggle_maximize_window(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        ctx.toggle_maximized_window();
        true
    }

    fn toggle_fullscreen(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        let window_id = ctx.window_id();
        WindowManager::handle(ctx).update(ctx, |state, ctx| {
            state.toggle_fullscreen(window_id, ctx);
        });
        true
    }

    fn minimize_window(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        ctx.minimize_window();
        true
    }

    fn focus_pane(
        &mut self,
        pane_view_locator: &PaneViewLocator,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        // Focus the appropriate window.
        let window_id = ctx.window_id();

        let mut quake_mode_state = QUAKE_STATE.lock();
        // If the window we are focusing is the Quake Mode window, then update the QuakeModeState.
        if let Some(mode) = quake_mode_state.as_mut()
            && mode.window_id == window_id
        {
            mode.window_state = WindowState::Open;
        }

        ctx.windows().show_window_and_focus_app(window_id);

        // Focus the appropriate tab/pane.
        self.workspace.update(ctx, |view, ctx| {
            view.focus_pane(*pane_view_locator, ctx);
        });
        true
    }

    fn activate_tab_by_pane_group_id(
        &mut self,
        pane_group_id: &EntityId,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        ctx.windows().show_window_and_focus_app(ctx.window_id());
        self.workspace.update(ctx, |view, ctx| {
            view.activate_tab_by_pane_group_id(*pane_group_id, ctx);
        });
        true
    }

    fn handle_notification_click(
        &mut self,
        pane_view_locator: &PaneViewLocator,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        // Focus the pane that the notification originated from.
        self.focus_pane(pane_view_locator, ctx);
        send_telemetry_from_ctx!(TelemetryEvent::NotificationClicked, ctx);
        true
    }

    #[allow(clippy::ptr_arg)]
    fn handle_incoming_auth_url(&mut self, url: &Url, ctx: &mut ViewContext<Self>) -> bool {
        match AuthRedirectPayload::from_url(url.clone()) {
            Ok(redirect_payload) => {
                AuthManager::handle(ctx).update(ctx, |auth_manager, ctx| {
                    auth_manager.initialize_user_from_auth_payload(redirect_payload, true, ctx);
                });
            }
            Err(error) => {
                safe_error!(
                    safe: ("Unable to parse AuthResult from url"),
                    full: ("Unable to parse AuthResult from url: {error}")
                );
            }
        }
        true
    }

    #[allow(clippy::ptr_arg)]
    fn add_session_at_path(&mut self, path: &PathBuf, ctx: &mut ViewContext<Self>) -> bool {
        let window_id = ctx.window_id();
        self.workspace.update(ctx, |view, ctx| {
            view.add_tab_with_pane_layout(
                PanesLayout::SingleTerminal(Box::new(
                    NewTerminalOptions::default()
                        .with_initial_directory_opt(path_if_directory(path).map(Into::into)),
                )),
                Arc::new(HashMap::new()),
                None,
                ctx,
            );
            ctx.windows().show_window_and_focus_app(window_id);
            ctx.notify();
        });
        true
    }

    pub fn open_team_settings_with_email_invite_in_existing_window(
        &mut self,
        arg: &OpenTeamsSettingsModalArgs,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        self.workspace.update(ctx, |workspace, ctx| {
            workspace.show_team_settings_page_with_email_invite(arg.invite_email.as_ref(), ctx)
        });
        true
    }

    pub fn open_warp_drive_object_in_existing_window(
        &mut self,
        arg: &OpenWarpDriveObjectArgs,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let cloud_model = CloudModel::as_ref(ctx);

        // No warp-server connection exists in this build (see `LOCAL_ONLY_MESSAGE` in
        // `server/server_api.rs`), so an object identified by `ServerId` can never be synced
        // into `CloudModel` here — every `warp://drive/...` link is a dead end. Fail fast and
        // legibly for every object type instead of routing Notebook/Workflow into a pane that
        // silently never loads (Folder/EnvVarCollection already guarded against this below;
        // this hoists the same guard above the match so it applies uniformly).
        if cloud_model.get_by_uid(&arg.server_id.uid()).is_none() {
            display_object_missing_error_in_window(ctx.window_id(), ctx);
            return false;
        }

        // Every arm below used to route into the Warp Drive left-panel tab, which no longer
        // exists (see `simplify-specs/plan.md`, Phase 4 Track A). The guard above already
        // makes this function unreachable in practice, since a `ServerId`-keyed object can
        // never resolve with no warp-server connection — so there is nothing left to route to
        // for any object type.
        log::info!(
            "Object type {:?} not supported for opening via link",
            arg.object_type
        );

        let window_id = ctx.window_id();
        ctx.windows().show_window_and_focus_app(window_id);
        ctx.notify();
        true
    }

    /// Opens a cloud conversation in an existing window.
    /// If the user owns the conversation, restores or navigates to it directly.
    /// Otherwise, opens a read-only transcript viewer.
    pub fn open_cloud_conversation_in_existing_window(
        &mut self,
        conversation_id: &ServerConversationToken,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        self.workspace.update(ctx, |workspace, ctx| {
            workspace.open_cloud_conversation_from_server_token(conversation_id.clone(), ctx);
        });
        let window_id = ctx.window_id();
        ctx.windows().show_window_and_focus_app(window_id);
        ctx.notify();
        true
    }

    /// Adds a tab and starts the guided `/create-environment` setup flow.
    fn create_environment_in_existing_window(
        &mut self,
        arg: &CreateEnvironmentArg,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let repos = arg.repos.clone();

        self.workspace.update(ctx, |workspace, ctx| {
            workspace.add_tab_with_pane_layout(
                PanesLayout::SingleTerminal(Box::default()),
                Arc::new(HashMap::new()),
                None,
                ctx,
            );

            workspace
                .active_tab_pane_group()
                .update(ctx, |pane_group, ctx| {
                    pane_group.set_title("Create Environment", ctx);

                    if let Some(terminal_view) = pane_group.active_session_view(ctx) {
                        terminal_view.update(ctx, |_, ctx| {
                            ctx.dispatch_typed_action_deferred(
                                TerminalAction::SetupCloudEnvironment(repos.clone()),
                            );
                        });
                    }
                });
        });
        let window_id = ctx.window_id();
        ctx.windows().show_window_and_focus_app(window_id);
        ctx.notify();
        true
    }

    /// Adds a tab and starts the guided `/create-environment` setup flow immediately.
    fn create_environment_in_existing_window_and_run(
        &mut self,
        arg: &CreateEnvironmentArg,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let repos = arg.repos.clone();

        self.workspace.update(ctx, |workspace, ctx| {
            workspace.add_tab_with_pane_layout(
                PanesLayout::SingleTerminal(Box::default()),
                Arc::new(HashMap::new()),
                None,
                ctx,
            );

            workspace
                .active_tab_pane_group()
                .update(ctx, |pane_group, ctx| {
                    pane_group.set_title("Create Environment", ctx);

                    if let Some(terminal_view) = pane_group.active_session_view(ctx) {
                        terminal_view.update(ctx, |_, ctx| {
                            ctx.dispatch_typed_action_deferred(
                                crate::terminal::view::TerminalAction::SetupCloudEnvironmentAndStart(
                                    repos.clone(),
                                ),
                            );
                        });
                    }
                });
        });

        let window_id = ctx.window_id();
        ctx.windows().show_window_and_focus_app(window_id);
        ctx.notify();
        true
    }

    pub fn add_file_pane(&mut self, path: &PathBuf, ctx: &mut ViewContext<Self>) -> bool {
        self.workspace.update(ctx, |workspace, ctx| {
            workspace.add_tab_for_file_notebook(Some(path.to_owned()), ctx);
        });
        let window_id = ctx.window_id();
        ctx.windows().show_window_and_focus_app(window_id);
        ctx.notify();
        true
    }

    /// Insert a command that should create a subshell. If we support bootstrapping AKA
    /// "warpifying" its [`ShellType`], set a flag to automatically bootstrap it when the command's
    /// block receives the [`AfterBlockStarted`] event.
    pub fn insert_subshell_command_and_bootstrap_if_supported(
        &mut self,
        arg: &SubshellCommandArg,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let window_id = ctx.window_id();
        self.workspace.update(ctx, |workspace, ctx| {
            workspace.insert_subshell_command_and_bootstrap_if_supported(
                &arg.command,
                arg.shell_type,
                ctx,
            );
            ctx.windows().show_window_and_focus_app(window_id);
        });
        true
    }

    /// Shows the user the settings view of their newly joined team
    /// within the app.
    pub fn handle_team_intent_link_action(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        // Force-open warp drive.
        let window_id = ctx.window_id();
        ctx.dispatch_typed_action_for_view(
            window_id,
            self.workspace.id(),
            &WorkspaceAction::OpenWarpDrive,
        );
        ctx.windows().show_window_and_focus_app(window_id);

        // Use the team tester model to notify relevant subscribers to refresh their data.
        TeamTesterStatus::handle(ctx).update(ctx, |model, ctx| {
            model.initiate_data_pollers(true, ctx);
        });
        true
    }

    pub fn open_team_settings_page(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        let window_id = ctx.window_id();
        ctx.dispatch_typed_action_for_view(
            window_id,
            self.workspace.id(),
            &WorkspaceAction::ShowSettingsPage(SettingsSection::Teams),
        );
        ctx.windows().show_window_and_focus_app(window_id);
        true
    }

    pub fn open_settings_page_in_existing_window(
        &mut self,
        section: &SettingsSection,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let window_id = ctx.window_id();
        ctx.dispatch_typed_action_for_view(
            window_id,
            self.workspace.id(),
            &WorkspaceAction::ShowSettingsPage(*section),
        );
        ctx.windows().show_window_and_focus_app(window_id);
        true
    }

    pub fn open_settings_in_existing_window(
        &mut self,
        args: &OpenSettingsArgs,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let window_id = ctx.window_id();
        let action = workspace_action_for_open_settings(args);
        ctx.dispatch_typed_action_for_view(window_id, self.workspace.id(), &action);
        ctx.windows().show_window_and_focus_app(window_id);
        true
    }

    /// Opens the MCP servers settings page in an existing window, optionally triggering auto-install.
    /// Waits for `initial_load_complete` before opening so gallery data is available for autoinstall.
    pub fn open_mcp_settings_in_existing_window(
        &mut self,
        args: &OpenMCPSettingsArgs,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let autoinstall = args.autoinstall.clone();
        let initial_load_complete = UpdateManager::as_ref(ctx).initial_load_complete();
        self.workspace.update(ctx, |_, ctx| {
            let _ = ctx.spawn(initial_load_complete, move |workspace, _, ctx| {
                workspace.open_mcp_servers_page(
                    MCPServersSettingsPage::List,
                    autoinstall.as_deref(),
                    ctx,
                )
            });
        });
        let window_id = ctx.window_id();
        ctx.windows().show_window_and_focus_app(window_id);
        true
    }

    /// Opens the Codex modal in an existing window.
    pub fn open_codex_in_existing_window(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        let window_id = ctx.window_id();
        self.workspace.update(ctx, |workspace, ctx| {
            workspace.open_codex_modal(ctx);
        });
        ctx.windows().show_window_and_focus_app(window_id);
        true
    }

    /// Opens a new tab with agent view for a Linear issue work deeplink.
    pub fn open_linear_issue_work_in_existing_window(
        &mut self,
        args: &LinearIssueWork,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let window_id = ctx.window_id();
        let args = args.clone();
        self.workspace.update(ctx, |workspace, ctx| {
            workspace.open_linear_issue_work(&args, ctx);
        });
        ctx.windows().show_window_and_focus_app(window_id);
        true
    }

    /// Syncs the local "onboarding completed" flag to the server if the user
    /// finished onboarding pre-login and has since authenticated. Runs on every
    /// `AuthComplete`, so it also covers users who skipped login during onboarding
    /// and later signed up through a different entrypoint (e.g. login modal,
    /// settings, command palette) while already in the `Terminal` state.
    fn sync_local_onboarding_to_server(auth_state: &AuthState, ctx: &mut AppContext) {
        let is_onboarded = auth_state.is_onboarded().unwrap_or(true);
        let is_anonymous = auth_state.is_user_anonymous().unwrap_or(false);
        let has_completed_local_onboarding = has_completed_local_onboarding(ctx);

        if has_completed_local_onboarding && !is_onboarded && !is_anonymous {
            AuthManager::handle(ctx).update(ctx, |model, ctx| model.set_user_onboarded(ctx));
        }
    }

    fn handle_auth_manager_event(&mut self, event: &AuthManagerEvent, ctx: &mut ViewContext<Self>) {
        let auth_state = AuthStateProvider::as_ref(ctx).get().clone();

        match event {
            AuthManagerEvent::AuthComplete => {
                Self::sync_local_onboarding_to_server(&auth_state, ctx);
                self.start_autoupdate_polling(ctx);
                self.focus(ctx);
            }
            AuthManagerEvent::AuthFailed(err) => match err {
                UserAuthenticationError::DeniedAccessToken(_) => {
                    // We show a banner in the app nudging them to reconnect, but don't
                    // actually log them out. That is handled in the workspace view.
                }
                UserAuthenticationError::UserAccountDisabled(_) => {
                    // Force sign them out, as they should not be able to continue to use Warp.
                    // Instead, they can sign in or up with a valid account.
                    crate::auth::log_out(ctx);
                }
                UserAuthenticationError::Unexpected(_) => {
                    report_error!(err);
                }
                UserAuthenticationError::DeviceCodeRequestTimedOut { .. } => {}
                UserAuthenticationError::InvalidStateParameter => {}
                UserAuthenticationError::MissingStateParameter => {}
            },
            _ => {}
        }
    }

    pub fn focus(&mut self, ctx: &mut ViewContext<Self>) -> bool {
        ctx.focus(&self.workspace);
        ctx.notify();
        true
    }

    /// Stops active voice input, if the configured voice input toggle key is released.
    #[cfg(feature = "voice_input")]
    fn maybe_stop_active_voice_input(
        &mut self,
        key_code: &warpui::platform::keyboard::KeyCode,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        use voice_input::{VoiceInput, VoiceInputState, VoiceInputToggledFrom};
        use warpui::event::KeyState;

        use crate::settings::AISettings;

        // Check that the released key matches the configured voice input toggle key.
        let ai_settings = AISettings::as_ref(ctx);
        if let Some(configured_key_code) = ai_settings.voice_input_toggle_key.value().to_key_code()
            && configured_key_code == *key_code
        {
            let voice_input = VoiceInput::handle(ctx);
            // Check if we're actively listening and it was started from a key press.
            if let VoiceInputState::Listening { enabled_from, .. } = voice_input.as_ref(ctx).state()
                && matches!(
                    enabled_from,
                    VoiceInputToggledFrom::Key {
                        state: KeyState::Pressed
                    }
                )
            {
                log::debug!("Voice input key release detected: {key_code:?}");
                // Stop listening and proceed to transcription (don't abort).
                voice_input.update(ctx, |voice_input, ctx| {
                    if let Err(e) = voice_input.stop_listening(ctx) {
                        report_error!(e.context("Failed to stop voice input on key release"));
                    }
                });
            }
        }
        true
    }
}

impl Entity for RootView {
    type Event = ();
}

impl View for RootView {
    fn ui_name() -> &'static str {
        "RootView"
    }

    fn on_focus(&mut self, focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {
        if focus_ctx.is_self_focused() {
            self.focus(ctx);
        }
    }

    fn render(&self, _app: &AppContext) -> Box<dyn Element> {
        let child = ChildView::new(&self.workspace).finish();

        let mut stack = Stack::new();
        stack.add_child(child);

        cfg_if::cfg_if! {
            if #[cfg(feature = "voice_input")] {
                use warpui::elements::{EventHandler, DispatchEventResult};
                EventHandler::new(stack.finish())
                    .on_modifier_state_changed(|ctx, _app, key_code, key_state| {
                        if matches!(key_state, warpui::event::KeyState::Released) {
                            ctx.dispatch_action("root_view:maybe_stop_active_voice_input", *key_code);
                        }
                        DispatchEventResult::PropagateToParent
                    })
                    .finish()
            } else {
                stack.finish()
            }
        }
    }

    fn keymap_context(&self, app: &AppContext) -> warpui::keymap::Context {
        let mut context = Self::default_keymap_context();
        if quake_mode_window_is_open() {
            context.set.insert(flags::QUAKE_WINDOW_OPEN_FLAG);
        }
        if *KeysSettings::as_ref(app).quake_mode_enabled {
            context.set.insert(flags::QUAKE_MODE_ENABLED_CONTEXT_FLAG);
        }
        if *KeysSettings::as_ref(app).activation_hotkey_enabled.value() {
            context.set.insert(flags::ACTIVATION_HOTKEY_FLAG);
        }
        context
    }
}

#[derive(Clone, Debug)]
pub enum RootViewAction {
    ToggleQuakeModeWindow,
    ShowOrHideNonQuakeModeWindows,
    ToggleFullscreen,
}

impl TypedActionView for RootView {
    type Action = RootViewAction;
    fn handle_action(&mut self, action: &RootViewAction, ctx: &mut ViewContext<Self>) {
        match action {
            RootViewAction::ToggleQuakeModeWindow => {
                let global_resource_handles =
                    GlobalResourceHandlesProvider::as_ref(ctx).get().clone();
                toggle_quake_mode_window(&global_resource_handles, ctx)
            }
            RootViewAction::ShowOrHideNonQuakeModeWindows => {
                show_or_hide_non_quake_mode_windows(&(), ctx)
            }
            RootViewAction::ToggleFullscreen => {
                let window_id = ctx.window_id();
                WindowManager::handle(ctx).update(ctx, |state, ctx| {
                    state.toggle_fullscreen(window_id, ctx);
                });
            }
        }
    }
}

impl WorkspaceArgs {
    fn create_workspace(self, ctx: &mut ViewContext<RootView>) -> ViewHandle<Workspace> {
        ctx.add_typed_action_view(|ctx| {
            Workspace::new(
                self.global_resource_handles,
                self.server_time,
                self.workspace_setting,
                ctx,
            )
        })
    }
}

#[cfg(test)]
#[path = "root_view_tests.rs"]
mod tests;
