//! This module contains functions for loading and fetching conversation data
//! from memory and the local database.

use std::collections::HashMap;
use std::future::Future;

use futures::FutureExt;
use itertools::Itertools as _;
use persistence::model::AgentConversationRecord;

use super::{
    AIConversationMetadata, BlocklistAIHistoryModel, MAX_HISTORICAL_CONVERSATIONS,
    agent_id_key_from_persisted_data,
};
use crate::ai::agent::api::ServerConversationToken;
use crate::ai::agent::conversation::{AIConversation, AIConversationId};
#[cfg(feature = "local_fs")]
use crate::persistence::agent::read_agent_conversation_by_id;
use crate::persistence::model::{
    AgentConversation, AgentConversationData, AgentConversationSummary,
};

/// Representation of the conversation data that can be fetched from cloud storage.
///
/// The exact format depends on the agent harness that produced the conversation.
pub enum CloudConversationData {
    /// A conversation produced by the Oz harness, which we can materialize into the
    /// [`AIConversation`] data model.
    Oz(Box<AIConversation>),
}

/// Converts an `AgentConversation` from the database to an `AIConversation`.
/// This utility function extracts the conversion logic that was originally embedded
/// in the terminal view restoration process.
#[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
pub fn convert_persisted_conversation_to_ai_conversation(
    persisted_conversation: AgentConversation,
) -> Option<AIConversation> {
    convert_persisted_conversation_to_ai_conversation_with_metadata(persisted_conversation)
}

/// Enhanced version of the conversion function with additional metadata.
/// This version supports the full feature set needed by terminal view restoration.
pub fn convert_persisted_conversation_to_ai_conversation_with_metadata(
    persisted_conversation: AgentConversation,
) -> Option<AIConversation> {
    let AgentConversation {
        tasks,
        conversation:
            AgentConversationRecord {
                conversation_id,
                conversation_data,
                ..
            },
    } = persisted_conversation;

    let conversation_id = match AIConversationId::try_from(conversation_id) {
        Ok(id) => id,
        Err(e) => {
            log::warn!("Failed to convert conversation ID: {e:?}");
            return None;
        }
    };

    let conversation_data = serde_json::from_str::<AgentConversationData>(&conversation_data).ok();

    // Local-DB restore: an empty `agent_tasks` row is the normal shape of a
    // child conversation persisted before its first server response, so
    // synthesize a fresh optimistic root rather than failing the restore.
    match AIConversation::new_restored_synthesizing_on_empty(
        conversation_id,
        tasks,
        conversation_data,
    ) {
        Ok(conversation) => Some(conversation),
        Err(e) => {
            log::warn!("Failed to convert persisted conversation to AIConversation: {e:?}");
            None
        }
    }
}

/// Boxes a future with the right type for the platform.
/// On WASM, futures must not implement Send.
fn box_future<F>(f: F) -> warpui::r#async::BoxFuture<'static, Option<CloudConversationData>>
where
    F: Future<Output = Option<CloudConversationData>> + warpui::r#async::Spawnable,
{
    cfg_if::cfg_if! {
        if #[cfg(target_family = "wasm")] {
            f.boxed_local()
        } else {
            f.boxed()
        }
    }
}

impl BlocklistAIHistoryModel {
    /// Loads conversation data from the appropriate source (DB or server).
    ///
    /// Loads the conversation from memory when present, otherwise from the
    /// local database when its metadata says local data exists.
    ///
    /// Note: This does NOT insert the conversation into memory. Callers are responsible
    /// for inserting the loaded conversation if needed.
    pub fn load_conversation_data(
        &self,
        conversation_id: AIConversationId,
    ) -> warpui::r#async::BoxFuture<'static, Option<CloudConversationData>> {
        // First check if the conversation is already in memory
        if let Some(conversation) = self.conversations_by_id.get(&conversation_id) {
            return box_future(futures::future::ready(Some(CloudConversationData::Oz(
                Box::new(conversation.clone()),
            ))));
        }

        // Check metadata to determine the source
        let Some(metadata) = self
            .all_conversations_metadata
            .get(&conversation_id)
            .cloned()
        else {
            log::warn!("No metadata found for conversation {conversation_id}");
            return box_future(futures::future::ready(None));
        };

        if metadata.has_local_data {
            // Load from local database synchronously
            let result = self
                .load_conversation_from_db(&conversation_id)
                .map(|c| CloudConversationData::Oz(Box::new(c)));
            box_future(futures::future::ready(result))
        } else {
            // Cloud conversation storage requires a Warp account/server, which
            // this build never has; there is nothing beyond the local database.
            log::warn!(
                "Cannot load conversation {conversation_id}: no local data and no server fallback"
            );
            box_future(futures::future::ready(None))
        }
    }

    /// Loads a conversation by its server token from local state.
    ///
    /// First attempts to find the conversation in local metadata and load it
    /// via `load_conversation_data`.
    ///
    /// Note: This does NOT insert the conversation into memory. Callers are responsible
    /// for inserting the loaded conversation if needed.
    pub fn load_conversation_by_server_token(
        &mut self,
        server_token: &ServerConversationToken,
    ) -> warpui::r#async::BoxFuture<'static, Option<CloudConversationData>> {
        let conversation_id =
            self.get_or_set_canonical_conversation_id_for_server_token(server_token);
        self.load_conversation_data(conversation_id)
    }

    /// Loads a conversation from local DB and returns it.
    /// This is a private helper method. Use `get_load_conversation_data_future` instead.
    ///
    /// Note: This does NOT insert the conversation into memory. Callers are responsible
    /// for inserting the loaded conversation if needed.
    pub(super) fn load_conversation_from_db(
        &self,
        conversation_id: &AIConversationId,
    ) -> Option<AIConversation> {
        // First check if the conversation is in memory
        if let Some(conversation) = self.conversations_by_id.get(conversation_id) {
            return Some(conversation.clone());
        }

        // If not in memory, try to load from the database
        #[cfg(feature = "local_fs")]
        {
            let persisted_ai_conversation = self.db_connection.clone().and_then(|conn| {
                let mut conn = conn.lock().ok()?;

                let id_str = conversation_id.to_string();
                log::info!("Loading conversation {id_str} from db");
                match read_agent_conversation_by_id(&mut conn, &id_str) {
                    Ok(Some(conv)) => Some(conv),
                    Ok(None) => {
                        log::warn!("No AgentConversation found with id {id_str}");
                        None
                    }
                    Err(e) => {
                        log::warn!("Failed to read AgentConversation {id_str}: {e:?}");
                        None
                    }
                }
            });

            // Convert the persisted conversation to an AIConversation
            if let Some(persisted_conversation) = persisted_ai_conversation
                && let Some(conversation) =
                    convert_persisted_conversation_to_ai_conversation(persisted_conversation)
            {
                return Some(conversation);
            }
        }

        None
    }

    /// Initializes historical conversations from restored agent conversations.
    ///
    /// At startup the conversations carry only `agent_conversations` records
    /// (empty task lists) whose summaries were computed at write time (or
    /// derived once at read time); tests may pass fully-hydrated
    /// conversations, whose summaries are derived from their tasks here.
    pub(super) fn initialize_historical_conversations(
        &mut self,
        conversations: &[AgentConversation],
    ) {
        struct HistoricalConversationRow<'a> {
            agent_conversation: &'a AgentConversation,
            conversation_id: AIConversationId,
            conversation_data: Option<AgentConversationData>,
            summary: AgentConversationSummary,
        }

        let historical_rows: Vec<_> = conversations
            .iter()
            .sorted_by_key(|c| c.conversation.last_modified_at)
            .rev()
            .take(MAX_HISTORICAL_CONVERSATIONS)
            .filter_map(|agent_conversation| {
                let conversation_id = match AIConversationId::try_from(
                    agent_conversation.conversation.conversation_id.clone(),
                ) {
                    Ok(id) => id,
                    Err(e) => {
                        log::warn!("Failed to convert conversation ID: {e:?}");
                        return None;
                    }
                };

                // Prefer the write-time summary from the `summary` column;
                // fall back to deriving from tasks for fully-hydrated inputs.
                let summary = agent_conversation
                    .conversation
                    .summary
                    .as_deref()
                    .and_then(|json| serde_json::from_str::<AgentConversationSummary>(json).ok())
                    .unwrap_or_else(|| {
                        AgentConversationSummary::from_tasks(agent_conversation.tasks.iter())
                    });

                if !summary.is_restorable {
                    return None;
                }

                let conversation_data = serde_json::from_str::<AgentConversationData>(
                    &agent_conversation.conversation.conversation_data,
                )
                .ok();

                if let Some(data) = conversation_data.as_ref() {
                    if let Some(agent_id) = agent_id_key_from_persisted_data(data) {
                        self.agent_id_to_conversation_id
                            .insert(agent_id.to_owned(), conversation_id);
                    }
                    if let Some(token) = data.server_conversation_token.as_ref() {
                        self.server_token_to_conversation_id
                            .insert(ServerConversationToken::new(token.clone()), conversation_id);
                    }
                }

                Some(HistoricalConversationRow {
                    agent_conversation,
                    conversation_id,
                    conversation_data,
                    summary,
                })
            })
            .collect();

        let collected: HashMap<AIConversationId, AIConversationMetadata> = historical_rows
            .into_iter()
            .filter_map(|row| {
                let HistoricalConversationRow {
                    agent_conversation,
                    conversation_id,
                    conversation_data,
                    summary,
                } = row;

                // Child agent conversations are managed by their parent's
                // status card and should not appear in navigation/history.
                // Record the parent→child mapping before filtering so that
                // create_missing_child_agent_panes can discover children
                // before they are loaded into conversations_by_id.
                if let Some(parent_id) = conversation_data
                    .as_ref()
                    .and_then(|data| self.resolved_parent_conversation_id_from_persisted_data(data))
                {
                    self.index_child_conversation(conversation_id, parent_id);
                    // Eagerly hydrate the child conversation into
                    // `conversations_by_id` so the pill bar and orchestration
                    // transcript name resolution can find it before the
                    // parent's hidden child pane materializes lazily. This is
                    // restricted to orchestration children only — non-child
                    // historical conversations continue to load lazily via
                    // `restore_conversations`. We do NOT emit
                    // `RestoredConversations`, touch
                    // `live_conversation_ids_for_terminal_view`, or update
                    // `terminal_view_created_at` here; those still happen
                    // later when the hidden pane is materialized via
                    // `restore_conversations`. A subsequent `restore_conversations`
                    // call replaces this entry idempotently.
                    //
                    // Startup rows carry no tasks, so the child's task
                    // payload is loaded from the local DB; fully-hydrated
                    // inputs convert directly.
                    let child_conversation = if agent_conversation.tasks.is_empty() {
                        self.load_conversation_from_db(&conversation_id)
                    } else {
                        convert_persisted_conversation_to_ai_conversation_with_metadata(
                            agent_conversation.clone(),
                        )
                    };
                    if let Some(child_conversation) = child_conversation {
                        self.conversations_by_id
                            .insert(conversation_id, child_conversation);
                    } else {
                        log::warn!(
                            "Failed to eagerly hydrate orchestration child {conversation_id}; \
                             pill bar / name resolution will fall back to lazy materialization",
                        );
                    }
                    return None;
                }

                // Skip conversations that only contain passive AutoCodeDiff
                // system queries the user never interacted with (past
                // accepting or rejecting the diff).
                if summary.is_unlisted_auto_code_diff {
                    return None;
                }

                let AgentConversationSummary {
                    initial_query,
                    title,
                    initial_working_directory,
                    ..
                } = summary;

                if initial_query.is_empty() {
                    log::warn!(
                        "Failed to record conversation with ID {conversation_id} because it was missing an initial query"
                    );
                    return None;
                }

                let credits_spent = conversation_data
                    .as_ref()
                    .and_then(|data| data.conversation_usage_metadata.as_ref())
                    .map(|m| m.credits_spent + m.platform_credits_spent);
                let artifacts = conversation_data
                    .as_ref()
                    .and_then(|data| data.artifacts_json.as_ref())
                    .and_then(|json| serde_json::from_str(json).ok())
                    .unwrap_or_default();
                let server_conversation_token = conversation_data
                    .as_ref()
                    .and_then(|data| data.server_conversation_token.as_ref())
                    .map(|token| ServerConversationToken::new(token.clone()));

                Some((
                    conversation_id,
                    AIConversationMetadata {
                        id: conversation_id,
                        title,
                        initial_query,
                        last_modified_at: agent_conversation.conversation.last_modified_at,
                        initial_working_directory,
                        credits_spent,
                        // If we have a server token, the conversation was synced to cloud
                        has_cloud_data: server_conversation_token.is_some(),
                        server_conversation_token,
                        has_local_data: true,
                        artifacts,
                        // Carry parent linkage from persisted data so child-agent
                        // status survives even if the parent isn't resolvable
                        // locally (the child-skip above only fires when the
                        // parent conversation is known).
                        parent_conversation_id: conversation_data
                            .as_ref()
                            .and_then(|data| data.parent_conversation_id.as_deref())
                            .and_then(|id| AIConversationId::try_from(id.to_owned()).ok()),
                        parent_agent_id: conversation_data
                            .as_ref()
                            .and_then(|data| data.parent_agent_id.clone()),
                    },
                ))
            })
            .collect();
        self.all_conversations_metadata = collected;
    }
}
