use std::collections::HashMap;
use std::path::PathBuf;

use warp_core::features::FeatureFlag;
use warpui::{AppContext, Entity, EntityId, ModelContext, ModelHandle, SingletonEntity};

use super::core::subscribe_to_shared_dependencies;
use super::{SlashCommandDataSource, SlashCommandDataSourceState, UpdatedActiveCommands};
use crate::ai::blocklist::agent_view::{AgentViewController, AgentViewControllerEvent};
use crate::ai::blocklist::block::cli_controller::CLISubagentController;
use crate::search::SyncDataSource;
use crate::search::data_source::{Query, QueryResult};
use crate::search::mixer::DataSourceRunErrorWrapper;
use crate::search::slash_command_menu::StaticCommand;
use crate::search::slash_command_menu::static_commands::Availability;
use crate::search::slash_command_menu::static_commands::commands::COMMAND_REGISTRY;
use crate::settings::{
    InputSettings, InputSettingsChangedEvent, PrivacySettings, PrivacySettingsChangedEvent,
};
use crate::terminal::input::slash_commands::AcceptSlashCommandOrSavedPrompt;
use crate::terminal::model::session::active_session::ActiveSession;

pub struct GuiDataSourceArgs {
    pub active_session: ModelHandle<ActiveSession>,
    pub agent_view_controller: ModelHandle<AgentViewController>,
    pub cli_subagent_controller: ModelHandle<CLISubagentController>,
    pub terminal_view_id: EntityId,
}

pub struct GuiSlashCommandDataSource {
    state: SlashCommandDataSourceState,
    agent_view_controller: ModelHandle<AgentViewController>,
}

impl GuiSlashCommandDataSource {
    pub fn new(args: GuiDataSourceArgs, ctx: &mut ModelContext<Self>) -> Self {
        let GuiDataSourceArgs {
            active_session,
            agent_view_controller,
            cli_subagent_controller,
            terminal_view_id,
        } = args;

        subscribe_to_shared_dependencies(
            &active_session,
            &cli_subagent_controller,
            Self::recompute_active_commands,
            ctx,
        );
        ctx.subscribe_to_model(&agent_view_controller, |me, _, event, ctx| match event {
            AgentViewControllerEvent::EnteredAgentView { .. }
            | AgentViewControllerEvent::ExitedAgentView { .. } => {
                me.recompute_active_commands(ctx);
            }
            _ => (),
        });
        // Preserve the existing GUI subscriptions whose settings affect GUI-only command gates.
        ctx.subscribe_to_model(&PrivacySettings::handle(ctx), |me, _, event, ctx| {
            if matches!(
                event,
                PrivacySettingsChangedEvent::UpdateIsCloudConversationStorageEnabled { .. }
            ) {
                me.recompute_active_commands(ctx);
            }
        });
        ctx.subscribe_to_model(&InputSettings::handle(ctx), |me, _, event, ctx| {
            if matches!(
                event,
                InputSettingsChangedEvent::EnableSlashCommandsInTerminal { .. }
            ) {
                me.recompute_active_commands(ctx);
            }
        });

        let mut me = Self {
            state: SlashCommandDataSourceState::new(
                active_session,
                cli_subagent_controller,
                terminal_view_id,
            ),
            agent_view_controller,
        };
        me.recompute_active_commands(ctx);
        me
    }

    pub fn is_agent_view_active(&self, ctx: &AppContext) -> bool {
        self.agent_view_controller.as_ref(ctx).is_active()
    }

    pub fn set_active_repo_root(
        &mut self,
        repo_root: Option<PathBuf>,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.update_active_repo_root(repo_root) {
            self.recompute_active_commands(ctx);
        }
    }

    pub(crate) fn command_is_active(&self, command: &StaticCommand, ctx: &AppContext) -> bool {
        let availability = self.availability(ctx);
        let gates = self.common_command_gates(ctx);
        command.supports_gui() && self.command_passes_common_gates(command, availability, &gates)
    }

    fn recompute_active_commands(&mut self, ctx: &mut ModelContext<Self>) {
        let availability = self.availability(ctx);
        let gates = self.common_command_gates(ctx);
        let commands = HashMap::from_iter(
            COMMAND_REGISTRY
                .all_commands_by_id()
                .filter(|(_, command)| {
                    command.supports_gui()
                        && self.command_passes_common_gates(command, availability, &gates)
                })
                .map(|(id, command)| (id, command.clone())),
        );
        if self.replace_active_commands(commands) {
            ctx.emit(UpdatedActiveCommands);
        }
    }

    fn availability(&self, ctx: &AppContext) -> Availability {
        let is_agent_view_active = self.is_agent_view_active(ctx);
        let mut availability =
            self.base_availability(ctx) | Self::view_availability(is_agent_view_active);

        if self.has_active_conversation(is_agent_view_active, ctx) {
            availability |= Availability::ACTIVE_CONVERSATION;
        }

        availability | Availability::NOT_CLOUD_AGENT
    }

    /// View-related availability bits for the GUI's legacy terminal-view and agent-view
    /// modalities. When the AgentView feature flag is disabled, both bits are set so either
    /// requirement is satisfied.
    fn view_availability(is_agent_view_active: bool) -> Availability {
        if !FeatureFlag::AgentView.is_enabled() {
            Availability::AGENT_VIEW | Availability::TERMINAL_VIEW
        } else if is_agent_view_active {
            Availability::AGENT_VIEW
        } else {
            Availability::TERMINAL_VIEW
        }
    }
}

impl SyncDataSource for GuiSlashCommandDataSource {
    type Action = AcceptSlashCommandOrSavedPrompt;

    fn run_query(
        &self,
        query: &Query,
        app: &AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        if query.text.is_empty() {
            return Ok(vec![]);
        }

        let query_text = query.text.trim().to_lowercase();
        let mut results = self.match_active_commands(&query_text, app);
        results.extend(self.match_skills(&query_text, app));

        Ok(results.into_iter().map(QueryResult::from).collect())
    }
}

impl SlashCommandDataSource for GuiSlashCommandDataSource {
    fn state(&self) -> &SlashCommandDataSourceState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut SlashCommandDataSourceState {
        &mut self.state
    }
}
impl Entity for GuiSlashCommandDataSource {
    type Event = UpdatedActiveCommands;
}
