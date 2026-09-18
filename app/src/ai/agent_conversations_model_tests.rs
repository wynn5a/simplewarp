use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{Duration, Utc};
use parking_lot::Mutex;
use persistence::model::AgentConversationData;
use warp_cli::agent::Harness;
use warp_core::features::FeatureFlag;
use warpui::{App, EntityId, ModelHandle, SingletonEntity};

use super::entry::{
    AgentConversationEntryId, AgentConversationNavigationSubject, AgentConversationProvenance,
};
use super::query::{DEFAULT_RESULT_COUNT, MAX_SEARCH_RESULTS};
use super::{
    AgentConversationsModel, AgentConversationsModelEvent, AgentManagementFilters, ArtifactFilter,
    ConversationMetadata, ConversationUpdateKind, HarnessFilter, OwnerFilter, StatusFilter,
    query_conversation_entries,
};
use crate::ai::active_agent_views_model::ActiveAgentViewsModel;
use crate::ai::agent::api::ServerConversationToken;
use crate::ai::agent::conversation::{AIConversation, AIConversationId, ConversationStatus};
use crate::ai::artifacts::Artifact;
use crate::ai::blocklist::history_model::{
    BlocklistAIHistoryEvent, BlocklistAIHistoryModel, ConversationStatusUpdate,
};
use crate::ai::conversation_navigation::ConversationNavigationData;
use crate::auth::AuthStateProvider;
use crate::test_util::ai_agent_tasks::{create_api_task, create_message};
use crate::workspace::{WorkspaceAction, WorkspaceRegistry};

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
            let entries = model.get_entries(&all_owner_filters(), ctx);
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
            let entries = model.get_entries(&all_owner_filters(), ctx);
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
            let entries = model.get_entries(&all_owner_filters(), ctx);
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

fn all_owner_filters() -> AgentManagementFilters {
    AgentManagementFilters {
        owners: OwnerFilter::All,
        ..Default::default()
    }
}

fn add_entry_projection_test_models(app: &mut App) {
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
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
            let entries = model.get_entries(&all_owner_filters(), ctx);

            assert_eq!(entries.len(), 1);
            let entry = &entries[0];
            assert_eq!(
                entry.id,
                AgentConversationEntryId::Conversation(conversation_id)
            );
            assert_eq!(entry.identity.local_conversation_id, Some(conversation_id));
            assert_eq!(entry.identity.ambient_agent_task_id, None);
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
    let plain = AIConversation::new(false, false);
    let plain_metadata = AIConversationMetadata::from(&plain);
    assert!(!plain.is_child_agent_conversation());
    assert_eq!(
        plain_metadata.is_child_agent_conversation(),
        plain.is_child_agent_conversation()
    );

    // Child conversation: the metadata predicate matches the conversation's.
    let mut child = AIConversation::new(false, false);
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

            assert!(matches!(
                action,
                Some(WorkspaceAction::OpenConversationTranscriptViewer {
                    conversation_id,
                    ambient_agent_task_id: None,
                }) if conversation_id == server_token
            ));
        });
    });
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
                is_remote_child: false,
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

#[test]
fn test_file_artifact_filter_matches_only_items_with_file_artifacts() {
    let artifacts_with_file = vec![Artifact::File {
        artifact_uid: "artifact-file-1".to_string(),
        filepath: "outputs/report.txt".to_string(),
        filename: "report.txt".to_string(),
        mime_type: "text/plain".to_string(),
        description: Some("Daily summary".to_string()),
        size_bytes: Some(42),
    }];
    let artifacts_with_pr = vec![Artifact::PullRequest {
        url: "https://github.com/org/repo/pull/1".to_string(),
        branch: "main".to_string(),
        repo: Some("repo".to_string()),
        number: Some(1),
    }];

    assert!(super::artifacts_match_filter(
        &artifacts_with_file,
        &ArtifactFilter::File,
    ));
    assert!(!super::artifacts_match_filter(
        &artifacts_with_pr,
        &ArtifactFilter::File,
    ));
    assert!(super::artifacts_match_filter(
        &artifacts_with_file,
        &ArtifactFilter::All,
    ));
}

#[test]
fn test_harness_filter_is_filtering_and_reset() {
    // Default is All → not filtering, and after toggling reset_all_but_owner returns to default.
    let mut filters = AgentManagementFilters::default();
    assert!(!filters.is_filtering());

    filters.harness = HarnessFilter::Specific(Harness::Claude);
    assert!(
        filters.is_filtering(),
        "harness != All should report filtering"
    );

    filters.reset_all_but_owner();
    assert_eq!(filters.harness, HarnessFilter::default());
    assert!(!filters.is_filtering());
}

#[test]
fn test_agent_management_filters_serde_backwards_compat() {
    // Persisted state from older clients has no `harness` key → deserializes to All.
    let legacy = r#"{
        "owners": "PersonalOnly",
        "status": "All",
        "source": "All",
        "created_on": "All",
        "creator": "All",
        "artifact": "All"
    }"#;
    let decoded: AgentManagementFilters =
        serde_json::from_str(legacy).expect("legacy payload without harness must deserialize");
    assert_eq!(decoded.harness, HarnessFilter::All);

    // Round trip a Specific(Claude) value.
    let original = AgentManagementFilters {
        harness: HarnessFilter::Specific(Harness::Claude),
        ..Default::default()
    };
    let encoded = serde_json::to_string(&original).unwrap();
    assert!(
        encoded.contains("\"harness\":\"claude\""),
        "expected serialized form to contain \"harness\":\"claude\", got {encoded}"
    );
    let decoded: AgentManagementFilters = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, original);

    // Unknown harness strings deserialize to All (forward compat).
    let forward = r#"{
        "owners": "PersonalOnly",
        "status": "All",
        "source": "All",
        "created_on": "All",
        "creator": "All",
        "artifact": "All",
        "harness": "some-future-harness"
    }"#;
    let decoded: AgentManagementFilters = serde_json::from_str(forward).unwrap();
    assert_eq!(decoded.harness, HarnessFilter::All);
}
