use anyhow::Result;
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use warp_graphql::queries::get_oauth_connect_tx_status::OauthConnectTxStatus;
use warp_graphql::queries::suggest_cloud_environment_image::SuggestCloudEnvironmentImageResult;
use warp_graphql::queries::user_github_info::UserGithubInfoResult;
use warp_graphql::queries::user_repo_auth_status::UserRepoAuthStatusOutput;

use super::ServerApi;

#[cfg(not(target_family = "wasm"))]
pub trait IntegrationsClientBounds: Send + Sync {}

#[cfg(not(target_family = "wasm"))]
impl<T: 'static + Send + Sync> IntegrationsClientBounds for T {}

#[cfg(target_family = "wasm")]
pub trait IntegrationsClientBounds {}

#[cfg(target_family = "wasm")]
impl<T: 'static> IntegrationsClientBounds for T {}

#[cfg_attr(test, automock)]
#[cfg_attr(target_family = "wasm", allow(dead_code))]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
pub trait IntegrationsClient: 'static + IntegrationsClientBounds {
    /// Checks the user's GitHub authorization status for the given repositories.
    ///
    /// Returns a list of statuses for each repo, indicating whether the user has
    /// access to the repo, and an optional auth URL for the user to authorize.
    async fn check_user_repo_auth_status(
        &self,
        repos: Vec<(String, String)>,
    ) -> Result<UserRepoAuthStatusOutput>;

    /// Polls the status of an OAuth connect transaction.
    ///
    /// # Arguments
    /// * `tx_id` - The transaction ID returned from create_simple_integration
    ///
    /// # Returns
    /// * `Ok(OauthConnectTxStatus)` - The current status of the transaction
    /// * `Err` - If the transaction is not found or polling fails
    async fn poll_oauth_connect_status(&self, tx_id: String) -> Result<OauthConnectTxStatus>;

    /// Gets the user's GitHub connection info, including accessible repos.
    ///
    /// # Returns
    /// * `Ok(UserGithubInfoResult)` - Either connected with repos, or auth required
    /// * `Err` - If the query fails
    async fn get_user_github_info(&self) -> Result<UserGithubInfoResult>;

    /// Suggests a Docker image for a cloud environment based on the provided repos.
    async fn suggest_cloud_environment_image(
        &self,
        repos: Vec<(String, String)>,
    ) -> Result<SuggestCloudEnvironmentImageResult>;
}

#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
impl IntegrationsClient for ServerApi {
    async fn check_user_repo_auth_status(
        &self,
        _repos: Vec<(String, String)>,
    ) -> Result<UserRepoAuthStatusOutput> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn poll_oauth_connect_status(&self, _tx_id: String) -> Result<OauthConnectTxStatus> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_user_github_info(&self) -> Result<UserGithubInfoResult> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn suggest_cloud_environment_image(
        &self,
        _repos: Vec<(String, String)>,
    ) -> Result<SuggestCloudEnvironmentImageResult> {
        Err(crate::server::server_api::local_only_error())
    }
}
