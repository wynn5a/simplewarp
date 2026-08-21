use std::sync::Arc;

use anyhow::Context as _;
use chrono::{DateTime, Utc};
use futures::channel::oneshot::{self, Receiver};
use instant::Instant;
use serde::{Deserialize, Serialize};
use warp_core::user_preferences::GetUserPreferences as _;
use warp_errors::report_error;
pub use warp_graphql::billing::BonusGrantType;
use warp_graphql::scalars::time::ServerTimestamp;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use crate::BlocklistAIHistoryModel;
use crate::ai::agent::AIAgentExchangeId;
use crate::ai::agent::conversation::AIConversationId;
use crate::auth::AuthStateProvider;
use crate::server::server_api::ai::AIClient;
use crate::settings::AISettings;
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::workspaces::workspace::WorkspaceUid;

/// Threshold of ambient-only credits at which we surface upgrade/CTA UI.
pub const AMBIENT_AGENT_TRIAL_CREDIT_THRESHOLD: i32 = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BonusGrantScope {
    User,
    Team(WorkspaceUid),
    Workspace(WorkspaceUid),
}

impl BonusGrantScope {
    pub fn workspace_uid(&self) -> Option<WorkspaceUid> {
        match self {
            BonusGrantScope::User => None,
            BonusGrantScope::Team(uid) | BonusGrantScope::Workspace(uid) => Some(*uid),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BonusGrant {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub cost_cents: i32,
    pub expiration: Option<chrono::DateTime<chrono::Utc>>,
    pub grant_type: BonusGrantType,
    pub reason: String,
    pub user_facing_message: Option<String>,
    pub request_credits_granted: i32,
    pub request_credits_remaining: i32,
    pub scope: BonusGrantScope,
}

/// The key for the corresponding entry in UserDefaults.
const REQUEST_LIMIT_INFO_CACHE_KEY: &str = "AIRequestLimitInfo";
const AMBIENT_CREDITS_BANNER_DISMISSED_KEY: &str = "AmbientCreditsBannerDismissed";

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

/// Contains all usage-related information fetched from the server.
pub struct RequestUsageInfo {
    pub request_limit_info: RequestLimitInfo,
    pub bonus_grants: Vec<BonusGrant>,
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

fn cache_request_limit_info(request_limit_info: RequestLimitInfo, app_mut: &mut AppContext) {
    if let Ok(serialized) = serde_json::to_string(&request_limit_info) {
        let _ = app_mut
            .private_user_preferences()
            .write_value(REQUEST_LIMIT_INFO_CACHE_KEY, serialized);
    }
}

fn get_cached_request_limit_info(app_mut: &mut AppContext) -> Option<RequestLimitInfo> {
    app_mut
        .private_user_preferences()
        .read_value(REQUEST_LIMIT_INFO_CACHE_KEY)
        .unwrap_or_default()
        .and_then(|serialized| serde_json::from_str(serialized.as_str()).ok())
}

fn cache_ambient_credits_banner_dismissed(dismissed: bool, app_mut: &mut AppContext) {
    let _ = app_mut
        .private_user_preferences()
        .write_value(AMBIENT_CREDITS_BANNER_DISMISSED_KEY, dismissed.to_string());
}

fn get_cached_ambient_credits_banner_dismissed(app_mut: &mut AppContext) -> bool {
    app_mut
        .private_user_preferences()
        .read_value(AMBIENT_CREDITS_BANNER_DISMISSED_KEY)
        .unwrap_or_default()
        .and_then(|value| value.parse::<bool>().ok())
        .unwrap_or_default()
}

pub struct AIRequestUsageModel {
    ai_client: Arc<dyn AIClient>,

    /// The last time at which `request_limit_info` was updated.
    last_update_time: Option<Instant>,

    request_limit_info: RequestLimitInfo,

    bonus_grants: Vec<BonusGrant>,

    /// Whether the ambient trial credits banner has been dismissed by the user.
    ambient_credits_banner_dismissed: bool,
}

impl Entity for AIRequestUsageModel {
    type Event = AIRequestUsageModelEvent;
}

pub enum AIRequestUsageModelEvent {
    RequestUsageUpdated,
    AmbientCreditsBannerDismissed,
    RequestBonusRefunded {
        requests_refunded: i32,
        server_conversation_id: String,
        request_id: String,
    },
}

impl AIRequestUsageModel {
    pub fn new(ai_client: Arc<dyn AIClient>, ctx: &mut ModelContext<Self>) -> Self {
        // Check if the user has cached request limit info from before.
        // This is only used to show the latest known value before we finish refreshing from the server below.
        let cached_request_limit_info = get_cached_request_limit_info(ctx);
        let request_limit_info = cached_request_limit_info.unwrap_or_default();
        let ambient_credits_banner_dismissed = get_cached_ambient_credits_banner_dismissed(ctx);

        Self {
            ai_client,
            request_limit_info,
            last_update_time: None,
            bonus_grants: vec![],
            ambient_credits_banner_dismissed,
        }
    }

    #[cfg(test)]
    pub fn new_for_test(ai_client: Arc<dyn AIClient>, ctx: &mut ModelContext<Self>) -> Self {
        Self {
            ai_client,
            last_update_time: None,
            request_limit_info: RequestLimitInfo::default(),
            bonus_grants: vec![],
            ambient_credits_banner_dismissed: get_cached_ambient_credits_banner_dismissed(ctx),
        }
    }

    pub fn last_update_time(&self) -> Option<Instant> {
        self.last_update_time
    }

    /// Refreshes the latest AI request usage and bonus grants from the server.
    ///
    /// The receiver resolves to the freshly fetched base request limit. It
    /// resolves to `None` if the user is logged out or the request fails, so
    /// callers making entitlement decisions do not fall back to cached data.
    pub fn refresh_request_usage(
        &mut self,
        ctx: &mut ModelContext<Self>,
    ) -> Receiver<Option<usize>> {
        let (sender, receiver) = oneshot::channel();
        if !AuthStateProvider::as_ref(ctx).get().is_logged_in() {
            let _ = sender.send(None);
            return receiver;
        }

        let ai_client = self.ai_client.clone();
        let mut sender = Some(sender);
        ctx.spawn(
            async move { ai_client.get_request_limit_info().await },
            move |model, result, ctx| {
                let request_limit = match result {
                    Ok(usage_info) => {
                        let request_limit = usage_info.request_limit_info.limit;
                        model.bonus_grants = usage_info.bonus_grants;
                        model.update_request_limit_info(usage_info.request_limit_info, ctx);
                        Some(request_limit)
                    }
                    Err(e) => {
                        log::warn!("Failed to retrieve request limit info: {e:#}");
                        None
                    }
                };
                if let Some(sender) = sender.take() {
                    let _ = sender.send(request_limit);
                }
            },
        );
        receiver
    }

    /// Spawns a task to refresh the latest AI request usage and bonus grants.
    pub fn refresh_request_usage_async(&mut self, ctx: &mut ModelContext<Self>) {
        drop(self.refresh_request_usage(ctx));
    }

    pub fn update_request_limit_info(
        &mut self,
        request_limit_info: RequestLimitInfo,
        ctx: &mut ModelContext<Self>,
    ) {
        self.last_update_time = Some(Instant::now());
        self.request_limit_info = request_limit_info;
        cache_request_limit_info(request_limit_info, ctx);

        AISettings::handle(ctx).update(ctx, |ai_settings, ctx| {
            ai_settings.update_quota_info(&request_limit_info, ctx);
        });

        ctx.emit(AIRequestUsageModelEvent::RequestUsageUpdated);
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
        if self.next_refresh_time() <= Utc::now() || self.is_unlimited() {
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

    pub fn requests_used(&self) -> usize {
        if self.next_refresh_time() <= Utc::now() {
            return 0;
        }
        self.request_limit_info.num_requests_used_since_refresh
    }

    pub fn request_limit(&self) -> usize {
        self.request_limit_info.limit
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

    pub fn next_refresh_time(&self) -> DateTime<Utc> {
        self.request_limit_info.next_refresh_time.utc()
    }

    pub fn is_unlimited(&self) -> bool {
        self.request_limit_info.is_unlimited
    }

    pub fn refresh_duration_to_string(&self) -> String {
        match self.request_limit_info.request_limit_refresh_duration {
            RequestLimitRefreshDuration::Weekly => "weekly".to_string(),
            RequestLimitRefreshDuration::Monthly => "monthly".to_string(),
            RequestLimitRefreshDuration::EveryTwoWeeks => "biweekly".to_string(),
        }
    }

    pub fn bonus_grants(&self) -> &[BonusGrant] {
        &self.bonus_grants
    }

    /// Returns the total remaining ambient-only credits for the user.
    /// Returns None if the user has never received any ambient-only grants.
    pub fn ambient_only_credits_remaining(&self) -> Option<i32> {
        let ambient_grants: Vec<_> = self
            .bonus_grants
            .iter()
            .filter(|g| g.grant_type == BonusGrantType::AmbientOnly)
            .collect();
        if ambient_grants.is_empty() {
            None
        } else {
            Some(
                ambient_grants
                    .iter()
                    .map(|g| g.request_credits_remaining)
                    .sum(),
            )
        }
    }

    pub fn is_ambient_credits_banner_dismissed(&self) -> bool {
        self.ambient_credits_banner_dismissed
    }

    pub fn dismiss_ambient_credits_banner(&mut self, ctx: &mut ModelContext<Self>) {
        if self.ambient_credits_banner_dismissed {
            return;
        }
        self.ambient_credits_banner_dismissed = true;
        cache_ambient_credits_banner_dismissed(true, ctx);
        ctx.emit(AIRequestUsageModelEvent::AmbientCreditsBannerDismissed);
    }

    pub fn total_workspace_and_team_bonus_credits_remaining(&self, uid: WorkspaceUid) -> i32 {
        let now = Utc::now();
        self.bonus_grants
            .iter()
            .filter(|grant| grant.scope.workspace_uid() == Some(uid))
            .filter(|grant| grant.expiration.is_none_or(|exp| now < exp))
            .map(|grant| grant.request_credits_remaining)
            .sum()
    }

    pub fn total_current_workspace_and_team_bonus_credits_remaining(
        &self,
        ctx: &AppContext,
    ) -> i32 {
        UserWorkspaces::as_ref(ctx)
            .current_workspace()
            .map(|workspace| self.total_workspace_and_team_bonus_credits_remaining(workspace.uid))
            .unwrap_or(0)
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
        if self.next_refresh_time() <= Utc::now() || self.is_unlimited_voice_requests() {
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
