use std::collections::HashMap;

use regex::Regex;
use warp_core::features::FeatureFlag;
use warp_core::settings::{ChangeEventReason, Setting};
use warpui::{
    AppContext, Entity, ModelContext, SingletonEntity, Tracked, ViewContext, WeakViewHandle,
    WindowId,
};

use super::team::Team;
#[cfg(test)]
use super::team::{MembershipRole, TeamVisibility};
#[cfg(test)]
use super::workspace::WorkspaceMemberUsageInfo;
use super::workspace::{
    AdminEnablementSetting, EnterpriseSecretRegex, HostEnablementSetting,
    UgcCollectionEnablementSetting, Workspace, WorkspaceUid,
};
use crate::ai::llms::LLMModelHost;
use crate::auth::{AuthStateProvider, UserUid};
use crate::channel::ChannelState;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::{ObjectType, Owner, Space};
use crate::server::ids::ServerId;
use crate::settings::{
    AISettings, AISettingsChangedEvent, CodeSettings, CodeSettingsChangedEvent, PrivacySettings,
};
#[cfg(test)]
use crate::workspaces::workspace::BillingMetadata;
use crate::workspaces::workspace::{AiAutonomySettings, SandboxedAgentSettings};
#[cfg(test)]
use crate::workspaces::workspace::{WorkspaceMember, WorkspaceSettings};

const STRIPE_SUBSCRIPTION_INTERVAL_PAGE_PREFIX: &str = "/upgrade";

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub enum UserWorkspacesEvent {
    /// Fired whenever the set of teams the user is on changes.
    TeamsChanged,
    /// Fired when the selected workspace actually changes to a different one.
    CurrentWorkspaceChanged,
    /// Fired when a single window's team assignment changes. Windows are independent, so
    /// subscribers that hold per-window state must only react to their own window.
    WindowTeamChanged,
    CodebaseContextEnablementChanged,
}

/// UserWorkspaces is a singleton model that holds workspace metadata (name, members, etc).
/// It should be used for getting information about the workspaces, teams, current teams,
/// and all other things related to operating on workspace and team data.
/// TODO: move other server_api calls to update_manager to correctly update sqlite.
pub struct UserWorkspaces {
    current_workspace_uid: Tracked<Option<WorkspaceUid>>,
    workspaces: Tracked<Vec<Workspace>>,
    window_team_uids: HashMap<WindowId, Option<ServerId>>,
}

impl UserWorkspaces {
    #[cfg(test)]
    pub fn mock(cached_workspaces: Vec<Workspace>, _ctx: &mut ModelContext<Self>) -> Self {
        Self {
            current_workspace_uid: cached_workspaces.first().map(|w| w.uid).into(),
            workspaces: cached_workspaces.into(),
            window_team_uids: Default::default(),
        }
    }

    #[cfg(test)]
    pub fn default_mock(ctx: &mut ModelContext<Self>) -> Self {
        Self::mock(vec![], ctx)
    }

    pub fn new(
        cached_workspaces: Vec<Workspace>,
        current_workspace_uid: Option<WorkspaceUid>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        ctx.subscribe_to_model(
            &CodeSettings::handle(ctx),
            |_, _, code_settings_event, ctx| {
                if let CodeSettingsChangedEvent::CodebaseContextEnabled { .. } = code_settings_event
                {
                    ctx.emit(UserWorkspacesEvent::CodebaseContextEnablementChanged);
                }
            },
        );

        ctx.subscribe_to_model(&AISettings::handle(ctx), |_, _, ai_settings_event, ctx| {
            if let AISettingsChangedEvent::IsAnyAIEnabled { .. } = ai_settings_event {
                ctx.emit(UserWorkspacesEvent::CodebaseContextEnablementChanged);
            }
        });

        Self {
            current_workspace_uid: current_workspace_uid.into(),
            workspaces: cached_workspaces.into(),
            window_team_uids: Default::default(),
        }
    }

    pub fn upgrade_link(user_id: UserUid) -> String {
        format!(
            "{}{}/{}/{}",
            ChannelState::server_root_url(),
            STRIPE_SUBSCRIPTION_INTERVAL_PAGE_PREFIX,
            "user",
            user_id.as_str()
        )
    }

    pub fn upgrade_link_for_team(team_uid: ServerId) -> String {
        format!(
            "{}{}/{}",
            ChannelState::server_root_url(),
            STRIPE_SUBSCRIPTION_INTERVAL_PAGE_PREFIX,
            team_uid
        )
    }

    pub fn team_from_uid(&self, team_uid: ServerId) -> Option<&Team> {
        self.current_workspace()
            .and_then(|w| w.teams.iter().find(|t| t.uid == team_uid))
    }

    pub fn register_window(
        &mut self,
        window_id: WindowId,
        team_uid: Option<ServerId>,
        ctx: &mut ModelContext<Self>,
    ) {
        let previous_team_uid = self.team_uid_for_window(window_id);
        self.window_team_uids.entry(window_id).or_insert(team_uid);
        if self.team_uid_for_window(window_id) != previous_team_uid {
            ctx.emit(UserWorkspacesEvent::WindowTeamChanged);
        }
        ctx.notify();
    }
    pub fn inherited_or_default_team_uid(
        &self,
        source_window_id: Option<WindowId>,
    ) -> Option<ServerId> {
        source_window_id
            .and_then(|source_window_id| self.team_uid_for_window(source_window_id))
            .or_else(|| {
                self.current_workspace()
                    .and_then(|workspace| workspace.teams.first())
                    .map(|team| team.uid)
            })
    }

    pub fn team_uid_for_window(&self, window_id: WindowId) -> Option<ServerId> {
        self.window_team_uids.get(&window_id).copied().flatten()
    }

    /// Returns `true` when the user belongs to more than one team in the current
    /// workspace, meaning the team-switcher pill and dropdown should be shown.
    /// Single-team and no-workspace users return `false` so their UI is unchanged.
    pub fn can_switch_teams(&self) -> bool {
        self.current_workspace()
            .map(|ws| ws.teams.len() > 1)
            .unwrap_or(false)
    }
    pub fn team_for_window(&self, window_id: WindowId) -> Option<&Team> {
        self.team_uid_for_window(window_id)
            .and_then(|team_uid| self.team_from_uid(team_uid))
    }
    pub fn team_for_view<T: Entity>(&self, ctx: &ViewContext<T>) -> Option<&Team> {
        self.team_for_window(ctx.window_id())
    }

    pub fn team_for_view_handle<T: Entity>(
        &self,
        view_handle: &WeakViewHandle<T>,
        ctx: &AppContext,
    ) -> Option<&Team> {
        view_handle
            .window_id(ctx)
            .and_then(|window_id| self.team_for_window(window_id))
    }

    /// Returns the windows whose team assignment changed.
    #[must_use]
    fn reconcile_window_team_assignments(&mut self) -> Vec<WindowId> {
        let team_uids = self
            .current_workspace()
            .map(|workspace| {
                workspace
                    .teams
                    .iter()
                    .map(|team| team.uid)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let fallback_team_uid = team_uids.first().copied();

        let mut reassigned_windows = Vec::new();
        for (window_id, window_team_uid) in self.window_team_uids.iter_mut() {
            if window_team_uid.is_none_or(|team_uid| !team_uids.contains(&team_uid))
                && *window_team_uid != fallback_team_uid
            {
                *window_team_uid = fallback_team_uid;
                reassigned_windows.push(*window_id);
            }
        }
        reassigned_windows
    }

    fn emit_window_team_changed(windows: Vec<WindowId>, ctx: &mut ModelContext<Self>) {
        for _ in windows {
            ctx.emit(UserWorkspacesEvent::WindowTeamChanged);
        }
    }

    pub fn workspace_from_uid(&self, workspace_uid: WorkspaceUid) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.uid == workspace_uid)
    }

    pub fn is_at_tier_limit_for_object_type(
        team_uid: ServerId,
        object_type: ObjectType,
        ctx: &AppContext,
    ) -> bool {
        match object_type {
            ObjectType::Notebook => {
                !UserWorkspaces::has_capacity_for_shared_notebooks(team_uid, ctx, 1)
            }
            ObjectType::Workflow => {
                !UserWorkspaces::has_capacity_for_shared_workflows(team_uid, ctx, 1)
            }
            ObjectType::Folder => false,
            ObjectType::GenericStringObject(_) => false,
        }
    }

    // Checks if the team has capacity for another shared notebook for their current
    // billing tier, given their current notebook count and delinquency status.
    pub fn has_capacity_for_shared_notebooks(
        team_uid: ServerId,
        ctx: &AppContext,
        new_shared_notebooks: usize,
    ) -> bool {
        let current_shared_notebooks = CloudModel::as_ref(ctx)
            .active_notebooks_in_space(Space::Team { team_uid }, ctx)
            .count();

        let team = UserWorkspaces::as_ref(ctx).team_from_uid(team_uid);
        if let Some(team) = team {
            // If the team is past due or unpaid, then don't allow new notebooks.
            if team.billing_metadata.is_delinquent_due_to_payment_issue() {
                return false;
            }

            if let Some(policy) = team.billing_metadata.tier.shared_notebooks_policy {
                // Allow new notebooks if policy is unlimited or if the number of notebooks
                // is less than the limit.
                policy.is_unlimited
                    || current_shared_notebooks + new_shared_notebooks
                        <= policy
                            .limit
                            .try_into()
                            .expect("shared notebooks limit should be within max i64 range")
            } else {
                // If no policy is set, then allow it to go through by default (should still be enforced server-side)
                true
            }
        } else {
            // If the team is not found, then allow it to go through by default (should still be enforced server-side)
            true
        }
    }

    // Checks if the team has capacity for another shared workflow for their current
    // billing tier, given their current workflow count and delinquency status.
    pub fn has_capacity_for_shared_workflows(
        team_uid: ServerId,
        ctx: &AppContext,
        new_shared_workflows: usize,
    ) -> bool {
        let current_shared_workflows = CloudModel::as_ref(ctx)
            .active_workflows_in_space(Space::Team { team_uid }, ctx)
            .count();

        let team = UserWorkspaces::as_ref(ctx).team_from_uid(team_uid);
        if let Some(team) = team {
            // If the team is past due or unpaid, then don't allow new workflows.
            if team.billing_metadata.is_delinquent_due_to_payment_issue() {
                return false;
            }

            if let Some(policy) = team.billing_metadata.tier.shared_workflows_policy {
                // Allow new workflows if policy is unlimited or if the number of workflows
                // is less than the limit.
                policy.is_unlimited
                    || current_shared_workflows + new_shared_workflows
                        <= policy
                            .limit
                            .try_into()
                            .expect("shared workflows limit should be within max i64 range")
            } else {
                // If no policy is set, then allow it to go through by default (should still be enforced server-side)
                true
            }
        } else {
            // If the team is not found, then allow it to go through by default (should still be enforced server-side)
            true
        }
    }

    pub fn sole_team(&self) -> Option<&Team> {
        let [team] = self.current_workspace()?.teams.as_slice() else {
            return None;
        };
        Some(team)
    }

    pub fn sole_team_uid(&self) -> Option<ServerId> {
        self.sole_team().map(|team| team.uid)
    }

    /// Note that the workspace is populated with dummy data until the initial fetch
    /// completes (only workspace name/ID and workspace team's name/ID are cached in
    /// sqlite locally).
    /// Consider whether you need to wait for the results of the fetch before checking the
    /// values of other fields.
    pub fn current_workspace(&self) -> Option<&Workspace> {
        self.current_workspace_uid
            .and_then(|workspace_uid| self.workspace_from_uid(workspace_uid))
    }
    pub fn workspaces(&self) -> &Vec<Workspace> {
        &self.workspaces
    }

    pub fn set_current_workspace_uid(
        &mut self,
        workspace_uid: WorkspaceUid,
        ctx: &mut ModelContext<Self>,
    ) {
        let changed = *self.current_workspace_uid != Some(workspace_uid);
        *self.current_workspace_uid = Some(workspace_uid);
        let reassigned_windows = self.reconcile_window_team_assignments();
        self.notify_and_emit_teams_changed(ctx);
        Self::emit_window_team_changed(reassigned_windows, ctx);
        if changed {
            ctx.emit(UserWorkspacesEvent::CurrentWorkspaceChanged);
        }
    }

    /// Whether Prompt Suggestions should be toggleable for the current user, based on the active policies.
    /// Note that the value may be incorrect if called before the team's billing metadata has been fetched.
    pub fn is_prompt_suggestions_toggleable(&self) -> bool {
        self.current_workspace()
            // If the user has no team, they can toggle prompt suggestions (no restrictions).
            .is_none_or(|workspace| {
                workspace
                    .billing_metadata
                    .tier
                    .warp_ai_policy
                    .is_some_and(|policy| policy.is_prompt_suggestions_toggleable)
            })
    }

    /// Whether Code Suggestions should be toggleable for the current user, based on the active policies.
    /// Note that the value may be incorrect if called before the team's billing metadata has been fetched.
    pub fn is_code_suggestions_toggleable(&self) -> bool {
        self.current_workspace()
            // If the user has no team, they can toggle code suggestions (no restrictions).
            .is_none_or(|workspace| {
                workspace
                    .billing_metadata
                    .tier
                    .warp_ai_policy
                    .is_some_and(|policy| policy.is_code_suggestions_toggleable)
            })
    }

    /// Whether Next Command should be toggleable for the current user, based on the active policies.
    /// Note that the value may be incorrect if called before the team's billing metadata has been fetched.
    pub fn is_next_command_enabled(&self) -> bool {
        self.current_workspace()
            // If the user has no team, they can toggle Next Command (no restrictions).
            .is_none_or(|workspace| {
                workspace
                    .billing_metadata
                    .tier
                    .warp_ai_policy
                    .is_some_and(|policy| policy.is_next_command_enabled)
            })
    }

    /// Whether Git Operations AI is enabled for the current user, based on the active policies.
    /// Note that the value may be incorrect if called before the team's billing metadata has been fetched.
    pub fn is_git_operations_ai_enabled(&self) -> bool {
        self.current_workspace()
            // If the user has no team, they can toggle Git Operations AI (no restrictions).
            .is_none_or(|workspace| {
                workspace
                    .billing_metadata
                    .tier
                    .warp_ai_policy
                    .is_some_and(|policy| policy.is_git_operations_ai_enabled)
            })
    }

    /// Whether voice input should be toggleable for the current user, based on the active policies.
    /// Note that the value may be incorrect if called before the team's billing metadata has been fetched.
    /// If voice input support is not compiled into this build, always returns `false`.
    pub fn is_voice_enabled(&self) -> bool {
        cfg!(feature = "voice_input")
            && self
                .current_workspace()
                // If the user has no team, they can toggle Voice (no restrictions).
                .is_none_or(|workspace| {
                    workspace
                        .billing_metadata
                        .tier
                        .warp_ai_policy
                        .is_some_and(|policy| policy.is_voice_enabled)
                })
    }

    /// Whether BYO API key is enabled for the current user.
    ///
    /// Always true. There is no Warp account and no Warp-billed inference in this build, so a
    /// user key is the only way to reach a model at all. The policy and logged-out checks this
    /// used to make would turn BYOK off for every user and leave the AI with no path.
    pub fn is_byo_api_key_enabled(&self, _app: &AppContext) -> bool {
        true
    }

    /// Whether the current workspace's managed BYOK/BYOE policy allows members
    /// to use their own provider API keys. Users with no workspace, or
    /// workspaces without the managed BYOK/BYOE policy, have no team-level
    /// restriction, so this returns true and the normal BYO entitlement applies.
    pub fn are_member_byo_keys_allowed(&self) -> bool {
        self.current_workspace().is_none_or(|workspace| {
            !workspace.billing_metadata.is_managed_byok_byoe_enabled()
                || workspace
                    .settings
                    .team_byo
                    .as_ref()
                    .is_some_and(|team_byo| {
                        team_byo.first_party_enabled && team_byo.allow_user_keys
                    })
        })
    }
    /// Whether custom inference endpoints are enabled for the current user.
    ///
    /// Always true, for the same reason as [`Self::is_byo_api_key_enabled`]: a custom endpoint is
    /// a first-class way to reach a model here, and the checks this used to make would switch it
    /// off for every user.
    pub fn is_custom_inference_enabled(&self, _app: &AppContext) -> bool {
        true
    }

    /// Whether the current workspace's managed BYOK/BYOE policy allows members
    /// to use their own custom endpoints. Users with no workspace, or
    /// workspaces without the managed BYOK/BYOE policy, have no team-level
    /// restriction, so this returns true and the normal BYO entitlement applies.
    pub fn are_member_byo_endpoints_allowed(&self) -> bool {
        self.current_workspace().is_none_or(|workspace| {
            !workspace.billing_metadata.is_managed_byok_byoe_enabled()
                || workspace
                    .settings
                    .team_byo
                    .as_ref()
                    .is_some_and(|team_byo| {
                        team_byo.endpoints_enabled && team_byo.allow_user_endpoints
                    })
        })
    }

    pub fn aws_bedrock_host_settings(&self) -> Option<&super::workspace::LlmHostSettings> {
        self.current_workspace().and_then(|workspace| {
            workspace
                .settings
                .llm_settings
                .host_configs
                .get(&LLMModelHost::AwsBedrock)
        })
    }

    /// Did the admin enable AWS Bedrock for the current workspace?
    pub fn is_aws_bedrock_available_from_workspace(&self) -> bool {
        self.current_workspace().is_some_and(|workspace| {
            workspace.settings.llm_settings.enabled
                && self
                    .aws_bedrock_host_settings()
                    .is_some_and(|settings| settings.enabled)
        })
    }
    pub fn aws_bedrock_host_enablement_setting(&self) -> HostEnablementSetting {
        self.aws_bedrock_host_settings()
            .map(|settings| settings.enablement_setting.clone())
            .unwrap_or_default()
    }

    pub fn is_aws_bedrock_credentials_toggleable(&self) -> bool {
        matches!(
            self.aws_bedrock_host_enablement_setting(),
            HostEnablementSetting::RespectUserSetting
        )
    }

    pub fn is_aws_bedrock_credentials_enabled(&self, app: &AppContext) -> bool {
        // i.e. did the admin go and toggle on aws bedrock in the admin panel?
        if !self.is_aws_bedrock_available_from_workspace() {
            return false;
        }

        match self.aws_bedrock_host_enablement_setting() {
            HostEnablementSetting::Enforce => true,
            HostEnablementSetting::RespectUserSetting => *AISettings::as_ref(app)
                .aws_bedrock_credentials_enabled
                .value(),
        }
    }

    pub fn gemini_enterprise_host_settings(&self) -> Option<&super::workspace::LlmHostSettings> {
        self.current_workspace().and_then(|workspace| {
            workspace
                .settings
                .llm_settings
                .host_configs
                .get(&LLMModelHost::GeminiEnterprise)
        })
    }

    /// Did the admin enable Gemini Enterprise (GEAP) for the current workspace?
    pub fn is_gemini_enterprise_available_from_workspace(&self) -> bool {
        self.current_workspace().is_some_and(|workspace| {
            workspace.settings.llm_settings.enabled
                && self
                    .gemini_enterprise_host_settings()
                    .is_some_and(|settings| settings.enabled)
        })
    }

    pub fn gemini_enterprise_host_enablement_setting(&self) -> HostEnablementSetting {
        self.gemini_enterprise_host_settings()
            .map(|settings| settings.enablement_setting.clone())
            .unwrap_or_default()
    }

    pub fn is_gemini_enterprise_credentials_toggleable(&self) -> bool {
        matches!(
            self.gemini_enterprise_host_enablement_setting(),
            HostEnablementSetting::RespectUserSetting
        )
    }

    /// Whether Gemini Enterprise (GEAP) credentials should be minted and attached for the
    /// current user. Anonymous/logged-out guard from [`Self::is_byo_api_key_enabled`]:
    /// a GEAP credential mint is rooted in the user's Warp session, so without one
    /// there is nothing to mint from.
    pub fn is_gemini_enterprise_credentials_enabled(&self, app: &AppContext) -> bool {
        if !FeatureFlag::GeminiEnterprise.is_enabled() {
            return false;
        }
        if AuthStateProvider::as_ref(app)
            .get()
            .is_anonymous_or_logged_out()
        {
            return false;
        }
        // i.e. did the admin toggle on Gemini Enterprise in the admin panel?
        if !self.is_gemini_enterprise_available_from_workspace() {
            return false;
        }

        match self.gemini_enterprise_host_enablement_setting() {
            HostEnablementSetting::Enforce => true,
            HostEnablementSetting::RespectUserSetting => *AISettings::as_ref(app)
                .gemini_enterprise_credentials_enabled
                .value(),
        }
    }

    /// Returns the AI autonomy settings that are enforced by the workspace for all its members.
    /// If a setting is `None`, the workspace doesn't enforce a particular setting.
    pub fn ai_autonomy_settings(&self) -> AiAutonomySettings {
        self.current_workspace()
            .map(|workspace| workspace.settings.ai_autonomy_settings.clone())
            .unwrap_or_default()
    }

    /// Returns the sandboxed agent settings enforced by the workspace, if any.
    pub fn sandboxed_agent_settings(&self) -> Option<SandboxedAgentSettings> {
        self.current_workspace()
            .and_then(|workspace| workspace.settings.sandboxed_agent_settings.clone())
    }

    /// Returns true iff AI autonomy features are allowed for this client.
    /// TODO: This should be deleted soon. AI autonomy settings have been moved into organization
    /// settings (see `ai_autonomy_settings` above), but there could be an interim time where we
    /// have not set up the org settings yet for an enterprise that previously had the entire
    /// feature set disabled. To capture that case, we'll see if all the settings are `None`;
    /// if so, we'll fall back to their billing metadata's value. Once we've migrated everyone
    /// into org settings, we should remove `is_enabled` from the policy and delete this function.
    pub fn is_ai_autonomy_allowed(&self) -> bool {
        self.current_workspace().is_none_or(|workspace| {
            let settings = &workspace.settings.ai_autonomy_settings;
            let all_settings_none = settings.apply_code_diffs_setting.is_none()
                && settings.read_files_setting.is_none()
                && settings.read_files_allowlist.is_none()
                && settings.execute_commands_setting.is_none()
                && settings.execute_commands_allowlist.is_none()
                && settings.execute_commands_denylist.is_none();

            if all_settings_none {
                workspace
                    .billing_metadata
                    .tier
                    .ai_autonomy_policy
                    .is_some_and(|policy| policy.is_enabled)
            } else {
                true
            }
        })
    }

    pub fn spaces_for_window(&self, window_id: WindowId, ctx: &AppContext) -> Vec<Space> {
        if AuthStateProvider::as_ref(ctx)
            .get()
            .is_user_web_anonymous_user()
            .unwrap_or_default()
        {
            return vec![Space::Shared];
        }
        let mut spaces = vec![];
        if let Some(team) = self.team_for_window(window_id) {
            spaces.push(Space::Team { team_uid: team.uid });
        }
        spaces.push(Space::Personal);

        spaces
    }

    // Returns the [`Owner`] for the user's personal drive. If the user is not authenticated, this
    // returns `None`.
    pub fn personal_drive(&self, ctx: &AppContext) -> Option<Owner> {
        AuthStateProvider::as_ref(ctx)
            .get()
            .user_id()
            .map(|user_uid| Owner::User { user_uid })
    }

    // Maps a [`Space`] into an [`Owner`], based on the user's team memberships. If the space
    // does not directly identify an owner (it's the space for shared objects), returns `None`.
    pub fn space_to_owner(&self, space: Space, ctx: &AppContext) -> Option<Owner> {
        match space {
            Space::Team { team_uid } => Some(Owner::Team { team_uid }),
            Space::Personal => self.personal_drive(ctx),
            Space::Shared => None,
        }
    }

    // Maps an [`Owner`] into a [`Space`], based on the user's team memberships.
    // This is always possible, as unknown owners imply the shared space.
    pub fn owner_to_space(&self, owner: Owner, _ctx: &AppContext) -> Space {
        match owner {
            Owner::User { .. } => Space::Personal,
            Owner::Team { team_uid } => Space::Team { team_uid },
        }
    }

    #[cfg(any(test, feature = "integration_tests"))]
    pub fn has_teams(&self) -> bool {
        if let Some(workspace) = self.current_workspace() {
            !workspace.teams.is_empty()
        } else {
            false
        }
    }

    #[cfg(any(test, feature = "integration_tests"))]
    pub fn update_workspaces(&mut self, workspaces: Vec<Workspace>, ctx: &mut ModelContext<Self>) {
        *self.workspaces = workspaces;
        let reassigned_windows = self.reconcile_window_team_assignments();
        self.notify_and_emit_teams_changed(ctx);
        Self::emit_window_team_changed(reassigned_windows, ctx);
    }

    fn notify_and_emit_teams_changed(&self, ctx: &mut ModelContext<Self>) {
        // Update session-sharing enablement since it depends on what teams the user
        // is part of.

        // PrivacySettings can't observe UserWorkspaces for updates, as it's initialized too early in
        // the app initialization flow. So, we update it manually whenever teams data changes.
        PrivacySettings::handle(ctx).update(ctx, |settings, ctx| {
            settings.set_is_telemetry_force_enabled(self.is_telemetry_force_enabled());
            settings.set_enterprise_secret_redaction_settings(
                self.is_enterprise_secret_redaction_enabled(),
                self.get_enterprise_secret_redaction_regex_list(),
                ChangeEventReason::CloudSync,
                ctx,
            );
        });

        ctx.emit(UserWorkspacesEvent::TeamsChanged);
        ctx.emit(UserWorkspacesEvent::CodebaseContextEnablementChanged);
        ctx.notify();
    }

    pub fn is_telemetry_force_enabled(&self) -> bool {
        self.current_workspace()
            .map(|workspace| workspace.settings.telemetry_settings.force_enabled)
            .unwrap_or(false)
    }

    pub fn is_enterprise_secret_redaction_enabled(&self) -> bool {
        self.current_workspace()
            .map(|workspace| workspace.settings.secret_redaction_settings.enabled)
            .unwrap_or(false)
    }

    pub fn get_enterprise_secret_redaction_regex_list(&self) -> Vec<EnterpriseSecretRegex> {
        self.current_workspace()
            .map(|workspace| workspace.settings.secret_redaction_settings.regexes.clone())
            .unwrap_or_default()
    }

    pub fn get_ugc_collection_enablement_setting(&self) -> UgcCollectionEnablementSetting {
        self.current_workspace()
            .map(|workspace| workspace.settings.ugc_collection_settings.setting.clone())
            .unwrap_or_default()
    }

    pub fn get_cloud_conversation_storage_enablement_setting(&self) -> AdminEnablementSetting {
        self.current_workspace()
            .map(|workspace| {
                workspace
                    .settings
                    .cloud_conversation_storage_settings
                    .setting
                    .clone()
            })
            .unwrap_or_default()
    }

    pub fn is_ai_allowed_in_remote_sessions(&self) -> bool {
        self.current_workspace()
            .map(|workspace| {
                workspace
                    .settings
                    .ai_permissions_settings
                    .allow_ai_in_remote_sessions
            })
            .unwrap_or(true)
    }

    pub fn get_remote_session_regex_list(&self) -> Vec<Regex> {
        self.current_workspace()
            .map(|workspace| {
                workspace
                    .settings
                    .ai_permissions_settings
                    .remote_session_regex_list
                    .clone()
            })
            .unwrap_or_default()
    }

    /// Returns the codebase context settings, taking into account the organization, /// global AI settings, and codebase-specific settings.
    /// Prefer this function to determine whether to show indexing-related functionality.
    pub fn is_codebase_context_enabled(&self, app: &AppContext) -> bool {
        // If the organization has an explicit setting, respect it and make user toggle irrelevant.
        // - Enable: forced ON by org, regardless of user preference.
        // - Disable: forced OFF by org.
        // - RespectUserSetting: respect the user setting.
        let org_setting = self.team_allows_codebase_context();
        let ai_globally_enabled = AISettings::as_ref(app).is_any_ai_enabled(app);

        match org_setting {
            AdminEnablementSetting::Enable => ai_globally_enabled,
            AdminEnablementSetting::Disable => false,
            AdminEnablementSetting::RespectUserSetting => {
                ai_globally_enabled && *CodeSettings::as_ref(app).codebase_context_enabled.value()
            }
        }
    }

    pub fn default_host_slug(&self) -> Option<&str> {
        self.current_workspace()
            .and_then(|workspace| workspace.settings.default_host_slug.as_deref())
    }

    /// Returns the team-level agent attribution setting.
    ///
    /// Use this to decide whether the user's attribution toggle should be locked
    /// (`Enable`/`Disable`) or editable (`RespectUserSetting`).
    pub fn get_agent_attribution_setting(&self) -> AdminEnablementSetting {
        self.current_workspace()
            .map(|workspace| workspace.settings.enable_warp_attribution.clone())
            .unwrap_or_default()
    }

    /// Returns only the organization-specific codebase context enablement setting.
    /// Do not use this function to determine whether codebase context is generally enabled --
    /// use `is_codebase_context_enabled` instead.
    pub fn team_allows_codebase_context(&self) -> AdminEnablementSetting {
        self.current_workspace()
            .map(|workspace| workspace.settings.codebase_context_settings.setting.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
impl UserWorkspaces {
    /// Creates a test workspace with a team and sets it as the current workspace.
    /// Returns the workspace UID and admin UID for use in tests.
    pub fn setup_test_workspace(&mut self, ctx: &mut ModelContext<Self>) {
        let workspace_uid = WorkspaceUid::from(ServerId::from(1));
        let owner_uid = UserUid::new("test_owner");

        let workspace_settings = WorkspaceSettings::default();

        let workspace = Workspace {
            uid: workspace_uid,
            name: "Test Workspace".to_string(),
            stripe_customer_id: None,
            teams: vec![Team {
                uid: ServerId::from(2),
                name: "Test Team".to_string(),
                settings: Default::default(),
                color: None,
                billing_metadata: BillingMetadata::default(),
                members: vec![],
                invite_link: None,
                pending_email_invites: vec![],
                invite_link_domain_restrictions: vec![],
                stripe_customer_id: None,
                is_eligible_for_discovery: false,
                has_billing_history: false,
                visibility: TeamVisibility::Open,
            }],
            members: vec![WorkspaceMember {
                uid: owner_uid,
                email: "test@example.com".to_string(),
                role: MembershipRole::Owner,
                usage_info: WorkspaceMemberUsageInfo {
                    requests_used_since_last_refresh: 0,
                    request_limit: 1000,
                    is_unlimited: false,
                    is_request_limit_prorated: false,
                },
            }],
            billing_metadata: BillingMetadata::default(),
            bonus_grants_purchased_this_month: Default::default(),
            billing_cycle_usage: None,
            has_billing_history: false,
            settings: workspace_settings,
            invite_link_domain_restrictions: vec![],
            pending_email_invites: vec![],
            is_eligible_for_discovery: false,
            total_requests_used_since_last_refresh: 0,
        };

        self.update_workspaces(vec![workspace], ctx);
        self.set_current_workspace_uid(workspace_uid, ctx);
    }

    /// Updates the current workspace by applying a mutation function.
    pub fn update_current_workspace<F>(&mut self, f: F, ctx: &mut ModelContext<Self>)
    where
        F: FnOnce(&mut Workspace),
    {
        if let Some(workspace) = self.current_workspace() {
            if workspace.teams.is_empty() {
                panic!("No team found in current workspace. Did you call setup_test_workspace()?");
            }

            let mut new_workspace = workspace.clone();
            f(&mut new_workspace);

            self.update_workspaces(vec![new_workspace], ctx);
        } else {
            panic!("No workspace found. Did you call setup_test_workspace()?");
        }
    }

    pub fn update_sandboxed_agent_settings<F>(&mut self, f: F, ctx: &mut ModelContext<Self>)
    where
        F: FnOnce(&mut Option<SandboxedAgentSettings>),
    {
        self.update_current_workspace(
            |workspace| {
                f(&mut workspace.settings.sandboxed_agent_settings);
            },
            ctx,
        );
    }

    pub fn update_ai_autonomy_settings<F>(&mut self, f: F, ctx: &mut ModelContext<Self>)
    where
        F: FnOnce(&mut AiAutonomySettings),
    {
        self.update_current_workspace(
            |workspace| {
                f(&mut workspace.settings.ai_autonomy_settings);
            },
            ctx,
        );
    }
}

impl Entity for UserWorkspaces {
    type Event = UserWorkspacesEvent;
}

/// Mark UserWorkspaces as global application state.
impl SingletonEntity for UserWorkspaces {}

#[cfg(test)]
#[path = "user_workspaces_tests.rs"]
mod user_workspaces_tests;
