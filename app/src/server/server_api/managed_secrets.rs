use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use warp_graphql::managed_secrets::{ManagedSecret, ManagedSecretType};
use warp_graphql::queries::task_secrets::ManagedSecretValue;
pub use warp_managed_secrets::client::{ManagedSecretConfigs, ManagedSecretsClient};
use warp_managed_secrets::client::{SecretOwner, TaskIdentityToken};

use super::ServerApi;

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl ManagedSecretsClient for ServerApi {
    async fn get_managed_secret_configs(&self) -> Result<ManagedSecretConfigs> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn create_managed_secret(
        &self,
        _owner: SecretOwner,
        _name: String,
        _secret_type: ManagedSecretType,
        _encrypted_value: String,
        _description: Option<String>,
    ) -> Result<ManagedSecret> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn delete_managed_secret(&self, _owner: SecretOwner, _name: String) -> Result<()> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn update_managed_secret(
        &self,
        _owner: SecretOwner,
        _name: String,
        _encrypted_value: Option<String>,
        _description: Option<String>,
    ) -> Result<ManagedSecret> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_harness_auth_secrets(
        &self,
        _harness: warp_graphql::ai::AgentHarness,
    ) -> Result<Vec<ManagedSecret>> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_secrets(&self) -> Result<Vec<ManagedSecret>> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_task_secrets(
        &self,
        _task_id: String,
        _workload_token: String,
    ) -> Result<HashMap<String, ManagedSecretValue>> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn issue_task_identity_token(
        &self,
        _options: warp_managed_secrets::client::IdentityTokenOptions,
    ) -> Result<TaskIdentityToken> {
        Err(crate::server::server_api::local_only_error())
    }
}
