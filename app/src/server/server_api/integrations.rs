use anyhow::Result;
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use warp_graphql::mutations::create_simple_integration::CreateSimpleIntegrationOutput;
use warp_graphql::queries::get_integrations_using_environment::GetIntegrationsUsingEnvironmentOutput;
use warp_graphql::queries::get_oauth_connect_tx_status::OauthConnectTxStatus;
use warp_graphql::queries::get_simple_integrations::SimpleIntegrationsOutput;
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

    /// Creates or updates a simple integration on the server.
    ///
    /// # Arguments
    /// * `integration_type` - The type of integration (e.g. "github", "linear", "slack")
    /// * `is_update` - Whether this is an update to an existing integration
    /// * `environment_uid` - The UID of the environment to associate with this integration
    /// * `base_prompt` - Optional base prompt for the integration
    /// * `model_id` - Optional model ID for the integration
    /// * `mcp_servers_json` - Optional JSON string encoding a map[string]MCPServerConfig (ambient agent spec)
    /// * `remove_mcp_server_names` - Optional list of MCP server names to remove (applies on update)
    /// * `worker_host` - Optional worker host ID for self-hosted workers
    /// * `enabled` - Whether the integration should be enabled on creation
    #[allow(clippy::too_many_arguments)]
    async fn create_or_update_simple_integration(
        &self,
        integration_type: String,
        is_update: bool,
        environment_uid: Option<String>,
        base_prompt: Option<String>,
        model_id: Option<String>,
        mcp_servers_json: Option<String>,
        remove_mcp_server_names: Option<Vec<String>>,
        worker_host: Option<String>,
        enabled: bool,
    ) -> Result<CreateSimpleIntegrationOutput>;

    /// Lists simple integrations for a fixed set of provider slugs.
    ///
    /// The server will return one SimpleIntegration entry per requested provider,
    /// regardless of whether the connection or integration currently exists.
    async fn list_simple_integrations(
        &self,
        providers: Vec<String>,
    ) -> Result<SimpleIntegrationsOutput>;

    /// Polls the status of an OAuth connect transaction.
    ///
    /// # Arguments
    /// * `tx_id` - The transaction ID returned from create_simple_integration
    ///
    /// # Returns
    /// * `Ok(OauthConnectTxStatus)` - The current status of the transaction
    /// * `Err` - If the transaction is not found or polling fails
    async fn poll_oauth_connect_status(&self, tx_id: String) -> Result<OauthConnectTxStatus>;

    /// Gets the list of integration provider names that are using the specified environment.
    ///
    /// # Arguments
    /// * `environment_id` - The ID of the environment to check
    ///
    /// # Returns
    /// * `Ok(Vec<String>)` - List of provider names (e.g., ["linear", "slack"]) using this environment
    /// * `Err` - If the query fails
    async fn get_integrations_using_environment(
        &self,
        environment_id: String,
    ) -> Result<GetIntegrationsUsingEnvironmentOutput>;

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

    #[allow(clippy::too_many_arguments)]
    async fn create_or_update_simple_integration(
        &self,
        _integration_type: String,
        _is_update: bool,
        _environment_uid: Option<String>,
        _base_prompt: Option<String>,
        _model_id: Option<String>,
        _mcp_servers_json: Option<String>,
        _remove_mcp_server_names: Option<Vec<String>>,
        _worker_host: Option<String>,
        _enabled: bool,
    ) -> Result<CreateSimpleIntegrationOutput> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_integrations_using_environment(
        &self,
        _environment_id: String,
    ) -> Result<GetIntegrationsUsingEnvironmentOutput> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_simple_integrations(
        &self,
        _providers: Vec<String>,
    ) -> Result<SimpleIntegrationsOutput> {
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
