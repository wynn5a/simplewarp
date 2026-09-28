#[cfg(feature = "local_tty")]
use std::collections::HashMap;
#[cfg(feature = "local_tty")]
use std::path::PathBuf;
#[cfg(feature = "local_tty")]
use std::sync::mpsc::SyncSender;

#[cfg(feature = "local_tty")]
use warp_errors::report_error;
use warpui::ViewContext;
#[cfg(feature = "local_tty")]
use warpui::geometry::vector::Vector2F;
#[cfg(feature = "local_tty")]
use warpui::{ModelHandle, ViewHandle};

use super::TerminalView;
#[cfg(feature = "local_tty")]
use crate::banner::BannerState;
#[cfg(feature = "local_tty")]
use crate::pane_group::TerminalViewResources;
#[cfg(feature = "local_tty")]
use crate::persistence::ModelEvent;
#[cfg(feature = "local_tty")]
use crate::terminal::TerminalManager;
#[cfg(feature = "local_tty")]
use crate::terminal::available_shells::AvailableShell;
#[cfg(feature = "local_tty")]
use crate::terminal::local_tty::docker_sandbox::resolve_sbx_path_from_user_shell;
#[cfg(feature = "local_tty")]
use crate::terminal::local_tty::{
    TerminalManager as LocalTtyTerminalManager, TerminalViewSurfaceConfig,
    create_terminal_view_surface,
};

/// Default base Docker image used for newly created sandbox shells.
///
/// `None` means "let sbx pick its own default template".
#[cfg(feature = "local_tty")]
pub(crate) const DEFAULT_DOCKER_SANDBOX_BASE_IMAGE: Option<&str> = None;

#[cfg(feature = "local_tty")]
#[allow(unused_variables)]
fn create_docker_sandbox_view(
    resources: TerminalViewResources,
    initial_size: Vector2F,
    model_event_sender: Option<SyncSender<ModelEvent>>,
    #[allow(dead_code)] sbx_path: PathBuf,
    ctx: &mut ViewContext<TerminalView>,
) -> (
    ViewHandle<TerminalView>,
    ModelHandle<Box<dyn TerminalManager>>,
) {
    cfg_if::cfg_if! {
        if #[cfg(feature = "local_tty")] {
            let user_default_shell_unsupported_banner_model_handle =
                ctx.add_model(|_| BannerState::default());

            let chosen_shell = Some(AvailableShell::new_docker_sandbox_shell(
                sbx_path,
                DEFAULT_DOCKER_SANDBOX_BASE_IMAGE.map(str::to_owned),
            ));

            let model_event_sender_for_surface = model_event_sender.clone();
            let window_id = ctx.window_id();
            let terminal_init = LocalTtyTerminalManager::<TerminalView>::create_model(
                None,
                HashMap::new(),
                None, /* restored_blocks */
                user_default_shell_unsupported_banner_model_handle,
                initial_size,
                model_event_sender,
                chosen_shell,
                ctx,
                |surface_init, ctx| {
                    create_terminal_view_surface(
                        TerminalViewSurfaceConfig {
                            resources,
                            model_event_sender: model_event_sender_for_surface,
                            window_id,
                            initial_input_config: None,
                            conversation_restoration: None,
                            has_conversation_restoration: false,
                            is_historical: false,
                            should_use_live_appearance: false,
                            has_restored_command_blocks: false,
                        },
                        surface_init,
                        ctx,
                    )
                },
            );
            let terminal_manager = terminal_init.manager;
            let terminal_view = terminal_init.surface;
        } else {
            log::info!("USING MOCK TERMINAL MANAGER!!!!!");
            use crate::terminal::MockTerminalManager;
            use crate::terminal::shell::{ShellName, ShellType};
            use crate::terminal::ShellLaunchState;
            let terminal_init = MockTerminalManager::create_model(
                ShellLaunchState::ShellSpawned {
                    available_shell: None,
                    display_name: ShellName::blank(),
                    shell_type: ShellType::Bash,
                },
                resources,
                None, /* restored_blocks */
                None, /* conversation_restoration */
                initial_size,
                ctx.window_id(),
                ctx,
            );
            let terminal_manager = terminal_init.manager;
            let terminal_view = terminal_init.view;
        }
    }

    (terminal_view, terminal_manager)
}

impl TerminalView {
    pub(crate) fn create_and_push_docker_sandbox(&self, ctx: &mut ViewContext<Self>) {
        // Resolve sbx via the user's interactive shell PATH (same mechanism
        // MCP servers use) before creating the pane. This is async, so we
        // spawn and then build the pane in the completion callback.
        //
        // The sbx resolution and sandbox creation are only meaningful on
        // platforms with a local tty; other builds log and bail.
        #[cfg(feature = "local_tty")]
        {
            let sbx_future = resolve_sbx_path_from_user_shell(ctx);
            ctx.spawn(sbx_future, move |me, sbx_path, ctx| {
                let Some(sbx_path) = sbx_path else {
                    report_error!("sbx binary not found; cannot create Docker sandbox");
                    return;
                };
                me.create_and_push_docker_sandbox_with_sbx(sbx_path, ctx);
            });
        }
        #[cfg(not(feature = "local_tty"))]
        {
            let _ = ctx;
            log::warn!("Docker sandbox requires the `local_tty` feature; ignoring request");
        }
    }

    #[cfg(feature = "local_tty")]
    fn create_and_push_docker_sandbox_with_sbx(
        &self,
        sbx_path: PathBuf,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(pane_stack) = self
            .pane_stack
            .as_ref()
            .and_then(|stack| stack.upgrade(ctx))
        else {
            log::warn!("Pane stack not available, cannot create docker sandbox session");
            return;
        };

        let resources = TerminalViewResources {
            tips_completed: self.tips_completed.clone(),
            model_event_sender: self.model_event_sender.clone(),
        };
        let pane_configuration = self.pane_configuration().clone();

        let (terminal_view, terminal_manager) = create_docker_sandbox_view(
            resources,
            self.size_info().pane_size_px(),
            self.model_event_sender.clone(),
            sbx_path,
            ctx,
        );

        terminal_view.update(ctx, |view, _| {
            view.set_pane_configuration(pane_configuration);
        });

        pane_stack.update(ctx, |stack, ctx| {
            stack.push(terminal_manager, terminal_view, ctx);
        });

        ctx.notify();
    }
}
