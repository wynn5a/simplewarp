use chrono::{DateTime, Utc};
use session_sharing_protocol::common::SessionId;
use warp_cli::agent::Harness;
use warpui::AppContext;

use super::{AgentRunDisplayStatus, ConversationMetadata, SessionStatus};
use crate::ai::agent::api::ServerConversationToken;
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::ambient_agents::AmbientAgentTaskId;
use crate::ai::artifacts::Artifact;
use crate::ai::blocklist::history_model::{AIConversationMetadata, BlocklistAIHistoryModel};
use crate::ai::blocklist::orchestration_topology::orchestration_aware_conversation_status;
use crate::ai::conversation_navigation::ConversationNavigationData;
use crate::workspace::RestoreConversationLayout;

/// Stable projection identity used by list and navigation surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentConversationEntryId {
    Conversation(AIConversationId),
}

impl AgentConversationEntryId {
    pub fn as_key(&self) -> String {
        let AgentConversationEntryId::Conversation(id) = self;
        format!("conv_{id}")
    }
}

/// Navigation request input for resolving an entry or server-token handle at action time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentConversationNavigationSubject {
    Entry(AgentConversationEntryId),
    #[allow(dead_code)]
    ServerToken(ServerConversationToken),
}

/// Normalized row data for agent conversation list, management, and navigation surfaces.
///
/// The entry keeps local conversation identity, ambient run identity, cloud token identity,
/// display fields, and available actions together so callers do not recompute navigation
/// policy from stale partial sources.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentConversationEntry {
    pub id: AgentConversationEntryId,
    pub identity: AgentConversationIdentity,
    pub provenance: AgentConversationProvenance,
    pub display: AgentConversationDisplayData,
    pub backing: AgentConversationBackingData,
    pub capabilities: AgentConversationCapabilities,
}

/// Cross-system identifiers that may refer to the same underlying conversation/run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentConversationIdentity {
    pub local_conversation_id: Option<AIConversationId>,
    pub ambient_agent_task_id: Option<AmbientAgentTaskId>,
    pub server_conversation_token: Option<ServerConversationToken>,
    pub session_id: Option<SessionId>,
}

/// Display-only fields for rendering a conversation entry without consulting source models.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentConversationDisplayData {
    pub title: String,
    pub initial_query: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub status: AgentRunDisplayStatus,
    pub executor: Option<AgentConversationPrincipal>,
    pub request_usage: Option<f32>,
    pub run_time: Option<String>,
    pub session_status: Option<SessionStatus>,
    pub working_directory: Option<String>,
    pub harness: Option<Harness>,
    pub artifacts: Vec<Artifact>,
}

/// Type of principal that created or executed a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrincipalType {
    User,
    ServiceAccount,
}

impl PrincipalType {
    /// Parse from the wire-format string sent by the server.
    pub fn parse(s: &str) -> Option<Self> {
        if s.eq_ignore_ascii_case("user") {
            Some(PrincipalType::User)
        } else if s.eq_ignore_ascii_case("service_account") || s.eq_ignore_ascii_case("agent") {
            Some(PrincipalType::ServiceAccount)
        } else {
            None
        }
    }

    pub fn is_service_account(self) -> bool {
        self == PrincipalType::ServiceAccount
    }
}

/// Principal information normalized across local conversations and ambient runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentConversationPrincipal {
    pub name: Option<String>,
    pub uid: Option<String>,
    pub principal_type: Option<PrincipalType>,
}

/// Source category that explains why an entry exists and which backing systems can refresh it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentConversationProvenance {
    LocalInteractive,
    CloudSyncedConversation,
}

/// Availability flags for the source data that contributed to an entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentConversationBackingData {
    pub has_loaded_conversation: bool,
    pub has_local_persisted_data: bool,
    pub has_cloud_data: bool,
    pub has_ambient_run: bool,
}

/// Actions that should be exposed for an entry after applying current navigation policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentConversationCapabilities {
    pub can_open: bool,
    pub can_copy_link: bool,
    pub can_delete: bool,
    pub can_fork_locally: bool,
    pub can_cancel: bool,
}

impl AgentConversationEntry {
    /// Returns whether this entry represents a cloud agent run.
    pub fn is_cloud_agent_run(&self) -> bool {
        self.backing.has_ambient_run || self.identity.ambient_agent_task_id.is_some()
    }

    pub fn has_open_action(
        &self,
        restore_layout: Option<RestoreConversationLayout>,
        app: &AppContext,
    ) -> bool {
        super::AgentConversationsModel::resolve_open_action(
            AgentConversationNavigationSubject::Entry(self.id),
            restore_layout,
            app,
        )
        .is_some()
    }
}

fn conversation_title(
    metadata: &ConversationMetadata,
    history_model: &BlocklistAIHistoryModel,
) -> String {
    history_model
        .conversation(&metadata.nav_data.id)
        .and_then(|conversation| conversation.title().clone())
        .unwrap_or(metadata.nav_data.title.clone())
}

fn conversation_display_status(
    metadata: &ConversationMetadata,
    history_model: &BlocklistAIHistoryModel,
) -> AgentRunDisplayStatus {
    history_model
        .conversation(&metadata.nav_data.id)
        .map(|conversation| {
            // Roll the whole orchestration subtree (children, grandchildren,
            // …) into the card's status.
            AgentRunDisplayStatus::from_conversation_status(
                &orchestration_aware_conversation_status(history_model, conversation),
            )
        })
        .unwrap_or(AgentRunDisplayStatus::Succeeded)
}

fn conversation_request_usage(
    metadata: &ConversationMetadata,
    history_model: &BlocklistAIHistoryModel,
) -> Option<f32> {
    history_model
        .conversation(&metadata.nav_data.id)
        .map(|conversation| conversation.credits_spent())
        .or_else(|| {
            history_model
                .get_conversation_metadata(&metadata.nav_data.id)
                .and_then(|metadata| metadata.credits_spent)
        })
}

fn conversation_artifacts(
    metadata: &ConversationMetadata,
    history_model: &BlocklistAIHistoryModel,
) -> Vec<Artifact> {
    history_model
        .conversation(&metadata.nav_data.id)
        .map(|conversation| conversation.artifacts().to_vec())
        .or_else(|| {
            history_model
                .get_conversation_metadata(&metadata.nav_data.id)
                .map(|metadata| metadata.artifacts.clone())
        })
        .unwrap_or_default()
}

pub(super) fn entry_for_conversation(
    metadata: &ConversationMetadata,
    history_model: &BlocklistAIHistoryModel,
) -> AgentConversationEntry {
    let conversation_metadata = history_model.get_conversation_metadata(&metadata.nav_data.id);
    entry_for_conversation_parts(
        metadata.nav_data.clone(),
        conversation_metadata,
        history_model,
    )
}

pub(super) fn entry_for_historical_metadata(
    metadata: &AIConversationMetadata,
    nav_data: ConversationNavigationData,
    history_model: &BlocklistAIHistoryModel,
) -> AgentConversationEntry {
    entry_for_conversation_parts(nav_data, Some(metadata), history_model)
}

fn entry_for_conversation_parts(
    nav_data: ConversationNavigationData,
    conversation_metadata: Option<&AIConversationMetadata>,
    history_model: &BlocklistAIHistoryModel,
) -> AgentConversationEntry {
    let metadata = ConversationMetadata { nav_data };
    let conversation_id = metadata.nav_data.id;
    let status = conversation_display_status(&metadata, history_model);
    let has_loaded_conversation = history_model.conversation(&conversation_id).is_some();
    let has_local_persisted_data = conversation_metadata
        .is_some_and(|metadata| metadata.has_local_data)
        || has_loaded_conversation;
    let has_cloud_data = conversation_metadata.is_some_and(|metadata| metadata.has_cloud_data)
        || server_conversation_token_for_conversation(
            conversation_id,
            Some(&metadata.nav_data),
            history_model,
        )
        .is_some();
    let provenance = if has_cloud_data {
        AgentConversationProvenance::CloudSyncedConversation
    } else {
        AgentConversationProvenance::LocalInteractive
    };

    AgentConversationEntry {
        id: AgentConversationEntryId::Conversation(conversation_id),
        identity: AgentConversationIdentity {
            local_conversation_id: Some(conversation_id),
            ambient_agent_task_id: None,
            server_conversation_token: server_conversation_token_for_conversation(
                conversation_id,
                Some(&metadata.nav_data),
                history_model,
            ),
            session_id: None,
        },
        provenance,
        display: AgentConversationDisplayData {
            title: conversation_title(&metadata, history_model),
            initial_query: metadata.nav_data.initial_query.clone(),
            created_at: metadata.nav_data.last_updated.into(),
            last_updated: metadata.nav_data.last_updated.into(),
            status: status.clone(),
            executor: None,
            request_usage: conversation_request_usage(&metadata, history_model),
            run_time: None,
            session_status: None,
            working_directory: metadata
                .nav_data
                .latest_working_directory
                .clone()
                .or_else(|| metadata.nav_data.initial_working_directory.clone()),
            harness: Some(Harness::Oz),
            artifacts: conversation_artifacts(&metadata, history_model),
        },
        backing: AgentConversationBackingData {
            has_loaded_conversation,
            has_local_persisted_data,
            has_cloud_data,
            has_ambient_run: false,
        },
        capabilities: AgentConversationCapabilities {
            can_open: has_local_persisted_data || has_cloud_data,
            can_copy_link: server_conversation_token_for_conversation(
                conversation_id,
                Some(&metadata.nav_data),
                history_model,
            )
            .is_some(),
            can_delete: has_local_persisted_data,
            can_fork_locally: has_local_persisted_data,
            can_cancel: status.is_cancellable(),
        },
    }
}

fn server_conversation_token_for_conversation(
    conversation_id: AIConversationId,
    nav_data: Option<&ConversationNavigationData>,
    history_model: &BlocklistAIHistoryModel,
) -> Option<ServerConversationToken> {
    history_model
        .conversation(&conversation_id)
        .and_then(|conversation| conversation.server_conversation_token())
        .cloned()
        .or_else(|| {
            history_model
                .get_conversation_metadata(&conversation_id)
                .and_then(|metadata| metadata.server_conversation_token.clone())
        })
        .or_else(|| nav_data.and_then(|nav_data| nav_data.server_conversation_token.clone()))
}
