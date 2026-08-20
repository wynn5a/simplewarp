use std::convert::TryFrom;

use anyhow::anyhow;
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use warp_graphql::queries::get_blocks_for_user::Block as GqlBlock;

use super::ServerApi;
use crate::ai::generate_block_title::api::{GenerateBlockTitleRequest, GenerateBlockTitleResponse};
use crate::server::block::{Block, DisplaySetting};

#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait BlockClient: 'static + Send + Sync {
    /// Unshares a block identified at `block_id`.
    async fn unshare_block(&self, block_id: String) -> Result<(), anyhow::Error>;

    /// Uploads a given block to the server via the /share_block endpoint.
    async fn save_block(
        &self,
        block: &Block,
        title: Option<String>,
        show_prompt: bool,
        display_setting: DisplaySetting,
    ) -> Result<String, anyhow::Error>;

    async fn blocks_owned_by_user(&self) -> Result<Vec<Block>, anyhow::Error>;

    async fn generate_shared_block_title(
        &self,
        request: GenerateBlockTitleRequest,
    ) -> Result<GenerateBlockTitleResponse, anyhow::Error>;
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl BlockClient for ServerApi {
    async fn unshare_block(&self, _block_uid: String) -> Result<(), anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn save_block(
        &self,
        _block: &Block,
        _title: Option<String>,
        _show_prompt: bool,
        _display_setting: DisplaySetting,
    ) -> Result<String, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn blocks_owned_by_user(&self) -> Result<Vec<Block>, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn generate_shared_block_title(
        &self,
        _request: GenerateBlockTitleRequest,
    ) -> Result<GenerateBlockTitleResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }
}

impl TryFrom<GqlBlock> for Block {
    type Error = anyhow::Error;

    fn try_from(value: GqlBlock) -> Result<Self, Self::Error> {
        match (value.uid, value.time_started_term) {
            (uid, Some(time_started_term)) => {
                Ok(Block {
                    id: Some(uid.into_inner()),
                    command: value.command,
                    output: None,
                    stylized_command: None,
                    stylized_output: None,
                    pwd: None,
                    time_started_term: time_started_term.utc().into(),
                    // This is a dummy value - we are no longer using time_completed_term,
                    // and GqlBlock does not have a time_completed_term field.
                    time_completed_term: time_started_term.utc().into(),
                    stylized_prompt: None,
                    stylized_prompt_and_command: None,
                })
            }
            _ => Err(anyhow!("missing id or time_started_term")),
        }
    }
}
