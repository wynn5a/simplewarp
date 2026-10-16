use std::collections::HashMap;

use persistence::model::{AgentConversationData, ConversationTokenUsage};
use warp_multi_agent_api as api;
use warpui::{App, EntityId, SingletonEntity};

use super::{ConversationDetailsData, PanelMode, format_token_count, format_token_usage};
use crate::ai::agent::conversation::{AIConversation, AIConversationId};
use crate::ai::blocklist::history_model::BlocklistAIHistoryModel;

fn create_message_with_directory(id: &str, task_id: &str, directory: &str) -> api::Message {
    api::Message {
        fetched_memories: vec![],
        id: id.to_string(),
        task_id: task_id.to_string(),
        server_message_data: String::new(),
        citations: vec![],
        message: Some(api::message::Message::UserQuery(api::message::UserQuery {
            query: "test query".to_string(),
            context: Some(api::InputContext {
                directory: Some(api::input_context::Directory {
                    pwd: directory.to_string(),
                    home: String::new(),
                    pwd_file_symbols_indexed: false,
                }),
                ..Default::default()
            }),
            referenced_attachments: HashMap::new(),
            mode: None,
            intended_agent: Default::default(),
        })),
        request_id: "request-1".to_string(),
        timestamp: None,
    }
}

fn create_agent_output_message(id: &str, task_id: &str) -> api::Message {
    api::Message {
        fetched_memories: vec![],
        id: id.to_string(),
        task_id: task_id.to_string(),
        server_message_data: String::new(),
        citations: vec![],
        message: Some(api::message::Message::AgentOutput(
            api::message::AgentOutput {
                text: "done".to_string(),
            },
        )),
        request_id: "request-1".to_string(),
        timestamp: None,
    }
}

fn create_restored_conversation(
    conversation_id: AIConversationId,
    root_task_id: &str,
    directory: &str,
    conversation_data: AgentConversationData,
) -> AIConversation {
    let task = api::Task {
        id: root_task_id.to_string(),
        messages: vec![
            create_message_with_directory("message-1", root_task_id, directory),
            create_agent_output_message("message-2", root_task_id),
        ],
        dependencies: None,
        description: String::new(),
        summary: String::new(),
        server_data: String::new(),
    };

    AIConversation::new_restored(conversation_id, vec![task], Some(conversation_data))
        .expect("restored conversation should build")
}

#[test]
fn test_from_conversation_populates_local_conversation_fields() {
    // Locks in that `ConversationDetailsData::from_conversation` works on native
    // and surfaces the conversation-derived fields the conversation details panel
    // renders for local Warp Agent runs (APP-3595).
    App::test((), |mut app| async move {
        let history_model =
            app.add_singleton_model(|_| BlocklistAIHistoryModel::new(vec![], vec![], &[]));

        let conversation_id = AIConversationId::new();
        let directory = "/tmp/local-conversation-directory";
        let conversation = create_restored_conversation(
            conversation_id,
            "root-task",
            directory,
            AgentConversationData {
                server_conversation_token: None,
                reverted_action_ids: None,
                forked_from_server_conversation_token: None,
                artifacts_json: None,
                autoexecute_override: None,
                root_task_is_optimistic: None,
                token_usage: None,
            },
        );

        history_model.update(&mut app, |model, ctx| {
            model.restore_conversations(EntityId::new(), vec![conversation], ctx);
        });

        app.update(|ctx| {
            let conversation = BlocklistAIHistoryModel::as_ref(ctx)
                .conversation(&conversation_id)
                .expect("conversation should be present");
            let data = ConversationDetailsData::from_conversation(conversation);

            // Mode should be Conversation with the working directory.
            let PanelMode::Conversation {
                directory: panel_directory,
                status,
            } = &data.mode;
            assert_eq!(panel_directory.as_deref(), Some(directory));
            assert!(status.is_some());

            assert_eq!(data.title, "test query");
            assert_eq!(data.source_prompt.as_deref(), Some("test query"));
        });
    });
}

#[test]
fn test_from_conversation_carries_restored_token_usage() {
    let usage = ConversationTokenUsage {
        input: 12_345,
        cached_input: 8_000,
        output: 1_100,
    };
    let conversation = create_restored_conversation(
        AIConversationId::new(),
        "root-task",
        "/tmp",
        AgentConversationData {
            server_conversation_token: None,
            reverted_action_ids: None,
            forked_from_server_conversation_token: None,
            artifacts_json: None,
            autoexecute_override: None,
            root_task_is_optimistic: None,
            token_usage: Some(usage),
        },
    );

    let data = ConversationDetailsData::from_conversation(&conversation);

    assert_eq!(data.token_usage, Some(usage));
}

#[test]
fn test_token_usage_formatting() {
    assert_eq!(
        format_token_usage(ConversationTokenUsage {
            input: 12_345,
            cached_input: 8_000,
            output: 1_100,
        }),
        "12.3k in (8.0k cached) · 1.1k out"
    );
    assert_eq!(
        format_token_usage(ConversationTokenUsage {
            input: 999,
            cached_input: 0,
            output: 2_500_000,
        }),
        "999 in · 2.5M out"
    );
    assert_eq!(format_token_count(999_949), "999.9k");
    assert_eq!(format_token_count(999_950), "1.0M");
}
