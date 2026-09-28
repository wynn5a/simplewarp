use warpui::{App, EntityId};

use super::*;
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::agent::task::TaskId;
use crate::ai::agent::{
    AIAgentAction, AIAgentActionId, AIAgentActionResultType, AIAgentActionType,
    ReadDocumentsRequest, ReadDocumentsResult,
};
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::ai::document::ai_document_model::{AIDocumentId, AIDocumentModel};
use crate::appearance::Appearance;
use crate::cloud_object::model::persistence::CloudModel;
use crate::test_util::settings::initialize_settings_for_tests;

fn initialize_app(app: &mut App) {
    initialize_settings_for_tests(app);
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(|_| CloudModel::new(None, Vec::new()));
    app.add_singleton_model(|_| AIDocumentModel::new_for_test());
    app.add_singleton_model(|_| BlocklistAIHistoryModel::new_for_test());
}

fn read_action(document_id: AIDocumentId) -> AIAgentAction {
    AIAgentAction {
        id: AIAgentActionId::from("read-documents-action".to_string()),
        task_id: TaskId::new("read-documents-task".to_string()),
        requires_result: true,
        action: AIAgentActionType::ReadDocuments(ReadDocumentsRequest {
            document_ids: vec![document_id],
        }),
    }
}

/// Local child agents read their parent's plans straight from the shared document model.
#[test]
fn execute_reads_plan_owned_by_another_conversation() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let executor = app.add_model(|_| ReadDocumentsExecutor::new());
        let document_id = AIDocumentModel::handle(&app).update(&mut app, |model, ctx| {
            model.create_document(
                "Parent plan",
                "# Parent plan",
                AIConversationId::new(),
                None,
                ctx,
            )
        });
        let child_conversation_id = BlocklistAIHistoryModel::handle(&app)
            .update(&mut app, |history, ctx| {
                history.start_new_conversation(EntityId::new(), false, false, ctx)
            });
        let action = read_action(document_id);

        let execution: AnyActionExecution = executor.update(&mut app, |executor, ctx| {
            executor
                .execute(
                    ExecuteActionInput {
                        action: &action,
                        conversation_id: child_conversation_id,
                    },
                    ctx,
                )
                .into()
        });

        let AnyActionExecution::Sync(AIAgentActionResultType::ReadDocuments(
            ReadDocumentsResult::Success { documents },
        )) = execution
        else {
            panic!("expected read_documents success");
        };
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].document_id, document_id);
        assert_eq!(documents[0].content, "# Parent plan\n");
    });
}

#[test]
fn execute_returns_error_for_missing_document_id() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let executor = app.add_model(|_| ReadDocumentsExecutor::new());
        let missing_document_id = AIDocumentId::new();
        let action = read_action(missing_document_id);

        let execution: AnyActionExecution = executor.update(&mut app, |executor, ctx| {
            executor
                .execute(
                    ExecuteActionInput {
                        action: &action,
                        conversation_id: AIConversationId::new(),
                    },
                    ctx,
                )
                .into()
        });

        let AnyActionExecution::Sync(AIAgentActionResultType::ReadDocuments(
            ReadDocumentsResult::Error(error),
        )) = execution
        else {
            panic!("expected read_documents error");
        };
        assert!(error.contains(&missing_document_id.to_string()));
    });
}
