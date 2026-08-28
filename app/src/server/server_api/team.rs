use anyhow::Result;
use async_trait::async_trait;
#[cfg(test)]
use mockall::{automock, predicate::*};

use super::ServerApi;
use crate::auth::UserUid;
use crate::cloud_object::CloudObjectEventEntrypoint;
use crate::server::ids::ServerId;
use crate::workspaces::team::MembershipRole;
use crate::workspaces::user_workspaces::{CreateTeamResponse, WorkspacesMetadataWithPricing};

#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait TeamClient: 'static + Send + Sync {
    async fn workspaces_metadata(&self) -> Result<WorkspacesMetadataWithPricing>;

    async fn add_invite_link_domain_restriction(
        &self,
        team_uid: ServerId,
        domain: String,
    ) -> Result<WorkspacesMetadataWithPricing>;

    async fn delete_invite_link_domain_restriction(
        &self,
        team_uid: ServerId,
        domain_uid: ServerId,
    ) -> Result<WorkspacesMetadataWithPricing>;

    /// Creates a team and returns the result from the server with the newly created team.
    async fn create_team(
        &self,
        name: String,
        entrypoint: CloudObjectEventEntrypoint,
        discoverable: Option<bool>,
    ) -> Result<CreateTeamResponse>;

    /// Removes the user from the selected team and returns a list of all teams that a user is
    /// still a member of (including updated team members).
    async fn remove_user_from_team(
        &self,
        user_uid: UserUid,
        team_uid: ServerId,
        entrypoint: CloudObjectEventEntrypoint,
    ) -> Result<WorkspacesMetadataWithPricing>;

    /// Removes the _current_ user from the team (user leaving the team) and returns the list of
    /// all teams that the current user is still a member of.
    async fn leave_team(
        &self,
        user_uid: UserUid,
        team_uid: ServerId,
        entrypoint: CloudObjectEventEntrypoint,
    ) -> Result<WorkspacesMetadataWithPricing>;

    async fn send_team_invite_email(
        &self,
        team_uid: ServerId,
        email: String,
    ) -> Result<WorkspacesMetadataWithPricing>;

    async fn delete_team_invite(
        &self,
        team_uid: ServerId,
        email: String,
    ) -> Result<WorkspacesMetadataWithPricing>;

    async fn rename_team(
        &self,
        new_name: String,
        team_uid: ServerId,
    ) -> Result<WorkspacesMetadataWithPricing>;

    async fn reset_invite_links(&self, team_uid: ServerId)
    -> Result<WorkspacesMetadataWithPricing>;

    async fn set_is_invite_link_enabled(
        &self,
        team_uid: ServerId,
        new_value: bool,
    ) -> Result<WorkspacesMetadataWithPricing>;

    async fn set_team_discoverability(
        &self,
        team_uid: ServerId,
        discoverable: bool,
    ) -> Result<WorkspacesMetadataWithPricing>;

    async fn transfer_team_ownership(
        &self,
        new_owner_email: String,
    ) -> Result<WorkspacesMetadataWithPricing>;

    async fn set_team_member_role(
        &self,
        user_uid: UserUid,
        team_uid: ServerId,
        role: MembershipRole,
    ) -> Result<WorkspacesMetadataWithPricing>;
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl TeamClient for ServerApi {
    #[tracing::instrument(skip_all, err, fields(tags.cloud_agent = true))]
    async fn workspaces_metadata(&self) -> Result<WorkspacesMetadataWithPricing> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn add_invite_link_domain_restriction(
        &self,
        _team_uid: ServerId,
        _domain: String,
    ) -> Result<WorkspacesMetadataWithPricing> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn delete_invite_link_domain_restriction(
        &self,
        _team_uid: ServerId,
        _domain_uid: ServerId,
    ) -> Result<WorkspacesMetadataWithPricing> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn create_team(
        &self,
        _name: String,
        _entrypoint: CloudObjectEventEntrypoint,
        _discoverable: Option<bool>,
    ) -> Result<CreateTeamResponse> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn remove_user_from_team(
        &self,
        _user_uid: UserUid,
        _team_uid: ServerId,
        _entrypoint: CloudObjectEventEntrypoint,
    ) -> Result<WorkspacesMetadataWithPricing> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn leave_team(
        &self,
        _user_uid: UserUid,
        _team_uid: ServerId,
        _entrypoint: CloudObjectEventEntrypoint,
    ) -> Result<WorkspacesMetadataWithPricing> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn send_team_invite_email(
        &self,
        _team_uid: ServerId,
        _email: String,
    ) -> Result<WorkspacesMetadataWithPricing> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn delete_team_invite(
        &self,
        _team_uid: ServerId,
        _email: String,
    ) -> Result<WorkspacesMetadataWithPricing> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn rename_team(
        &self,
        _new_name: String,
        _team_uid: ServerId,
    ) -> Result<WorkspacesMetadataWithPricing> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn reset_invite_links(
        &self,
        _team_uid: ServerId,
    ) -> Result<WorkspacesMetadataWithPricing> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn set_is_invite_link_enabled(
        &self,
        _team_uid: ServerId,
        _new_value: bool,
    ) -> Result<WorkspacesMetadataWithPricing> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn set_team_discoverability(
        &self,
        _team_uid: ServerId,
        _new_value: bool,
    ) -> Result<WorkspacesMetadataWithPricing> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn transfer_team_ownership(
        &self,
        _new_owner_email: String,
    ) -> Result<WorkspacesMetadataWithPricing> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn set_team_member_role(
        &self,
        _user_uid: UserUid,
        _team_uid: ServerId,
        _role: MembershipRole,
    ) -> Result<WorkspacesMetadataWithPricing> {
        Err(crate::server::server_api::local_only_error())
    }
}
