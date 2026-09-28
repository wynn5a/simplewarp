use std::collections::HashMap;

use warp_core::settings::Setting as _;
use warpui::{
    AppContext, Entity, ModelContext, SingletonEntity, Tracked, ViewContext, WeakViewHandle,
    WindowId,
};

use super::team::Team;
use super::workspace::{Workspace, WorkspaceUid};
use crate::auth::AuthStateProvider;
use crate::cloud_object::{Owner, Space};
use crate::server::ids::ServerId;
use crate::settings::{AISettings, AISettingsChangedEvent, CodeSettings, CodeSettingsChangedEvent};

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub enum UserWorkspacesEvent {
    /// Fired whenever the set of teams the user is on changes.
    #[allow(dead_code)]
    TeamsChanged,
    /// Fired when the selected workspace actually changes to a different one.
    #[allow(dead_code)]
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
                if let CodeSettingsChangedEvent::CodebaseContextEnabled = code_settings_event {
                    ctx.emit(UserWorkspacesEvent::CodebaseContextEnablementChanged);
                }
            },
        );

        ctx.subscribe_to_model(&AISettings::handle(ctx), |_, _, ai_settings_event, ctx| {
            if let AISettingsChangedEvent::IsAnyAIEnabled = ai_settings_event {
                ctx.emit(UserWorkspacesEvent::CodebaseContextEnablementChanged);
            }
        });

        Self {
            current_workspace_uid: current_workspace_uid.into(),
            workspaces: cached_workspaces.into(),
            window_team_uids: Default::default(),
        }
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
    #[cfg(test)]
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

    #[cfg(test)]
    fn emit_window_team_changed(windows: Vec<WindowId>, ctx: &mut ModelContext<Self>) {
        for _ in windows {
            ctx.emit(UserWorkspacesEvent::WindowTeamChanged);
        }
    }

    pub fn workspace_from_uid(&self, workspace_uid: WorkspaceUid) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.uid == workspace_uid)
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

    #[cfg(test)]
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

    /// Whether members may use their own provider API keys. A workspace on the managed BYOK/BYOE
    /// policy only allows team-managed keys, and no team-managed key exists locally.
    pub fn are_member_byo_keys_allowed(&self) -> bool {
        self.current_workspace()
            .is_none_or(|workspace| !workspace.billing_metadata.is_managed_byok_byoe_enabled())
    }

    /// Whether custom inference endpoints are enabled for the current user.
    ///
    /// Always true, for the same reason as [`Self::is_byo_api_key_enabled`]: a custom endpoint is
    /// a first-class way to reach a model here, and the checks this used to make would switch it
    /// off for every user.
    pub fn is_custom_inference_enabled(&self, _app: &AppContext) -> bool {
        true
    }

    /// Whether members may use their own custom endpoints; see
    /// [`Self::are_member_byo_keys_allowed`].
    pub fn are_member_byo_endpoints_allowed(&self) -> bool {
        self.are_member_byo_keys_allowed()
    }

    /// Whether AWS Bedrock credentials are attached to agent requests. Bedrock is only available
    /// once a workspace admin enables it, which no local workspace can.
    pub fn is_aws_bedrock_credentials_enabled(&self) -> bool {
        false
    }

    /// Returns true iff AI autonomy features are allowed for this client by the workspace's
    /// billing policy.
    pub fn is_ai_autonomy_allowed(&self) -> bool {
        self.current_workspace().is_none_or(|workspace| {
            workspace
                .billing_metadata
                .tier
                .ai_autonomy_policy
                .is_some_and(|policy| policy.is_enabled)
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

    #[cfg(test)]
    pub fn has_teams(&self) -> bool {
        if let Some(workspace) = self.current_workspace() {
            !workspace.teams.is_empty()
        } else {
            false
        }
    }

    #[cfg(test)]
    pub fn update_workspaces(&mut self, workspaces: Vec<Workspace>, ctx: &mut ModelContext<Self>) {
        *self.workspaces = workspaces;
        let reassigned_windows = self.reconcile_window_team_assignments();
        self.notify_and_emit_teams_changed(ctx);
        Self::emit_window_team_changed(reassigned_windows, ctx);
    }

    #[cfg(test)]
    fn notify_and_emit_teams_changed(&self, ctx: &mut ModelContext<Self>) {
        ctx.emit(UserWorkspacesEvent::TeamsChanged);
        ctx.emit(UserWorkspacesEvent::CodebaseContextEnablementChanged);
        ctx.notify();
    }

    /// Whether codebase context is enabled, from the global AI and codebase-specific settings.
    /// Prefer this function to determine whether to show indexing-related functionality.
    pub fn is_codebase_context_enabled(&self, app: &AppContext) -> bool {
        AISettings::as_ref(app).is_any_ai_enabled()
            && *CodeSettings::as_ref(app).codebase_context_enabled.value()
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
