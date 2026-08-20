use anyhow::Result;
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use warp_graphql::mutations::upsert_runner::UpsertRunnerInput;
use warp_graphql::queries::get_runners::{Runner, RunnerSortBy};

use super::ServerApi;

/// The result of upserting a runner: the resulting [`Runner`] plus whether the
/// operation updated an existing runner (vs. creating a new one).
// `upsert_runner`/`delete_runner` back CLI commands that aren't built for wasm, so
// this type is unused there while `get_runners` still powers the runner picker.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
pub struct UpsertedRunner {
    pub runner: Runner,
    pub is_update: bool,
}

/// Client for the Factory GraphQL surface (runner CRUD).
#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait FactoryClient: 'static + Send + Sync {
    /// Fetch all runners visible to the caller, optionally sorted.
    async fn get_runners(&self, sort_by: Option<RunnerSortBy>) -> Result<Vec<Runner>>;

    /// Create or update a runner. `input.uid` is `None` for a create and
    /// `Some(_)` for an update; this single method backs both CLI commands.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    async fn upsert_runner(&self, input: UpsertRunnerInput) -> Result<UpsertedRunner>;

    /// Delete a runner by UID, returning the deleted UID on success.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    async fn delete_runner(&self, uid: String) -> Result<String>;
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl FactoryClient for ServerApi {
    async fn get_runners(&self, _sort_by: Option<RunnerSortBy>) -> Result<Vec<Runner>> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn upsert_runner(&self, _input: UpsertRunnerInput) -> Result<UpsertedRunner> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn delete_runner(&self, _uid: String) -> Result<String> {
        Err(crate::server::server_api::local_only_error())
    }
}
