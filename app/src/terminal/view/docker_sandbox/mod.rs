use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::SyncSender;

use warp_errors::report_error;
use warpui::geometry::vector::Vector2F;
use warpui::{ModelHandle, ViewContext, ViewHandle};

use super::TerminalView;
use crate::banner::BannerState;
use crate::pane_group::TerminalViewResources;
use crate::persistence::ModelEvent;
use crate::terminal::TerminalManager;
use crate::terminal::available_shells::AvailableShell;
use crate::terminal::local_tty::docker_sandbox::resolve_sbx_path_from_user_shell;
use crate::terminal::local_tty::{
    TerminalManager as LocalTtyTerminalManager, TerminalViewSurfaceConfig,
    create_terminal_view_surface,
};

/// Default base Docker image used for newly created sandbox shells.
///
/// `None` means "let sbx pick its own default template".
pub(crate) const DEFAULT_DOCKER_SANDBOX_BASE_IMAGE: Option<&str> = None;

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
    }

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
