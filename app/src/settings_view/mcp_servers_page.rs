use std::collections::HashMap;

use uuid::Uuid;
use warp_errors::report_error;
use warpui::elements::{ChildView, Container};
use warpui::ui_components::components::{Coords, UiComponentStyles};
use warpui::{
    AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use crate::ai::mcp::templatable_installation::VariableValue;
use crate::ai::mcp::{
    FileBasedMCPManager, TemplatableMCPServer, TemplatableMCPServerInstallation,
    TemplatableMCPServerManager,
};
use crate::appearance::Appearance;
use crate::modal::{Modal, ModalViewState};
use crate::settings_view::SettingsSection;
use crate::settings_view::mcp_servers::edit_page::{
    MCPServersEditPageView, MCPServersEditPageViewEvent,
};
use crate::settings_view::mcp_servers::installation_modal::{
    InstallationModalBody, InstallationModalBodyEvent,
};
use crate::settings_view::mcp_servers::list_page::{
    MCPServersListPageView, MCPServersListPageViewEvent,
};
use crate::settings_view::mcp_servers::{ServerCardItemId, style};
use crate::settings_view::settings_page::{MatchData, PageType, SettingsPageMeta, SettingsWidget};
use crate::view_components::DismissibleToast;
use crate::workspace::ToastStack;

const PAGE_TITLE_TEXT: &str = "MCP Servers";
#[derive(Debug, Default, Copy, Clone)]
pub enum MCPServersSettingsPage {
    #[default]
    List,
    Edit {
        item_id: Option<ServerCardItemId>,
    },
}

#[derive(Debug, Clone)]
pub enum MCPServersSettingsPageEvent {
    ShowModal,
    HideModal,
}

pub struct MCPServersSettingsPageView {
    page: PageType<Self>,
    current_page: MCPServersSettingsPage,
    list_view: ViewHandle<MCPServersListPageView>,
    edit_view: ViewHandle<MCPServersEditPageView>,
    installation_modal_state: ModalViewState<Modal<InstallationModalBody>>,
}

impl MCPServersSettingsPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let list_view = ctx.add_typed_action_view(MCPServersListPageView::new);
        ctx.subscribe_to_view(&list_view, |me, _, event, ctx| {
            me.handle_list_view_event(event, ctx);
        });

        let edit_view = ctx.add_typed_action_view(MCPServersEditPageView::new);
        ctx.subscribe_to_view(&edit_view, |me, _, event, ctx| {
            me.handle_edit_view_event(event, ctx);
        });

        let installation_modal_body = ctx.add_typed_action_view(InstallationModalBody::new);
        ctx.subscribe_to_view(&installation_modal_body, |me, _, event, ctx| {
            me.handle_installation_modal_body_event(event, ctx);
        });

        let installation_modal = ctx.add_typed_action_view(|ctx| {
            Modal::new(None, installation_modal_body, ctx).with_body_style(UiComponentStyles {
                padding: Some(Coords::uniform(0.)),
                ..Default::default()
            })
        });
        let installation_modal_state = ModalViewState::new(installation_modal);

        Self {
            page: PageType::new_monolith(
                MCPServersSettingsWidget::default(),
                Some(PAGE_TITLE_TEXT),
                true,
            ),
            current_page: MCPServersSettingsPage::default(),
            list_view,
            edit_view,
            installation_modal_state,
        }
    }

    pub fn update_page(&mut self, page: MCPServersSettingsPage, ctx: &mut ViewContext<Self>) {
        self.current_page = page;
        if let MCPServersSettingsPage::Edit { item_id } = page {
            self.edit_view.update(ctx, |edit_view, ctx| {
                edit_view.set_mcp_server(item_id, ctx);
            });
        }
        self.focus(ctx);
        ctx.notify();
    }

    pub fn focus(&mut self, ctx: &mut ViewContext<Self>) {
        match self.current_page {
            MCPServersSettingsPage::List => ctx.focus(&self.list_view),
            MCPServersSettingsPage::Edit { .. } => ctx.focus(&self.edit_view),
        }
    }

    fn add_toast(&mut self, message: &str, ctx: &mut ViewContext<Self>) {
        let window_id = ctx.window_id();
        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
            toast_stack.add_ephemeral_toast(
                DismissibleToast::default(message.to_string()),
                window_id,
                ctx,
            );
        });
    }

    fn handle_log_out(
        &mut self,
        item_id: ServerCardItemId,
        server_name: Option<String>,
        ctx: &mut ViewContext<Self>,
    ) {
        let message = match server_name {
            Some(name) => format!("Successfully logged out of {name} MCP server"),
            None => "Successfully logged out of MCP server".to_string(),
        };
        match item_id {
            ServerCardItemId::TemplatableMCP(_) => {
                report_error!("Logging out is not supported for template MCP servers.");
            }
            ServerCardItemId::TemplatableMCPInstallation(uuid) => {
                TemplatableMCPServerManager::handle(ctx).update(ctx, |manager, ctx| {
                    manager.delete_credentials_from_secure_storage(uuid, ctx);
                    manager.shutdown_server(uuid, ctx);
                });
                self.add_toast(&message, ctx);
            }
            ServerCardItemId::FileBasedMCP(uuid) => {
                if let Some(installation) =
                    FileBasedMCPManager::as_ref(ctx).get_installation_by_uuid(uuid)
                    && let Some(hash) = installation.hash()
                {
                    TemplatableMCPServerManager::handle(ctx).update(ctx, |manager, ctx| {
                        manager.shutdown_server(uuid, ctx);
                        manager.purge_file_based_server_credentials(&vec![hash], ctx);
                    });
                }
                self.add_toast(&message, ctx);
            }
        }
    }

    fn start_server_installation(
        &mut self,
        templatable_mcp_server: TemplatableMCPServer,
        instructions_in_markdown: Option<String>,
        ctx: &mut ViewContext<Self>,
    ) {
        // The modal is only needed when the template has variables to fill in or instructions to show.
        let has_variables = !templatable_mcp_server.template.variables.is_empty();
        let has_instructions = instructions_in_markdown.is_some();
        let should_show_modal = has_variables || has_instructions;

        if should_show_modal {
            self.installation_modal_state
                .view
                .update(ctx, |modal, ctx| {
                    modal.body().update(ctx, |body, ctx| {
                        body.set_templatable_mcp_server(
                            Some(templatable_mcp_server),
                            instructions_in_markdown,
                            ctx,
                        )
                    });
                });
            self.installation_modal_state.open();
            ctx.focus(&self.installation_modal_state.view);
            ctx.emit(MCPServersSettingsPageEvent::ShowModal);
        } else {
            self.process_server_installation(&templatable_mcp_server, HashMap::new(), ctx);
        }
        ctx.notify();
    }

    fn process_server_installation(
        &mut self,
        templatable_mcp_server: &TemplatableMCPServer,
        variable_values: HashMap<String, VariableValue>,
        ctx: &mut ViewContext<Self>,
    ) -> Option<TemplatableMCPServerInstallation> {
        TemplatableMCPServerManager::handle(ctx).update(ctx, |templatable_manager, ctx| {
            if templatable_manager
                .get_cloud_server(templatable_mcp_server.uuid, ctx)
                .is_none()
            {
                templatable_manager
                    .create_templatable_mcp_server(templatable_mcp_server.clone(), ctx);
            }

            let installation = templatable_manager.install_from_template(
                templatable_mcp_server.clone(),
                variable_values.clone(),
                true,
                ctx,
            );
            ctx.notify();
            installation
        })
    }

    pub fn reinstall_server(&mut self, installation_uuid: Uuid, ctx: &mut ViewContext<Self>) {
        let template_uuid =
            TemplatableMCPServerManager::as_ref(ctx).get_template_uuid(installation_uuid);
        if let Some(template_uuid) = template_uuid {
            let templatable_mcp_server =
                TemplatableMCPServerManager::as_ref(ctx).get_templatable_mcp_server(template_uuid);

            if let Some(templatable_mcp_server) = templatable_mcp_server {
                self.start_server_installation(templatable_mcp_server.clone(), None, ctx);
            }
        }
    }

    fn handle_list_view_event(
        &mut self,
        event: &MCPServersListPageViewEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            MCPServersListPageViewEvent::Edit(mcp_item_id) => {
                self.update_page(
                    MCPServersSettingsPage::Edit {
                        item_id: Some(*mcp_item_id),
                    },
                    ctx,
                );
            }
            MCPServersListPageViewEvent::Add => {
                self.update_page(MCPServersSettingsPage::Edit { item_id: None }, ctx);
            }
            MCPServersListPageViewEvent::LogOut(server_card_item_id, server_name) => {
                self.handle_log_out(*server_card_item_id, Some(server_name.clone()), ctx);
            }
            MCPServersListPageViewEvent::StartInstallation {
                templatable_mcp_server: template,
                instructions_in_markdown,
            } => {
                self.start_server_installation(
                    template.clone(),
                    instructions_in_markdown.clone(),
                    ctx,
                );
            }
            MCPServersListPageViewEvent::ShowModal => {
                ctx.emit(MCPServersSettingsPageEvent::ShowModal);
            }
            MCPServersListPageViewEvent::HideModal => {
                ctx.emit(MCPServersSettingsPageEvent::HideModal);
            }
        }
    }

    fn handle_edit_view_event(
        &mut self,
        event: &MCPServersEditPageViewEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            MCPServersEditPageViewEvent::Back => {
                self.update_page(MCPServersSettingsPage::List, ctx);
            }
            MCPServersEditPageViewEvent::Reinstall(template_uuid) => {
                self.reinstall_server(*template_uuid, ctx);
            }
            MCPServersEditPageViewEvent::Delete(item_id) => {
                self.list_view.update(ctx, |list_view, ctx| {
                    list_view.delete_server(*item_id, ctx);
                });
                self.update_page(MCPServersSettingsPage::List, ctx);
            }
            MCPServersEditPageViewEvent::LogOut(server_card_item_id, server_name) => {
                self.handle_log_out(*server_card_item_id, server_name.clone(), ctx);
            }
        }
    }

    pub fn get_modal_content(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        if self.installation_modal_state.is_open() {
            Some(self.installation_modal_state.render())
        } else {
            match self.current_page {
                MCPServersSettingsPage::List => self
                    .list_view
                    .read(app, |list_view, _| list_view.get_modal_content()),
                MCPServersSettingsPage::Edit { .. } => None,
            }
        }
    }

    fn handle_installation_modal_body_event(
        &mut self,
        event: &InstallationModalBodyEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            InstallationModalBodyEvent::Install(templatable_mcp_server, variable_values) => {
                // Uninstall the old copy with outdated variable values
                TemplatableMCPServerManager::handle(ctx).update(ctx, |manager, ctx| {
                    let old_installation =
                        manager.get_installation_by_template_uuid(templatable_mcp_server.uuid);
                    if let Some(old_installation) = old_installation {
                        let old_installation_uuid = old_installation.uuid();
                        manager
                            .delete_templatable_mcp_server_installation(old_installation_uuid, ctx);
                        ctx.notify();
                    };
                });

                // Install the copy with new variables
                let new_installation = self.process_server_installation(
                    templatable_mcp_server,
                    variable_values.clone(),
                    ctx,
                );

                // When we re-install, the installation uuid changes, so we should load the edit page with the new installation uuid
                if let Some(new_installation) = new_installation {
                    self.edit_view.update(ctx, |edit_page, ctx| {
                        edit_page.set_mcp_server(
                            Some(ServerCardItemId::TemplatableMCPInstallation(
                                new_installation.uuid(),
                            )),
                            ctx,
                        );
                    });
                }

                self.installation_modal_state
                    .view
                    .update(ctx, |modal, ctx| {
                        modal.body().update(ctx, |body, ctx| {
                            body.set_templatable_mcp_server(None, None, ctx)
                        });
                    });
                self.installation_modal_state.close();
                ctx.emit(MCPServersSettingsPageEvent::HideModal);

                ctx.notify();
            }
            InstallationModalBodyEvent::Cancel => {
                self.installation_modal_state.close();
                ctx.emit(MCPServersSettingsPageEvent::HideModal);
                ctx.notify();
            }
        }
    }
}

impl Entity for MCPServersSettingsPageView {
    type Event = MCPServersSettingsPageEvent;
}

impl View for MCPServersSettingsPageView {
    fn ui_name() -> &'static str {
        "MCPServersSettingsPageView"
    }

    fn render(&self, _app: &AppContext) -> Box<dyn Element> {
        match self.current_page {
            MCPServersSettingsPage::List => self.page.render(self, _app),
            MCPServersSettingsPage::Edit { item_id: _ } => {
                // The edit view needs to be constrained so we will render it directly
                // instead of rendering inside the settings widget
                Container::new(ChildView::new(&self.edit_view).finish())
                    .with_uniform_padding(style::PAGE_PADDING)
                    .finish()
            }
        }
    }
}

impl TypedActionView for MCPServersSettingsPageView {
    type Action = ();
}

impl SettingsPageMeta for MCPServersSettingsPageView {
    fn section() -> SettingsSection {
        SettingsSection::AgentMCPServers
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
        self.page.clear_highlighted_widget()
    }
}

#[derive(Default)]
pub struct MCPServersSettingsWidget {
    // No state yet
}

impl SettingsWidget for MCPServersSettingsWidget {
    type View = MCPServersSettingsPageView;

    fn search_terms(&self) -> &str {
        "mcp servers"
    }

    fn render(
        &self,
        view: &Self::View,
        _appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        // The settings widget will always return list view
        // The edit view needs to be constrained so we will render it directly
        // instead of rendering inside the settings widget
        ChildView::new(&view.list_view).finish()
    }
}
