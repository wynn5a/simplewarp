use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use settings::Setting as _;
use uuid::Uuid;
use warp_errors::report_error;
use warpui::{AppContext, Entity, EntityId, ModelContext, SingletonEntity};

use super::{
    AIExecutionProfile, ActionPermission, ExecutionProfileId, ExecutionProfilesConfig,
    WriteToPtyPermission,
};
use crate::LaunchMode;
use crate::ai::llms::{LLMId, LLMPreferences};
use crate::ai::mcp::TemplatableMCPServerManager;
use crate::ai::mcp::templatable_manager::TemplatableMCPServerManagerEvent;
use crate::settings::{AISettings, AISettingsChangedEvent, AgentModeCommandExecutionPredicate};

#[derive(Clone, Debug)]
pub struct AIExecutionProfileInfo {
    id: ExecutionProfileId,
    data: AIExecutionProfile,
}

impl AIExecutionProfileInfo {
    pub fn id(&self) -> &ExecutionProfileId {
        &self.id
    }

    pub fn data(&self) -> &AIExecutionProfile {
        &self.data
    }
}

/// Where this launch reads and writes execution profiles.
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
enum ProfileSource {
    /// The collection stored in [`AISettings`] is authoritative.
    Settings,
    /// No collection has been stored yet. Reads see only this implicit default profile until the
    /// first local edit materializes the collection in [`AISettings`], so the empty implicit
    /// settings default is never exposed.
    PendingSettings { default_profile: AIExecutionProfile },
    /// CLI launches use a fixed, more permissive default profile that can't be edited. The
    /// user's local profiles are also readable, so a run can select one of them instead.
    Cli {
        id: ExecutionProfileId,
        profile: AIExecutionProfile,
        local_profiles: ExecutionProfilesConfig,
    },
}

impl ProfileSource {
    fn for_launch_mode(launch_mode: &LaunchMode, ctx: &AppContext) -> Self {
        match launch_mode {
            LaunchMode::App { .. } | LaunchMode::Test { .. } => {
                if AISettings::as_ref(ctx)
                    .execution_profiles
                    .is_value_explicitly_set()
                {
                    Self::Settings
                } else {
                    Self::PendingSettings {
                        default_profile: implicit_default_profile(ctx),
                    }
                }
            }
            LaunchMode::CommandLine {
                is_sandboxed,
                computer_use_override,
                ..
            } => Self::Cli {
                id: ExecutionProfileId::new(),
                profile: AIExecutionProfile::create_default_cli_profile(
                    *is_sandboxed,
                    *computer_use_override,
                ),
                local_profiles: stored_or_implicit_profiles(ctx),
            },
        }
    }
}

/// The profiles the app would show: the stored collection, or just the implicit default profile
/// when none has been stored yet.
fn stored_or_implicit_profiles(ctx: &AppContext) -> ExecutionProfilesConfig {
    let execution_profiles = &AISettings::as_ref(ctx).execution_profiles;
    if execution_profiles.is_value_explicitly_set() {
        execution_profiles.value().clone()
    } else {
        let mut profiles = ExecutionProfilesConfig::default();
        profiles.insert(
            ExecutionProfileId::default_profile(),
            implicit_default_profile(ctx),
        );
        profiles
    }
}

#[cfg(feature = "agent_mode_evals")]
fn implicit_default_profile(_ctx: &AppContext) -> AIExecutionProfile {
    AIExecutionProfile::create_agent_mode_eval_profile()
}

#[cfg(not(feature = "agent_mode_evals"))]
fn implicit_default_profile(ctx: &AppContext) -> AIExecutionProfile {
    super::create_default_from_legacy_settings(ctx)
}

pub struct AIExecutionProfilesModel {
    source: ProfileSource,
    /// Previous settings snapshot used only to classify collection change events.
    last_settings_profiles: ExecutionProfilesConfig,
    /// Only contains entries for non-default profiles.
    active_profiles_per_session: HashMap<EntityId, ExecutionProfileId>,
}

impl AIExecutionProfilesModel {
    pub(crate) fn new(launch_mode: &LaunchMode, ctx: &mut ModelContext<Self>) -> Self {
        let source = ProfileSource::for_launch_mode(launch_mode, ctx);
        let last_settings_profiles = AISettings::as_ref(ctx).execution_profiles.value().clone();

        if !matches!(source, ProfileSource::Cli { .. }) {
            ctx.subscribe_to_model(&AISettings::handle(ctx), |me, _, event, ctx| {
                if matches!(event, AISettingsChangedEvent::ExecutionProfiles) {
                    me.handle_settings_profiles_changed(ctx);
                }
            });
        }

        ctx.subscribe_to_model(
            &TemplatableMCPServerManager::handle(ctx),
            |me, _, event, ctx| {
                me.handle_templatable_mcp_server_manager_event(event, ctx);
            },
        );

        Self {
            source,
            last_settings_profiles,
            active_profiles_per_session: HashMap::new(),
        }
    }

    /// Snapshots the profiles visible while the settings collection is pending, so a local edit
    /// can materialize them.
    fn pending_profiles(&self, ctx: &AppContext) -> ExecutionProfilesConfig {
        let mut profiles = ExecutionProfilesConfig::default();
        for profile_id in self.get_all_profile_ids() {
            if let Some(profile) = self.get_profile_by_id(&profile_id, ctx) {
                profiles.insert(profile_id, profile.data);
            }
        }
        profiles
    }

    /// Returns the collection that an edit starts from, or `None` when this launch's profiles
    /// can't be edited.
    fn editable_profiles(&self, ctx: &AppContext) -> Option<ExecutionProfilesConfig> {
        match &self.source {
            ProfileSource::Settings => {
                Some(AISettings::as_ref(ctx).execution_profiles.value().clone())
            }
            ProfileSource::PendingSettings { .. } => Some(self.pending_profiles(ctx)),
            ProfileSource::Cli { .. } => None,
        }
    }

    /// Persists an edited collection in [`AISettings`], which makes it authoritative.
    ///
    /// Returns `false` when the collection can't be persisted.
    fn persist_profiles(
        &mut self,
        profiles: ExecutionProfilesConfig,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        let update_result = AISettings::handle(ctx).update(ctx, |settings, ctx| {
            settings.execution_profiles.set_value(profiles, ctx)
        });
        match update_result {
            Ok(()) => {
                self.source = ProfileSource::Settings;
                true
            }
            Err(error) => {
                report_error!(error.context("Failed to persist execution profile settings"));
                false
            }
        }
    }

    /// Classifies a collection update into profile events and removes stale selections.
    fn handle_settings_profiles_changed(&mut self, ctx: &mut ModelContext<Self>) {
        let current = AISettings::as_ref(ctx).execution_profiles.value().clone();
        let previous_ids = self
            .last_settings_profiles
            .profile_ids()
            .cloned()
            .collect::<HashSet<_>>();
        let current_ids = current.profile_ids().cloned().collect::<HashSet<_>>();

        if current_ids.iter().any(|id| !previous_ids.contains(id)) {
            ctx.emit(AIExecutionProfilesModelEvent::ProfileCreated);
        }
        if previous_ids.iter().any(|id| !current_ids.contains(id)) {
            ctx.emit(AIExecutionProfilesModelEvent::ProfileDeleted);
        }
        for id in current_ids.intersection(&previous_ids) {
            if current.profile(id) != self.last_settings_profiles.profile(id) {
                ctx.emit(AIExecutionProfilesModelEvent::ProfileUpdated((*id).clone()));
            }
        }
        self.active_profiles_per_session
            .retain(|_, id| current_ids.contains(id));
        self.last_settings_profiles = current;
        if matches!(self.source, ProfileSource::PendingSettings { .. })
            && AISettings::as_ref(ctx)
                .execution_profiles
                .is_value_explicitly_set()
        {
            self.source = ProfileSource::Settings;
        }
    }

    pub fn create_profile(&mut self, ctx: &mut ModelContext<Self>) -> Option<ExecutionProfileId> {
        let Some(mut profiles) = self.editable_profiles(ctx) else {
            log::warn!("Attempted to create an execution profile, which the CLI doesn't support.");
            return None;
        };
        let profile_id = ExecutionProfileId::new();
        let mut new_profile = self.default_profile(ctx).data().clone();
        new_profile.name = String::new();
        new_profile.is_default_profile = false;
        profiles.insert(profile_id.clone(), new_profile);
        self.persist_profiles(profiles, ctx).then_some(profile_id)
    }

    pub fn delete_profile(
        &mut self,
        profile_id: &ExecutionProfileId,
        ctx: &mut ModelContext<Self>,
    ) {
        if *profile_id == self.default_profile_id() {
            log::warn!("Attempted to delete default profile (id: {profile_id})");
            return;
        }
        let Some(mut profiles) = self.editable_profiles(ctx) else {
            return;
        };
        if profiles.remove(profile_id).is_none() {
            return;
        }
        if !self.persist_profiles(profiles, ctx) {
            return;
        }
        self.active_profiles_per_session
            .retain(|_, active_profile_id| active_profile_id != profile_id);
    }

    /// Returns the active permissions profile for a specific terminal view.
    /// If no terminal_view is provided, returns the default profile.
    pub fn active_profile(
        &self,
        terminal_view_id: Option<EntityId>,
        ctx: &AppContext,
    ) -> AIExecutionProfileInfo {
        terminal_view_id
            .and_then(|id| self.active_profiles_per_session.get(&id))
            .and_then(|profile_id| self.get_profile_by_id(profile_id, ctx))
            .unwrap_or_else(|| self.default_profile(ctx))
    }

    pub fn default_profile_id(&self) -> ExecutionProfileId {
        match &self.source {
            ProfileSource::Settings | ProfileSource::PendingSettings { .. } => {
                ExecutionProfileId::default_profile()
            }
            ProfileSource::Cli { id, .. } => id.clone(),
        }
    }

    pub fn default_profile(&self, ctx: &AppContext) -> AIExecutionProfileInfo {
        let id = self.default_profile_id();
        let data = match &self.source {
            ProfileSource::Settings => AISettings::as_ref(ctx)
                .execution_profiles
                .value()
                .profile(&id)
                .cloned()
                .unwrap_or_else(|| {
                    report_error!("Execution profile settings are missing the default profile");
                    AIExecutionProfile {
                        name: "Default".to_string(),
                        is_default_profile: true,
                        ..Default::default()
                    }
                }),
            ProfileSource::PendingSettings { default_profile } => default_profile.clone(),
            ProfileSource::Cli { profile, .. } => profile.clone(),
        };
        AIExecutionProfileInfo { id, data }
    }

    /// Sets the active profile for a specific terminal view.
    pub fn set_active_profile(
        &mut self,
        terminal_view_id: EntityId,
        profile_id: ExecutionProfileId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.active_profiles_per_session
            .insert(terminal_view_id, profile_id);
        ctx.emit(AIExecutionProfilesModelEvent::UpdatedActiveProfile { terminal_view_id });
    }

    /// Returns a profile by its ID.
    /// Returns None if the profile is not found.
    pub fn get_profile_by_id(
        &self,
        profile_id: &ExecutionProfileId,
        ctx: &AppContext,
    ) -> Option<AIExecutionProfileInfo> {
        match &self.source {
            ProfileSource::Settings => AISettings::as_ref(ctx)
                .execution_profiles
                .value()
                .profile(profile_id)
                .cloned()
                .map(|data| AIExecutionProfileInfo {
                    id: profile_id.clone(),
                    data,
                }),
            ProfileSource::PendingSettings { .. } => {
                (*profile_id == self.default_profile_id()).then(|| self.default_profile(ctx))
            }
            ProfileSource::Cli { local_profiles, .. } => {
                if *profile_id == self.default_profile_id() {
                    Some(self.default_profile(ctx))
                } else {
                    local_profiles
                        .profile(profile_id)
                        .cloned()
                        .map(|data| AIExecutionProfileInfo {
                            id: profile_id.clone(),
                            data,
                        })
                }
            }
        }
    }

    /// The user's local profiles: the ones the app shows, even in a CLI launch whose own default
    /// profile is the fixed CLI one.
    pub fn local_profiles(&self, ctx: &AppContext) -> ExecutionProfilesConfig {
        match &self.source {
            ProfileSource::Settings => AISettings::as_ref(ctx).execution_profiles.value().clone(),
            ProfileSource::PendingSettings { .. } => self.pending_profiles(ctx),
            ProfileSource::Cli { local_profiles, .. } => local_profiles.clone(),
        }
    }

    pub fn get_all_profile_ids(&self) -> Vec<ExecutionProfileId> {
        match &self.source {
            ProfileSource::Settings => self.last_settings_profiles.profile_ids().cloned().collect(),
            ProfileSource::PendingSettings { .. } | ProfileSource::Cli { .. } => {
                vec![self.default_profile_id()]
            }
        }
    }

    pub fn has_multiple_profiles(&self) -> bool {
        match &self.source {
            ProfileSource::Settings => self.last_settings_profiles.profile_ids().nth(1).is_some(),
            ProfileSource::PendingSettings { .. } | ProfileSource::Cli { .. } => false,
        }
    }

    pub fn set_base_model(
        &mut self,
        profile_id: &ExecutionProfileId,
        llm_id: Option<LLMId>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.base_model != llm_id {
                    profile.base_model = llm_id.clone();
                    return true;
                }
                false
            },
            ctx,
        );

        if let Some(_model_id) = &llm_id {}
    }

    pub fn set_coding_model(
        &mut self,
        profile_id: &ExecutionProfileId,
        model_id: Option<LLMId>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.coding_model != model_id {
                    profile.coding_model = model_id.clone();
                    return true;
                }
                false
            },
            ctx,
        );

        if let Some(_model_id) = &model_id {}
    }

    pub fn set_cli_agent_model(
        &mut self,
        profile_id: &ExecutionProfileId,
        model_id: Option<LLMId>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.cli_agent_model != model_id {
                    profile.cli_agent_model = model_id.clone();
                    return true;
                }
                false
            },
            ctx,
        );

        if let Some(_model_id) = &model_id {}
    }

    pub fn set_computer_use_model(
        &mut self,
        profile_id: &ExecutionProfileId,
        model_id: Option<LLMId>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.computer_use_model != model_id {
                    profile.computer_use_model = model_id.clone();
                    return true;
                }
                false
            },
            ctx,
        );

        if let Some(_model_id) = &model_id {}
    }

    pub fn set_context_window_limit(
        &mut self,
        profile_id: &ExecutionProfileId,
        limit: Option<u32>,
        ctx: &mut ModelContext<Self>,
    ) {
        let changed = self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.context_window_limit != limit {
                    profile.context_window_limit = limit;
                    return true;
                }
                false
            },
            ctx,
        );

        // Gate on the limit being non-empty. The limit is cleared during
        // reconciliation, which runs inside an `LLMPreferences` update where the
        // `LLMPreferences::as_ref` read below would panic.
        if changed && limit.is_some() {
            let Some(profile) = self.get_profile_by_id(profile_id, ctx) else {
                return;
            };
            let llm_preferences = LLMPreferences::as_ref(ctx);
            let _model_info = profile
                .data()
                .base_model
                .as_ref()
                .and_then(|id| llm_preferences.get_llm_info(id))
                .unwrap_or_else(|| llm_preferences.get_default_base_model(ctx));
        }
    }

    pub fn set_apply_code_diffs(
        &mut self,
        profile_id: &ExecutionProfileId,
        apply_code_diffs: &ActionPermission,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.apply_code_diffs != *apply_code_diffs {
                    profile.apply_code_diffs = *apply_code_diffs;
                    return true;
                }
                false
            },
            ctx,
        );
    }

    pub fn set_read_files(
        &mut self,
        profile_id: &ExecutionProfileId,
        read_files: &ActionPermission,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.read_files != *read_files {
                    profile.read_files = *read_files;
                    return true;
                }
                false
            },
            ctx,
        );
    }

    pub fn set_execute_commands(
        &mut self,
        profile_id: &ExecutionProfileId,
        execute_commands: &ActionPermission,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.execute_commands != *execute_commands {
                    profile.execute_commands = *execute_commands;
                    return true;
                }
                false
            },
            ctx,
        );
    }

    pub fn set_write_to_pty(
        &mut self,
        profile_id: &ExecutionProfileId,
        write_to_pty: &WriteToPtyPermission,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.write_to_pty != *write_to_pty {
                    profile.write_to_pty = *write_to_pty;
                    return true;
                }
                false
            },
            ctx,
        );
    }

    pub fn set_mcp_permissions(
        &mut self,
        profile_id: &ExecutionProfileId,
        mcp_permissions: &ActionPermission,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.mcp_permissions == *mcp_permissions {
                    return false;
                }

                if mcp_permissions == &ActionPermission::AlwaysAllow {
                    profile.mcp_allowlist.clear();
                } else if mcp_permissions == &ActionPermission::AlwaysAsk {
                    profile.mcp_denylist.clear();
                }
                profile.mcp_permissions = *mcp_permissions;
                true
            },
            ctx,
        );
    }

    pub fn set_computer_use(
        &mut self,
        profile_id: &ExecutionProfileId,
        permission: &super::ComputerUsePermission,
        ctx: &mut ModelContext<Self>,
    ) {
        let current_value = self
            .get_profile_by_id(profile_id, ctx)
            .map(|p| p.data().computer_use);

        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.computer_use != *permission {
                    profile.computer_use = *permission;
                    return true;
                }
                false
            },
            ctx,
        );

        if current_value != Some(*permission) {}
    }

    pub fn set_ask_user_question(
        &mut self,
        profile_id: &ExecutionProfileId,
        permission: super::AskUserQuestionPermission,
        ctx: &mut ModelContext<Self>,
    ) {
        let current_value = self
            .get_profile_by_id(profile_id, ctx)
            .map(|p| p.data().ask_user_question);

        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.ask_user_question != permission {
                    profile.ask_user_question = permission;
                    return true;
                }
                false
            },
            ctx,
        );

        if current_value != Some(permission) {}
    }

    pub fn set_run_agents(
        &mut self,
        profile_id: &ExecutionProfileId,
        permission: super::RunAgentsPermission,
        ctx: &mut ModelContext<Self>,
    ) {
        let current_value = self
            .get_profile_by_id(&profile_id.clone(), ctx)
            .map(|p| p.data().run_agents);

        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.run_agents != permission {
                    profile.run_agents = permission;
                    return true;
                }
                false
            },
            ctx,
        );

        if current_value != Some(permission) {}
    }

    pub fn set_web_search_enabled(
        &mut self,
        profile_id: &ExecutionProfileId,
        enabled: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.web_search_enabled != enabled {
                    profile.web_search_enabled = enabled;
                    return true;
                }
                false
            },
            ctx,
        );
    }

    pub fn set_profile_name(
        &mut self,
        profile_id: &ExecutionProfileId,
        name: &str,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if profile.name != name {
                    profile.name = name.to_string();
                    return true;
                }
                false
            },
            ctx,
        );
    }

    pub fn add_to_command_allowlist(
        &mut self,
        profile_id: &ExecutionProfileId,
        predicate: &AgentModeCommandExecutionPredicate,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if !profile.command_allowlist.contains(predicate) {
                    profile.command_allowlist.push(predicate.clone());
                    return true;
                }
                false
            },
            ctx,
        );
    }

    pub fn remove_from_command_allowlist(
        &mut self,
        profile_id: &ExecutionProfileId,
        predicate: &AgentModeCommandExecutionPredicate,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                let original_len = profile.command_allowlist.len();
                profile.command_allowlist.retain(|p| p != predicate);
                profile.command_allowlist.len() != original_len
            },
            ctx,
        );
    }

    pub fn add_to_directory_allowlist(
        &mut self,
        profile_id: &ExecutionProfileId,
        path: &PathBuf,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if !profile.directory_allowlist.contains(path) {
                    profile.directory_allowlist.push(path.clone());
                    return true;
                }
                false
            },
            ctx,
        );
    }

    pub fn remove_from_directory_allowlist(
        &mut self,
        profile_id: &ExecutionProfileId,
        path: &PathBuf,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                let original_len = profile.directory_allowlist.len();
                profile.directory_allowlist.retain(|p| p != path);
                profile.directory_allowlist.len() != original_len
            },
            ctx,
        );
    }

    pub fn add_to_command_denylist(
        &mut self,
        profile_id: &ExecutionProfileId,
        predicate: &AgentModeCommandExecutionPredicate,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if !profile.command_denylist.contains(predicate) {
                    profile.command_denylist.push(predicate.clone());
                    return true;
                }
                false
            },
            ctx,
        );
    }

    pub fn remove_from_command_denylist(
        &mut self,
        profile_id: &ExecutionProfileId,
        predicate: &AgentModeCommandExecutionPredicate,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                let original_len = profile.command_denylist.len();
                profile.command_denylist.retain(|p| p != predicate);
                profile.command_denylist.len() != original_len
            },
            ctx,
        );
    }

    pub fn add_to_mcp_allowlist(
        &mut self,
        profile_id: &ExecutionProfileId,
        id: &Uuid,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if !profile.mcp_allowlist.contains(id) {
                    profile.mcp_allowlist.push(*id);
                    return true;
                }
                false
            },
            ctx,
        );
    }

    pub fn remove_from_mcp_allowlist(
        &mut self,
        profile_id: &ExecutionProfileId,
        id: &Uuid,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                let original_len = profile.mcp_allowlist.len();
                profile.mcp_allowlist.retain(|p| p != id);
                profile.mcp_allowlist.len() != original_len
            },
            ctx,
        );
    }

    pub fn add_to_mcp_denylist(
        &mut self,
        profile_id: &ExecutionProfileId,
        id: &Uuid,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                if !profile.mcp_denylist.contains(id) {
                    profile.mcp_denylist.push(*id);
                    return true;
                }
                false
            },
            ctx,
        );
    }

    pub fn remove_from_mcp_denylist(
        &mut self,
        profile_id: &ExecutionProfileId,
        id: &Uuid,
        ctx: &mut ModelContext<Self>,
    ) {
        self.edit_profile_internal(
            profile_id,
            |profile| {
                let original_len = profile.mcp_denylist.len();
                profile.mcp_denylist.retain(|p| p != id);
                profile.mcp_denylist.len() != original_len
            },
            ctx,
        );
    }

    /// Edits a profile and persists the changed collection.
    ///
    /// `edit_fn` returns whether it changed the profile; nothing is persisted when it didn't.
    /// Returns `true` if the profile was changed and persisted.
    fn edit_profile_internal(
        &mut self,
        profile_id: &ExecutionProfileId,
        edit_fn: impl FnOnce(&mut AIExecutionProfile) -> bool,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        let Some(mut profiles) = self.editable_profiles(ctx) else {
            log::warn!("Attempted to edit CLI default profile, which is not yet supported.");
            return false;
        };
        let Some(profile) = profiles.profile_mut(profile_id) else {
            return false;
        };
        if !edit_fn(profile) {
            return false;
        }
        self.persist_profiles(profiles, ctx)
    }

    fn handle_templatable_mcp_server_manager_event(
        &mut self,
        event: &TemplatableMCPServerManagerEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        match event {
            TemplatableMCPServerManagerEvent::TemplatableMCPServersUpdated => {
                self.remove_deleted_mcp_servers(ctx);
            }
            TemplatableMCPServerManagerEvent::LegacyServerConverted
            | TemplatableMCPServerManagerEvent::StateChanged { uuid: _, state: _ }
            | TemplatableMCPServerManagerEvent::AuthenticationRequired { uuid: _ }
            | TemplatableMCPServerManagerEvent::CredentialsChanged { uuid: _ }
            | TemplatableMCPServerManagerEvent::ServerInstallationAdded(_)
            | TemplatableMCPServerManagerEvent::ServerInstallationDeleted(_) => {}
        }
    }

    /// Handle deleted MCP servers by deleting its uuid from all profiles.
    fn remove_deleted_mcp_servers(&mut self, ctx: &mut ModelContext<Self>) {
        let all_valid_uuids = TemplatableMCPServerManager::get_all_cloud_synced_mcp_servers(ctx);
        for profile_id in self.get_all_profile_ids() {
            self.edit_profile_internal(
                &profile_id,
                |profile| {
                    let original_allowlist_len = profile.mcp_allowlist.len();
                    let original_denylist_len = profile.mcp_denylist.len();
                    profile
                        .mcp_allowlist
                        .retain(|uuid| all_valid_uuids.contains_key(uuid));
                    profile
                        .mcp_denylist
                        .retain(|uuid| all_valid_uuids.contains_key(uuid));
                    profile.mcp_allowlist.len() != original_allowlist_len
                        || profile.mcp_denylist.len() != original_denylist_len
                },
                ctx,
            );
        }
    }

    /// Replaces the given profile's data with CLI defaults for the given sandboxed state.
    /// Use in tests to simulate the profile configuration used by the sandboxed CLI agent.
    #[cfg(test)]
    pub fn apply_cli_profile_defaults_for_test(
        &mut self,
        profile_id: &ExecutionProfileId,
        is_sandboxed: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let cli_profile = AIExecutionProfile::create_default_cli_profile(is_sandboxed, None);
        self.edit_profile_internal(
            profile_id,
            move |profile| {
                *profile = cli_profile;
                true
            },
            ctx,
        );
    }
}

#[allow(clippy::enum_variant_names)]
pub enum AIExecutionProfilesModelEvent {
    ProfileUpdated(ExecutionProfileId),
    ProfileCreated,
    ProfileDeleted,
    UpdatedActiveProfile { terminal_view_id: EntityId },
}

impl Entity for AIExecutionProfilesModel {
    type Event = AIExecutionProfilesModelEvent;
}

impl SingletonEntity for AIExecutionProfilesModel {}

#[cfg(test)]
#[path = "profiles_tests.rs"]
mod tests;
