use futures::FutureExt;
use futures::future::BoxFuture;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use super::{ActionExecution, AnyActionExecution, ExecuteActionInput, PreprocessActionInput};
use crate::ai::agent::{
    AIAgentAction, AIAgentActionType, DocumentContext, ReadDocumentsRequest, ReadDocumentsResult,
};
use crate::ai::document::ai_document_model::{AIDocumentId, AIDocumentModel};

pub struct ReadDocumentsExecutor;

impl ReadDocumentsExecutor {
    pub fn new() -> Self {
        Self
    }

    pub(super) fn should_autoexecute(
        &self,
        _input: ExecuteActionInput,
        _ctx: &mut ModelContext<Self>,
    ) -> bool {
        // Document operations are always auto-executed
        true
    }

    pub(super) fn execute(
        &mut self,
        input: ExecuteActionInput,
        ctx: &mut ModelContext<Self>,
    ) -> impl Into<AnyActionExecution> + use<> {
        let AIAgentAction {
            action: AIAgentActionType::ReadDocuments(ReadDocumentsRequest { document_ids }),
            ..
        } = input.action
        else {
            return ActionExecution::<ReadDocumentsResult>::InvalidAction;
        };

        let mut documents = Vec::with_capacity(document_ids.len());
        let mut missing_documents = Vec::new();
        for id in document_ids {
            match try_read_document(id, ctx) {
                Some(document) => documents.push(document),
                None => missing_documents.push(*id),
            }
        }

        if !missing_documents.is_empty() {
            let missing_list = missing_documents
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            return ActionExecution::Sync(
                ReadDocumentsResult::Error(format!("Document(s) not found: {missing_list}")).into(),
            );
        }

        ActionExecution::Sync(ReadDocumentsResult::Success { documents }.into())
    }

    pub(super) fn preprocess_action(
        &mut self,
        _input: PreprocessActionInput,
        _ctx: &mut ModelContext<Self>,
    ) -> BoxFuture<'static, ()> {
        futures::future::ready(()).boxed()
    }
}

impl Entity for ReadDocumentsExecutor {
    type Event = ();
}

/// Reads one document, returning `None` if it is not loaded locally.
fn try_read_document(id: &AIDocumentId, ctx: &AppContext) -> Option<DocumentContext> {
    let model = AIDocumentModel::as_ref(ctx);
    let content = model.get_document_content(id, ctx)?;
    let version = model.get_current_document(id)?.version;
    Some(DocumentContext {
        document_id: *id,
        content,
        line_ranges: vec![],
        document_version: version,
    })
}

#[cfg(test)]
#[path = "read_documents_tests.rs"]
mod tests;
