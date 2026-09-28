use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{Duration, Utc};
use parking_lot::Mutex;
use persistence::model::AgentConversationData;
use warp_core::features::FeatureFlag;
use warpui::{App, EntityId, ModelHandle, SingletonEntity};

use super::entry::{
    AgentConversationEntryId, AgentConversationNavigationSubject, AgentConversationProvenance,
};
use super::query::{DEFAULT_RESULT_COUNT, MAX_SEARCH_RESULTS};
use super::{
    AgentConversationsModel, AgentConversationsModelEvent, ConversationMetadata,
    ConversationUpdateKind, StatusFilter, query_conversation_entries,
};
use crate::ai::active_agent_views_model::ActiveAgentViewsModel;
use crate::ai::agent::api::ServerConversationToken;
use crate::ai::agent::conversation::{AIConversation, AIConversationId, ConversationStatus};
use crate::ai::blocklist::history_model::{
    BlocklistAIHistoryEvent, BlocklistAIHistoryModel, ConversationStatusUpdate,
};
use crate::ai::conversation_navigation::ConversationNavigationData;
use crate::test_util::ai_agent_tasks::{create_api_task, create_message};
use crate::workspace::WorkspaceRegistry;

type CapturedConversationUpdate = Mutex<Option<ConversationUpdateKind>>;

/// Test-only handler that mirrors the production view subscription: extracts the
/// `ConversationUpdated` payload and stashes it on a shared cell that test cases assert
/// against.
fn handle_agent_conversation_model_event(
    captured: &CapturedConversationUpdate,
    event: &AgentConversationsModelEvent,
) {
    if let AgentConversationsModelEvent::ConversationUpdated { kind } = event {
        *captured.lock() = Some(*kind);
    }
}

/// Subscribes a [`handle_agent_conversation_model_event`] capture cell to `model` and
/// returns the cell so individual cases can assert on the most recent emission without
/// re-implementing the subscription bookkeeping.
fn subscribe_to_conversation_updated(
    app: &mut App,
    model: &ModelHandle<AgentConversationsModel>,
) -> Arc<CapturedConversationUpdate> {
    let captured = Arc::new(Mutex::new(None));
    let captured_clone = captured.clone();
    app.update(|ctx| {
        ctx.subscribe_to_model(model, move |_, event, _| {
            handle_agent_conversation_model_event(&captured_clone, event);
        });
    });
    captured
}

#[test]
fn test_restored_conversation_emits_restored_kind() {
    App::test((), |mut app| async move {
        let _interactive_management_guard =
            FeatureFlag::InteractiveConversationManagementView.override_enabled(true);
        let agent_model = app.add_singleton_model(|_| create_test_model());
        let captured = subscribe_to_conversation_updated(&mut app, &agent_model);

        agent_model.update(&mut app, |model, ctx| {
            model.handle_history_event(
                &BlocklistAIHistoryEvent::UpdatedConversationStatus {
                    conversation_id: AIConversationId::new(),
                    terminal_surface_id: EntityId::new(),
                    update: ConversationStatusUpdate::Restored,
                    new_status: ConversationStatus::Success,
                },
                ctx,
            );
        });

        let captured = *captured.lock();
        assert_eq!(captured, Some(ConversationUpdateKind::Restored));
    });
}

#[test]
fn test_status_transition_emits_status_set_with_filter_buckets() {
    App::test((), |mut app| async move {
        let _interactive_management_guard =
            FeatureFlag::InteractiveConversationManagementView.override_enabled(true);
        let agent_model = app.add_singleton_model(|_| create_test_model());
        let captured = subscribe_to_conversation_updated(&mut app, &agent_model);

        agent_model.update(&mut app, |model, ctx| {
            model.handle_history_event(
                &BlocklistAIHistoryEvent::UpdatedConversationStatus {
                    conversation_id: AIConversationId::new(),
                    terminal_surface_id: EntityId::new(),
                    update: ConversationStatusUpdate::Changed {
                        prev_status: ConversationStatus::InProgress,
                    },
                    new_status: ConversationStatus::Success,
                },
                ctx,
            );
        });

        let captured = *captured.lock();
        assert_eq!(
            captured,
            Some(ConversationUpdateKind::StatusSet {
                prev_filter: StatusFilter::Working,
                new_filter: StatusFilter::Done,
            }),
        );
    });
}

#[test]
fn test_same_bucket_re_emission_emits_status_set_with_equal_filters() {
    App::test((), |mut app| async move {
        let _interactive_management_guard =
            FeatureFlag::InteractiveConversationManagementView.override_enabled(true);
        let agent_model = app.add_singleton_model(|_| create_test_model());
        let captured = subscribe_to_conversation_updated(&mut app, &agent_model);

        agent_model.update(&mut app, |model, ctx| {
            model.handle_history_event(
                &BlocklistAIHistoryEvent::UpdatedConversationStatus {
                    conversation_id: AIConversationId::new(),
                    terminal_surface_id: EntityId::new(),
                    update: ConversationStatusUpdate::Changed {
                        prev_status: ConversationStatus::InProgress,
                    },
                    new_status: ConversationStatus::InProgress,
                },
                ctx,
            );
        });

        let captured = *captured.lock();
        assert_eq!(
            captured,
            Some(ConversationUpdateKind::StatusSet {
                prev_filter: StatusFilter::Working,
                new_filter: StatusFilter::Working,
            }),
        );
    });
}

fn create_test_model() -> AgentConversationsModel {
    AgentConversationsModel {
        conversations: HashMap::new(),
    }
}

#[test]
fn conversation_query_caps_recent_entries_and_places_newest_last() {
    App::test((), |mut app| async move {
        add_entry_projection_test_models(&mut app);
        let mut model = create_test_model();
        for index in 0..55 {
            let mut metadata = create_test_conversation_metadata(
                AIConversationId::new(),
                &format!("Conversation {index}"),
            );
            metadata.nav_data.last_updated = (Utc::now() - Duration::seconds(index as i64)).into();
            model.conversations.insert(metadata.nav_data.id, metadata);
        }

        app.update(|ctx| {
            let entries = model.get_entries(ctx);
            let results = query_conversation_entries(entries, "");

            assert_eq!(results.len(), DEFAULT_RESULT_COUNT);
            assert_eq!(
                results
                    .first()
                    .map(|result| result.entry.display.title.as_str()),
                Some("Conversation 49")
            );
            assert_eq!(
                results
                    .last()
                    .map(|result| result.entry.display.title.as_str()),
                Some("Conversation 0")
            );
            assert!(
                !results
                    .iter()
                    .any(|result| result.entry.display.title == "Conversation 50")
            );
        });
    });
}

#[test]
fn conversation_query_filters_titles_and_caps_best_fuzzy_results() {
    App::test((), |mut app| async move {
        add_entry_projection_test_models(&mut app);
        let mut model = create_test_model();
        for index in 0..=MAX_SEARCH_RESULTS + 2 {
            let title = if index == 1 {
                "Fix unit tests".to_owned()
            } else {
                format!("Deploy service {index}")
            };
            let mut metadata = create_test_conversation_metadata(AIConversationId::new(), &title);
            metadata.nav_data.last_updated = (Utc::now() - Duration::seconds(index as i64)).into();
            model.conversations.insert(metadata.nav_data.id, metadata);
        }

        app.update(|ctx| {
            let entries = model.get_entries(ctx);
            let results = query_conversation_entries(entries, "deploy");

            assert_eq!(results.len(), MAX_SEARCH_RESULTS);
            assert!(
                results
                    .iter()
                    .all(|result| result.entry.display.title.contains("Deploy"))
            );
            assert!(results.windows(2).all(|window| {
                window[0].title_match.as_ref().unwrap().score
                    <= window[1].title_match.as_ref().unwrap().score
            }));
        });
    });
}

#[test]
fn conversation_query_orders_equal_fuzzy_scores_by_recency() {
    App::test((), |mut app| async move {
        add_entry_projection_test_models(&mut app);
        let mut model = create_test_model();
        for index in [0, 2, 1] {
            let mut metadata =
                create_test_conversation_metadata(AIConversationId::new(), "Deploy service");
            metadata.nav_data.last_updated = (Utc::now() - Duration::seconds(index as i64)).into();
            model.conversations.insert(metadata.nav_data.id, metadata);
        }

        app.update(|ctx| {
            let entries = model.get_entries(ctx);
            let results = query_conversation_entries(entries, "deploy");

            assert!(results.windows(2).all(|window| {
                window[0].entry.display.last_updated <= window[1].entry.display.last_updated
            }));
        });
    });
}

fn create_test_conversation_metadata(
    conversation_id: AIConversationId,
    title: &str,
) -> ConversationMetadata {
    ConversationMetadata {
        nav_data: ConversationNavigationData {
            id: conversation_id,
            title: title.to_string(),
            initial_query: None,
            last_updated: chrono::Local::now(),
            terminal_view_id: None,
            window_id: None,
            pane_view_locator: None,
            initial_working_directory: None,
            latest_working_directory: None,
            is_selected: false,
            is_in_active_pane: false,
            is_closed: false,
            server_conversation_token: None,
        },
    }
}

fn create_restored_conversation(
    conversation_id: AIConversationId,
    root_task_id: &str,
    conversation_data: AgentConversationData,
) -> AIConversation {
    let task = create_api_task(
        root_task_id,
        vec![create_message(
            &format!("{root_task_id}-message"),
            root_task_id,
        )],
    );

    AIConversation::new_restored(conversation_id, vec![task], Some(conversation_data))
        .expect("restored conversation should build")
}

fn add_entry_projection_test_models(app: &mut App) {
    app.add_singleton_model(|_| BlocklistAIHistoryModel::new(vec![], vec![], &[]));
    app.add_singleton_model(|_| ActiveAgentViewsModel::new());
    app.add_singleton_model(|_| WorkspaceRegistry::new());
}

#[test]
fn test_get_entries_includes_local_only_entry() {
    App::test((), |mut app| async move {
        add_entry_projection_test_models(&mut app);

        let conversation_id = AIConversationId::new();
        let mut model = create_test_model();
        model.conversations.insert(
            conversation_id,
            create_test_conversation_metadata(conversation_id, "Local conversation"),
        );

        app.update(|ctx| {
            let entries = model.get_entries(ctx);

            assert_eq!(entries.len(), 1);
            let entry = &entries[0];
            assert_eq!(
                entry.id,
                AgentConversationEntryId::Conversation(conversation_id)
            );
            assert_eq!(entry.identity.local_conversation_id, Some(conversation_id));
            assert_eq!(
                entry.provenance,
                AgentConversationProvenance::LocalInteractive
            );
            assert_eq!(entry.display.title, "Local conversation");
        });
    });
}

#[test]
fn test_conversation_metadata_child_predicate_matches_conversation() {
    use crate::ai::blocklist::history_model::AIConversationMetadata;

    // Non-child conversation: neither representation reports a child.
    let plain = AIConversation::new(false);
    let plain_metadata = AIConversationMetadata::from(&plain);
    assert!(!plain.is_child_agent_conversation());
    assert_eq!(
        plain_metadata.is_child_agent_conversation(),
        plain.is_child_agent_conversation()
    );

    // Child conversation: the metadata predicate matches the conversation's.
    let mut child = AIConversation::new(false);
    child.set_parent_conversation_id(AIConversationId::new());
    let child_metadata = AIConversationMetadata::from(&child);
    assert!(child.is_child_agent_conversation());
    assert_eq!(
        child_metadata.is_child_agent_conversation(),
        child.is_child_agent_conversation()
    );
}

#[test]
fn test_resolve_open_action_handles_server_token_subject_without_entry() {
    App::test((), |mut app| async move {
        add_entry_projection_test_models(&mut app);
        app.add_singleton_model(|_| create_test_model());

        let server_token = ServerConversationToken::new("server-token-subject".to_string());
        app.update(|ctx| {
            let action = AgentConversationsModel::resolve_open_action(
                AgentConversationNavigationSubject::ServerToken(server_token.clone()),
                None,
                ctx,
            );

            // A server token with no locally-resolvable conversation has no local open action.
            assert!(action.is_none());
        });
    })
}

#[test]
fn test_server_token_assignment_emits_conversation_updated() {
    App::test((), |mut app| async move {
        let _interactive_management_guard =
            FeatureFlag::InteractiveConversationManagementView.override_enabled(true);
        add_entry_projection_test_models(&mut app);

        let conversation_id = AIConversationId::new();
        let terminal_view_id = EntityId::new();
        let conversation = create_restored_conversation(
            conversation_id,
            "root-task",
            AgentConversationData {
                server_conversation_token: None,
                conversation_usage_metadata: None,
                reverted_action_ids: None,
                forked_from_server_conversation_token: None,
                artifacts_json: None,
                parent_agent_id: None,
                agent_name: None,
                orchestration_harness_type: None,
                parent_conversation_id: None,
                root_task_is_optimistic: None,
                run_id: None,
                autoexecute_override: None,
                last_event_sequence: None,
                pinned: false,
            },
        );

        BlocklistAIHistoryModel::handle(&app).update(&mut app, |model, ctx| {
            model.restore_conversations(terminal_view_id, vec![conversation], ctx);
        });

        let agent_model = app.add_singleton_model(|_| {
            let mut model = create_test_model();
            model.conversations.insert(
                conversation_id,
                create_test_conversation_metadata(conversation_id, "Conversation"),
            );
            model
        });
        let saw_conversation_updated = Arc::new(AtomicBool::new(false));

        app.update(|ctx| {
            let saw_conversation_updated = saw_conversation_updated.clone();
            ctx.subscribe_to_model(&agent_model, move |_, event, _| {
                if matches!(
                    event,
                    AgentConversationsModelEvent::ConversationUpdated { .. }
                ) {
                    saw_conversation_updated.store(true, Ordering::SeqCst);
                }
            });
        });

        let token = "assigned-token-after-entry-build";
        BlocklistAIHistoryModel::handle(&app).update(&mut app, |model, _| {
            model
                .set_server_conversation_token_for_conversation(conversation_id, token.to_string());
        });
        agent_model.update(&mut app, |model, ctx| {
            model.handle_history_event(
                &BlocklistAIHistoryEvent::ConversationServerTokenAssigned {
                    conversation_id,
                    terminal_surface_id: terminal_view_id,
                },
                ctx,
            );
        });

        app.update(|_ctx| {
            assert!(saw_conversation_updated.load(Ordering::SeqCst));
        });
    });
}
