use std::sync::Arc;

use anyhow::Context as _;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use warp_errors::report_error;
use warp_graphql::scalars::time::ServerTimestamp;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use crate::BlocklistAIHistoryModel;
use crate::ai::agent::AIAgentExchangeId;
use crate::ai::agent::conversation::AIConversationId;
use crate::server::server_api::ai::AIClient;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum RequestLimitRefreshDuration {
    Weekly,
    Monthly,
    EveryTwoWeeks,
}

/// The current rate limit info for the user.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct RequestLimitInfo {
    pub limit: usize,
    pub num_requests_used_since_refresh: usize,
    pub next_refresh_time: ServerTimestamp,
    pub is_unlimited: bool,
    pub request_limit_refresh_duration: RequestLimitRefreshDuration,
    pub is_unlimited_voice: bool,
    #[serde(default)]
    pub voice_request_limit: usize,
    #[serde(default)]
    pub voice_requests_used_since_last_refresh: usize,
    #[serde(default)]
    pub is_unlimited_codebase_indices: bool,
    #[serde(default)]
    pub max_codebase_indices: usize,
    #[serde(default)]
    pub max_files_per_repo: usize,
    #[serde(default)]
    pub embedding_generation_batch_size: usize,
}

fn default_voice_requests_limit() -> usize {
    10000
}

impl Default for RequestLimitInfo {
    /// This is the default rate limit for the free tier imposed by the server as of 02/10/25.
    fn default() -> Self {
        Self {
            limit: 150,
            num_requests_used_since_refresh: 0,
            next_refresh_time: ServerTimestamp::new(Utc::now() + chrono::Duration::days(30)),
            is_unlimited: false,
            request_limit_refresh_duration: RequestLimitRefreshDuration::Monthly,
            is_unlimited_voice: false,
            voice_request_limit: default_voice_requests_limit(),
            voice_requests_used_since_last_refresh: 0,
            is_unlimited_codebase_indices: false,
            max_codebase_indices: 3,
            max_files_per_repo: 5000,
            embedding_generation_batch_size: 100,
        }
    }
}

#[cfg(test)]
impl RequestLimitInfo {
    pub fn new_for_test(limit: usize, num_requests_used_since_refresh: usize) -> Self {
        Self {
            limit,
            num_requests_used_since_refresh,
            ..Self::default()
        }
    }
}

pub struct CodebaseContextUsageLimit {
    pub max_files_per_repo: usize,
    pub max_indices_allowed: Option<usize>,
    pub embedding_generation_batch_size: usize,
}

#[cfg(feature = "agent_mode_evals")]
impl RequestLimitInfo {
    pub fn new_for_evals() -> Self {
        Self {
            limit: 999999,
            num_requests_used_since_refresh: 0,
            next_refresh_time: ServerTimestamp::new(Utc::now() + chrono::Duration::days(30)),
            is_unlimited: true,
            request_limit_refresh_duration: RequestLimitRefreshDuration::Monthly,
            is_unlimited_voice: true,
            voice_request_limit: 999999,
            voice_requests_used_since_last_refresh: 0,
            is_unlimited_codebase_indices: false,
            max_codebase_indices: 40,
            max_files_per_repo: 10000,
            embedding_generation_batch_size: 100,
        }
    }
}

pub struct AIRequestUsageModel {
    ai_client: Arc<dyn AIClient>,

    request_limit_info: RequestLimitInfo,
}

impl Entity for AIRequestUsageModel {
    type Event = AIRequestUsageModelEvent;
}

pub enum AIRequestUsageModelEvent {
    RequestBonusRefunded {
        requests_refunded: i32,
        server_conversation_id: String,
        request_id: String,
    },
}

impl AIRequestUsageModel {
    pub fn new(ai_client: Arc<dyn AIClient>) -> Self {
        Self {
            ai_client,
            request_limit_info: RequestLimitInfo::default(),
        }
    }

    #[cfg(test)]
    pub fn new_for_test(ai_client: Arc<dyn AIClient>) -> Self {
        Self {
            ai_client,
            request_limit_info: RequestLimitInfo::default(),
        }
    }

    pub fn provide_negative_feedback_response_for_ai_conversation(
        &mut self,
        client_conversation_id: AIConversationId,
        request_id: String,
        client_exchange_id: AIAgentExchangeId,
        ctx: &mut ModelContext<Self>,
    ) {
        let server_conversation_id = BlocklistAIHistoryModel::as_ref(ctx)
            .conversation(&client_conversation_id)
            .and_then(|conversation| conversation.server_conversation_token());

        let Some(server_conversation_id) = server_conversation_id else {
            return;
        };
        let server_conversation_id_string = server_conversation_id.as_str().to_string();
        let server_conversation_id_string_clone = server_conversation_id_string.clone();

        let request_ids = BlocklistAIHistoryModel::as_ref(ctx)
            .conversation(&client_conversation_id)
            .map(|conversation| {
                let mut request_ids = vec![];

                let target_exchange = conversation
                    .root_task_exchanges()
                    .find(|exchange| exchange.id == client_exchange_id);

                let mut found_target = false;

                for exchange in conversation.exchanges_reversed() {
                    if let Some(target_exchange) = target_exchange {
                        if exchange.id == target_exchange.id {
                            found_target = true;
                        }
                    } else {
                        break;
                    }

                    if found_target {
                        if let Some(server_output_id) = exchange.output_status.server_output_id() {
                            request_ids.push(server_output_id.to_string());
                        }

                        if exchange
                            .input
                            .iter()
                            .any(|input| input.display_query().is_some())
                        {
                            break;
                        }
                    }
                }

                request_ids
            })
            .unwrap_or_default();

        // No reason to refund if there are no request ids.
        if request_ids.is_empty() {
            return;
        }

        let ai_client = self.ai_client.clone();
        ctx.spawn(
            async move {
                ai_client
                    .provide_negative_feedback_response_for_ai_conversation(
                        server_conversation_id_string_clone,
                        request_ids,
                    )
                    .await
            },
            |_, result, ctx| match result
                .context("Failed to provide negative feedback response for ai conversation")
            {
                Ok(requests_refunded) => {
                    if requests_refunded > 0 {
                        ctx.emit(AIRequestUsageModelEvent::RequestBonusRefunded {
                            requests_refunded,
                            server_conversation_id: server_conversation_id_string,
                            request_id,
                        });
                    }
                }
                Err(e) => {
                    report_error!(e);
                }
            },
        );
    }

    /// Returns the number of remaining requests the user has based on their latest rate limit info.
    /// If the current time is past the next refresh time, then the number of remaining reqs is the limit.
    fn requests_remaining(&self) -> usize {
        if self.request_limit_info.next_refresh_time.utc() <= Utc::now()
            || self.request_limit_info.is_unlimited
        {
            self.request_limit_info.limit
        } else {
            self.request_limit_info
                .limit
                .saturating_sub(self.request_limit_info.num_requests_used_since_refresh)
        }
    }

    /// Whether unused base-plan request quota remains.
    pub(crate) fn has_base_plan_requests_remaining(&self) -> bool {
        self.requests_remaining() > 0
    }

    /// SimpleWarp does not meter AI requests, so there is always AI remaining.
    ///
    /// The gate this replaces asked warp-server whether the account had credit
    /// left. This fork has no account and no meter, so every request is allowed
    /// and nothing is counted against a quota.
    pub fn has_any_ai_remaining(&self, _ctx: &AppContext) -> bool {
        true
    }

    /// Returns the number of indices the user's tier allows them to create and the number of files
    /// the user's tier allows them to index. If the user is allowed unlimited indices, then the
    /// max_indices_allowed is None.
    pub fn codebase_context_limits(&self) -> CodebaseContextUsageLimit {
        CodebaseContextUsageLimit {
            max_files_per_repo: self.request_limit_info.max_files_per_repo,
            max_indices_allowed: if self.request_limit_info.is_unlimited_codebase_indices {
                None
            } else {
                Some(self.request_limit_info.max_codebase_indices)
            },
            embedding_generation_batch_size: self
                .request_limit_info
                .embedding_generation_batch_size,
        }
    }
}

/// Voice request usage, only available if built with voice input support.
#[cfg(feature = "voice_input")]
impl AIRequestUsageModel {
    fn voice_requests(&self) -> usize {
        self.request_limit_info
            .voice_requests_used_since_last_refresh
    }

    fn voice_requests_limit(&self) -> usize {
        self.request_limit_info.voice_request_limit
    }

    fn is_unlimited_voice_requests(&self) -> bool {
        self.request_limit_info.is_unlimited_voice
    }

    /// Returns the number of remaining requests the user has based on their latest rate limit info.
    /// If the current time is past the next refresh time, then the number of remaining reqs is the limit.
    fn voice_requests_remaining(&self) -> usize {
        if self.request_limit_info.next_refresh_time.utc() <= Utc::now()
            || self.is_unlimited_voice_requests()
        {
            self.voice_requests_limit()
        } else {
            self.voice_requests_limit()
                .saturating_sub(self.voice_requests())
        }
    }

    /// Returns `true` if the user has at least one voice request before hitting the
    /// limit. Returns `false` otherwise.
    fn has_voice_requests_remaining(&self) -> bool {
        self.voice_requests_remaining() > 0
    }

    /// Checks request limits to see if the user can make a voice request.
    /// Returns true if the user can make a voice request, false otherwise.
    pub fn can_request_voice(&self) -> bool {
        self.has_voice_requests_remaining()
    }
}

impl SingletonEntity for AIRequestUsageModel {}

#[cfg(test)]
#[path = "request_usage_model_tests.rs"]
mod tests;
