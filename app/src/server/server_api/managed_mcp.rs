// We don't resolve managed MCPs from agent run CLI flows on WASM, so this code is unused there.
#![cfg_attr(target_family = "wasm", expect(dead_code))]

use anyhow::Result;
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use warp_graphql::mutations::create_managed_mcp_client_config::CreateManagedMcpClientConfigOutput;

use super::ServerApi;

#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait ManagedMcpClient: 'static + Send + Sync {
    /// `uid` is a managed MCP server UUID or a well-known integration id
    /// (e.g. "linear") — the GraphQL input is an opaque `ID!`.
    async fn create_managed_mcp_client_config(
        &self,
        uid: String,
    ) -> Result<CreateManagedMcpClientConfigOutput>;
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl ManagedMcpClient for ServerApi {
    async fn create_managed_mcp_client_config(
        &self,
        _uid: String,
    ) -> Result<CreateManagedMcpClientConfigOutput> {
        Err(crate::server::server_api::local_only_error())
    }
}
