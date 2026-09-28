pub mod entry;
mod query;

use std::collections::{HashMap, HashSet};

pub use entry::{
    AgentConversationEntry, AgentConversationEntryId, AgentConversationNavigationSubject,
};
use fuzzy_match::FuzzyMatchResult;
use itertools::Itertools;
pub use query::query_conversation_entries;
use serde::{Deserialize, Serialize};
use warp_core::features::FeatureFlag;
use warp_core::ui::theme::WarpTheme;
use warp_core::ui::theme::color::internal_colors;
use warpui::color::ColorU;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use crate::ai::active_agent_views_model::ActiveAgentViewsModel;
use crate::ai::agent::api::ServerConversationToken;
use crate::ai::agent::conversation::{AIConversationId, ConversationStatus};
use crate::ai::blocklist::{
    BlocklistAIHistoryEvent, BlocklistAIHistoryModel, ConversationStatusUpdate,
};
use crate::ai::conversation_navigation::ConversationNavigationData;
use crate::ui_components::icons::Icon;
use crate::workspace::{RestoreConversationLayout, WorkspaceAction};

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum StatusFilter {
    #[default]
    All,
    Working,
    Done,
    Failed,
}

/// Frontend-specific classification of a normalized conversation-list entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentConversationListEntryState {
    Selected,
    OpenElsewhere,
    Available,
    Unavailable,
}

/// Per-frontend policy for classifying normalized conversation-list entries.
pub trait AgentConversationListPolicy: 'static {
    /// Classifies `entry` as selected, open elsewhere, available, or unavailable.
    fn classify_entry(
        &self,
        entry: &AgentConversationEntry,
        app: &AppContext,
    ) -> AgentConversationListEntryState;
}

/// A normalized conversation entry paired with optional title-match metadata.
pub struct AgentConversationQueryResult {
    pub entry: AgentConversationEntry,
    pub title_match: Option<FuzzyMatchResult>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentRunDisplayStatus {
    /// Conversation-derived lifecycle states for interactive conversations.
    InProgress,
    Succeeded,
    Error,
    Blocked {
        blocked_action: String,
    },
    Cancelled,
}

impl AgentRunDisplayStatus {
    pub fn from_conversation_status(status: &ConversationStatus) -> Self {
        match status {
            ConversationStatus::InProgress => Self::InProgress,
            // A recovery is in flight; the run is still working.
            ConversationStatus::TransientError => Self::InProgress,
            ConversationStatus::Success => Self::Succeeded,
            ConversationStatus::Error => Self::Error,
            ConversationStatus::Cancelled => Self::Cancelled,
            ConversationStatus::Blocked { blocked_action } => Self::Blocked {
                blocked_action: blocked_action.clone(),
            },
            // Treat a yielded conversation as still in progress for the
            // agent-run list display so it stays in the working bucket.
            ConversationStatus::WaitingForEvents => Self::InProgress,
        }
    }

    pub fn status_filter(&self) -> StatusFilter {
        match self {
            AgentRunDisplayStatus::InProgress => StatusFilter::Working,
            AgentRunDisplayStatus::Succeeded => StatusFilter::Done,
            AgentRunDisplayStatus::Error
            | AgentRunDisplayStatus::Blocked { .. }
            | AgentRunDisplayStatus::Cancelled => StatusFilter::Failed,
        }
    }

    pub fn to_conversation_status(&self) -> ConversationStatus {
        match self {
            AgentRunDisplayStatus::InProgress => ConversationStatus::InProgress,
            AgentRunDisplayStatus::Succeeded => ConversationStatus::Success,
            AgentRunDisplayStatus::Error => ConversationStatus::Error,
            AgentRunDisplayStatus::Blocked { blocked_action } => ConversationStatus::Blocked {
                blocked_action: blocked_action.clone(),
            },
            AgentRunDisplayStatus::Cancelled => ConversationStatus::Cancelled,
        }
    }

    pub fn is_cancellable(&self) -> bool {
        self.is_working()
    }

    pub fn is_working(&self) -> bool {
        matches!(self, AgentRunDisplayStatus::InProgress)
    }

    pub fn status_icon_and_color(&self, theme: &WarpTheme) -> (Icon, ColorU) {
        match self {
            AgentRunDisplayStatus::InProgress => (Icon::ClockLoader, theme.ansi_fg_magenta()),
            AgentRunDisplayStatus::Succeeded => (Icon::Check, theme.ansi_fg_green()),
            AgentRunDisplayStatus::Error => (Icon::Triangle, theme.ansi_fg_red()),
            AgentRunDisplayStatus::Blocked { .. } => (Icon::StopFilled, theme.ansi_fg_yellow()),
            AgentRunDisplayStatus::Cancelled => {
                (Icon::StopFilled, internal_colors::neutral_5(theme))
            }
        }
    }
}

impl std::fmt::Display for AgentRunDisplayStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentRunDisplayStatus::InProgress => write!(f, "In progress"),
            AgentRunDisplayStatus::Succeeded => write!(f, "Done"),
            AgentRunDisplayStatus::Error => write!(f, "Error"),
            AgentRunDisplayStatus::Blocked { .. } => write!(f, "Blocked"),
            AgentRunDisplayStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// Stores conversation metadata needed for display in conversation/task views.
pub struct ConversationMetadata {
    pub nav_data: ConversationNavigationData,
}

/// This model serves as the interface for reading local agent conversations.
/// It backs both the agent management view and the conversation list view.
pub struct AgentConversationsModel {
    /// A map of conversation IDs to local conversations.
    conversations: HashMap<AIConversationId, ConversationMetadata>,
}

pub enum AgentConversationsModelEvent {
    /// Conversation data was loaded or refreshed.
    ConversationsLoaded,
    /// Conversation status data was updated
    ConversationUpdated { kind: ConversationUpdateKind },
    /// Conversation artifacts were updated (plans, PRs, etc.)
    ConversationArtifactsUpdated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationUpdateKind {
    /// The conversation was re-loaded into a terminal view.
    Restored,
    /// The conversation's status was set.
    StatusSet {
        prev_filter: StatusFilter,
        new_filter: StatusFilter,
    },
    /// Conversation metadata or capabilities changed.
    MetadataChanged,
    /// Conversation title changed.
    TitleChanged,
}

impl Entity for AgentConversationsModel {
    type Event = AgentConversationsModelEvent;
}

impl SingletonEntity for AgentConversationsModel {}

impl AgentConversationsModel {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let history_model = BlocklistAIHistoryModel::handle(ctx);
        ctx.subscribe_to_model(&history_model, move |me, _, event, ctx| {
            me.handle_history_event(event, ctx);
        });

        let active_views_model = ActiveAgentViewsModel::handle(ctx);
        ctx.subscribe_to_model(&active_views_model, |me, _, _event, ctx| {
            me.sync_conversations(ctx);
        });

        Self {
            conversations: HashMap::new(),
        }
    }

    /// Sync all conversations to the AgentConversationsModel.
    ///
    /// This function will loop through all active panes, recently closed panes, and historical
    /// conversations to construct a complete snapshot of conversations.
    pub fn sync_conversations(&mut self, ctx: &mut ModelContext<Self>) {
        if !FeatureFlag::InteractiveConversationManagementView.is_enabled() {
            return;
        }

        let nav_data_list = ConversationNavigationData::all_conversations(ctx);

        self.conversations.clear();
        for nav_data in nav_data_list {
            let conversation_id = nav_data.id;
            let metadata = ConversationMetadata { nav_data };
            self.conversations.insert(conversation_id, metadata);
        }

        ctx.emit(AgentConversationsModelEvent::ConversationsLoaded);
    }

    /// Returns normalized, owned entries for agent management/navigation surfaces.
    pub fn get_entries(&self, app: &AppContext) -> Vec<AgentConversationEntry> {
        self.unfiltered_entries(app)
            .into_iter()
            .sorted_by(|a, b| b.display.last_updated.cmp(&a.display.last_updated))
            .collect()
    }

    /// Returns normalized entries before user-selected filters are applied.
    fn unfiltered_entries(&self, app: &AppContext) -> Vec<AgentConversationEntry> {
        let history_model = BlocklistAIHistoryModel::as_ref(app);
        let mut entries = Vec::new();
        let mut emitted_conversation_ids = HashSet::new();

        for metadata in self.conversations.values() {
            let conversation_id = metadata.nav_data.id;
            let entry = entry::entry_for_conversation(metadata, history_model);
            emitted_conversation_ids.insert(conversation_id);
            entries.push(entry);
        }

        for metadata in history_model.get_local_conversations_metadata() {
            if emitted_conversation_ids.contains(&metadata.id) {
                continue;
            }
            let nav_data =
                ConversationNavigationData::from_historical_conversation_metadata(metadata);
            entries.push(entry::entry_for_historical_metadata(
                metadata,
                nav_data,
                history_model,
            ));
        }

        entries
    }

    pub fn get_entry_by_id(
        &self,
        id: &AgentConversationEntryId,
        app: &AppContext,
    ) -> Option<AgentConversationEntry> {
        let history_model = BlocklistAIHistoryModel::as_ref(app);
        let AgentConversationEntryId::Conversation(conversation_id) = id;
        self.conversations
            .get(conversation_id)
            .map(|metadata| entry::entry_for_conversation(metadata, history_model))
            .or_else(|| {
                history_model
                    .get_conversation_metadata(conversation_id)
                    .map(|metadata| {
                        let nav_data =
                            ConversationNavigationData::from_historical_conversation_metadata(
                                metadata,
                            );
                        entry::entry_for_historical_metadata(metadata, nav_data, history_model)
                    })
            })
    }

    pub fn resolve_open_action(
        subject: AgentConversationNavigationSubject,
        restore_layout: Option<RestoreConversationLayout>,
        app: &AppContext,
    ) -> Option<WorkspaceAction> {
        let model = Self::as_ref(app);
        match subject {
            AgentConversationNavigationSubject::Entry(id) => model
                .get_entry_by_id(&id, app)
                .and_then(|entry| model.resolve_entry_open_action(&entry, restore_layout, app)),
            AgentConversationNavigationSubject::ServerToken(server_token) => model
                .entry_for_server_token(&server_token, app)
                .and_then(|entry| model.resolve_entry_open_action(&entry, restore_layout, app)),
        }
    }

    fn resolve_entry_open_action(
        &self,
        entry: &AgentConversationEntry,
        restore_layout: Option<RestoreConversationLayout>,
        app: &AppContext,
    ) -> Option<WorkspaceAction> {
        let active_views_model = ActiveAgentViewsModel::as_ref(app);

        if let Some(conversation_id) = entry.identity.local_conversation_id
            && active_views_model.is_conversation_open(conversation_id, app)
        {
            if let Some(nav_data) = self
                .conversations
                .get(&conversation_id)
                .map(|metadata| &metadata.nav_data)
            {
                return Some(WorkspaceAction::RestoreOrNavigateToConversation {
                    conversation_id,
                    window_id: nav_data.window_id,
                    pane_view_locator: nav_data.pane_view_locator,
                    terminal_view_id: nav_data.terminal_view_id,
                    restore_layout,
                });
            }

            if let Some(terminal_view_id) =
                active_views_model.get_terminal_view_id_for_conversation(conversation_id, app)
            {
                return Some(WorkspaceAction::FocusTerminalViewInWorkspace { terminal_view_id });
            }
        }

        if let Some(conversation_id) = entry.identity.local_conversation_id {
            let nav_data = self
                .conversations
                .get(&conversation_id)
                .map(|metadata| &metadata.nav_data);
            if !entry.backing.has_cloud_data
                || entry.backing.has_local_persisted_data
                || entry.backing.has_loaded_conversation
                || nav_data.is_some()
            {
                return Some(WorkspaceAction::RestoreOrNavigateToConversation {
                    conversation_id,
                    window_id: nav_data.and_then(|nav_data| nav_data.window_id),
                    pane_view_locator: None,
                    terminal_view_id: nav_data.and_then(|nav_data| nav_data.terminal_view_id),
                    restore_layout,
                });
            }
        }

        None
    }

    fn entry_for_server_token(
        &self,
        server_token: &ServerConversationToken,
        app: &AppContext,
    ) -> Option<AgentConversationEntry> {
        let history_model = BlocklistAIHistoryModel::as_ref(app);
        let conversation_id = history_model.find_conversation_id_by_server_token(server_token)?;
        self.get_entry_by_id(
            &AgentConversationEntryId::Conversation(conversation_id),
            app,
        )
    }

    fn handle_history_event(
        &mut self,
        event: &BlocklistAIHistoryEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        if !FeatureFlag::InteractiveConversationManagementView.is_enabled() {
            return;
        }
        match event {
            // Events that affect conversation navigation data - need full sync
            BlocklistAIHistoryEvent::StartedNewConversation { .. }
            | BlocklistAIHistoryEvent::SetActiveConversation { .. }
            | BlocklistAIHistoryEvent::AppendedExchange { .. }
            | BlocklistAIHistoryEvent::SplitConversation { .. }
            | BlocklistAIHistoryEvent::RestoredConversations { .. }
            | BlocklistAIHistoryEvent::RemoveConversation { .. }
            | BlocklistAIHistoryEvent::DeletedConversation { .. }
            | BlocklistAIHistoryEvent::ClearedConversationsForTerminalSurface { .. }
            | BlocklistAIHistoryEvent::ClearedActiveConversation { .. }
            => {
                self.sync_conversations(ctx);
            }

            // Status changes - just trigger re-render since status is looked up at render time
            BlocklistAIHistoryEvent::UpdatedConversationStatus {
                update, new_status, ..
            } => {
                let kind = match update {
                    ConversationStatusUpdate::Restored => ConversationUpdateKind::Restored,
                    ConversationStatusUpdate::Changed { prev_status } => {
                        ConversationUpdateKind::StatusSet {
                            prev_filter: AgentRunDisplayStatus::from_conversation_status(
                                prev_status,
                            )
                            .status_filter(),
                            new_filter: AgentRunDisplayStatus::from_conversation_status(new_status)
                                .status_filter(),
                        }
                    }
                };
                ctx.emit(AgentConversationsModelEvent::ConversationUpdated { kind });
            }

            // Artifact changes - notify so panels re-render from the history model.
            BlocklistAIHistoryEvent::UpdatedConversationArtifacts { .. } => {
                ctx.emit(AgentConversationsModelEvent::ConversationArtifactsUpdated);
            }
            BlocklistAIHistoryEvent::UpdatedConversationTitle { .. } => {
                ctx.emit(AgentConversationsModelEvent::ConversationUpdated {
                    kind: ConversationUpdateKind::TitleChanged,
                });
            }

            // Task/exchange-level changes that don't affect conversation navigation.
            BlocklistAIHistoryEvent::CreatedSubtask { .. }
            | BlocklistAIHistoryEvent::UpgradedTask { .. }
            | BlocklistAIHistoryEvent::ReassignedExchange { .. }
            | BlocklistAIHistoryEvent::UpdatedTodoList { .. }
            | BlocklistAIHistoryEvent::UpdatedAutoexecuteOverride { .. }
            // UpdatedStreamingExchange covers streaming and other exchange-level updates but
            // doesn't change any ConversationNavigationData fields (title comes from
            // UpdateTaskDescription, last_updated uses exchange.start_time which is set at append time).
            | BlocklistAIHistoryEvent::UpdatedStreamingExchange { .. }
            | BlocklistAIHistoryEvent::ConversationTransferredBetweenTerminalSurfaces { .. }
            | BlocklistAIHistoryEvent::NewConversationRequestComplete { .. }
            | BlocklistAIHistoryEvent::OrchestrationConfigUpdated { .. }
            | BlocklistAIHistoryEvent::ConversationUsageMetadataUpdated { .. }
            | BlocklistAIHistoryEvent::UpdatedConversationMetadata { .. } => {}

            BlocklistAIHistoryEvent::ConversationServerTokenAssigned { .. } => {
                ctx.emit(AgentConversationsModelEvent::ConversationUpdated {
                    kind: ConversationUpdateKind::MetadataChanged,
                });
            }
        }
    }
}

#[cfg(test)]
#[path = "agent_conversations_model_tests.rs"]
mod tests;
