use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ai::index::locations::CodeContextLocation;
use futures_util::stream::AbortHandle;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity as _};

use crate::ai::agent::{AIAgentActionId, SearchCodebaseResult};
use crate::ai::blocklist::SessionContext;
use crate::ai::outline::{OutlineStatus, RepoOutlines};
#[cfg_attr(not(target_family = "wasm"), path = "remote_search/native.rs")]
#[cfg_attr(target_family = "wasm", path = "remote_search/wasm.rs")]
mod remote_search;

#[derive(Debug)]
pub enum GetRelevantFilesControllerEvent {
    Success {
        action_id: AIAgentActionId,
        result: GetRelevantFilesControllerResult,
    },
    Error {
        action_id: AIAgentActionId,
    },
}

impl GetRelevantFilesControllerEvent {
    pub fn action_id(&self) -> &AIAgentActionId {
        match self {
            GetRelevantFilesControllerEvent::Success { action_id, .. } => action_id,
            GetRelevantFilesControllerEvent::Error { action_id } => action_id,
        }
    }
}

#[derive(Debug)]
pub enum GetRelevantFilesControllerResult {
    Locations(Arc<HashSet<CodeContextLocation>>),
    SearchResult(SearchCodebaseResult),
}

pub enum GetRelevantFilesRequestTarget {
    Local {
        directory: PathBuf,
    },
    Remote {
        session_context: SessionContext,
        requested_codebase_path: Option<String>,
    },
}
#[derive(Debug, thiserror::Error)]
pub enum GetRelevantFilesError {
    #[error("Repo outline is still being computed.")]
    Pending,
    #[error("Failed to create outline.")]
    CreateFailed,
    #[error("Failed to create outline.")]
    Missing,
}

/// Controller for GetRelevantFiles action. This is scoped per terminal session.
#[derive(Default)]
pub struct GetRelevantFilesController {
    /// Search requests currently in flight, keyed by the originating action ID.
    /// This allows several SearchCodebase actions to be active at once without newer requests
    /// cancelling unrelated older ones.
    pending_requests: std::collections::HashMap<AIAgentActionId, AbortHandle>,
}

impl GetRelevantFilesController {
    pub fn new(_ctx: &mut ModelContext<Self>) -> Self {
        Self::default()
    }

    /// Start a new search query based on the repo outline.
    pub fn send_request(
        &mut self,
        target: GetRelevantFilesRequestTarget,
        query: String,
        partial_path_segments: Option<&Vec<String>>,
        action_id: AIAgentActionId,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), GetRelevantFilesError> {
        // Cancel any previous request for this action before dispatching to either the local or
        // remote implementation.
        self.cancel_request_for_action(&action_id, ctx);
        match target {
            GetRelevantFilesRequestTarget::Local { directory } => {
                self.send_local_request(&directory, partial_path_segments, action_id, ctx)
            }
            GetRelevantFilesRequestTarget::Remote {
                session_context,
                requested_codebase_path,
            } => self.send_remote_request(
                session_context,
                requested_codebase_path,
                query,
                partial_path_segments.cloned(),
                action_id,
                ctx,
            ),
        }
    }

    fn send_local_request(
        &mut self,
        directory: &Path,
        partial_path_segments: Option<&Vec<String>>,
        action_id: AIAgentActionId,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), GetRelevantFilesError> {
        const WHOLE_REPO_SUGGESTION_FILE_LIMIT: usize = 2;

        match RepoOutlines::as_ref(ctx).get_outline(directory) {
            Some((OutlineStatus::Complete(outline), _)) => {
                let file_outlines = outline.to_file_symbols(partial_path_segments);
                if file_outlines.len() < WHOLE_REPO_SUGGESTION_FILE_LIMIT {
                    ctx.emit(GetRelevantFilesControllerEvent::Success {
                        action_id,
                        result: GetRelevantFilesControllerResult::Locations(Arc::new(
                            file_outlines
                                .into_iter()
                                .map(|file| {
                                    CodeContextLocation::WholeFile(PathBuf::from(file.path))
                                })
                                .collect(),
                        )),
                    });
                } else {
                    // Ranking files within a larger outline ran on Warp's server, which
                    // no longer exists; the search can only fail, so report that without
                    // opening a connection.
                    ctx.emit(GetRelevantFilesControllerEvent::Error { action_id });
                }
                Ok(())
            }
            Some((OutlineStatus::Pending, _)) => Err(GetRelevantFilesError::Pending),
            Some((OutlineStatus::Failed, _)) => Err(GetRelevantFilesError::CreateFailed),
            None => Err(GetRelevantFilesError::Missing),
        }
    }

    fn send_remote_request(
        &mut self,
        session_context: SessionContext,
        requested_codebase_path: Option<String>,
        query: String,
        partial_path_segments: Option<Vec<String>>,
        action_id: AIAgentActionId,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), GetRelevantFilesError> {
        let remote_search::RemoteSearchRequest::Ready(result) = remote_search::send_request(
            query,
            partial_path_segments,
            session_context,
            requested_codebase_path,
            action_id.clone(),
            ctx,
        );
        ctx.emit(GetRelevantFilesControllerEvent::Success {
            action_id,
            result: GetRelevantFilesControllerResult::SearchResult(result),
        });
        Ok(())
    }

    /// Returns the path to the root directory for a codebase search where pwd is `directory`.
    pub fn root_directory_for_search(&self, directory: &Path, app: &AppContext) -> Option<PathBuf> {
        RepoOutlines::as_ref(app)
            .get_outline(directory)
            .map(|(_, root)| root)
    }

    pub fn root_directory_for_remote_search(
        &self,
        session_context: &SessionContext,
        requested_codebase_path: Option<&str>,
        app: &AppContext,
    ) -> Option<PathBuf> {
        remote_search::root_directory_for_search(session_context, requested_codebase_path, app)
    }

    pub fn cancel_request_for_action(
        &mut self,
        action_id: &AIAgentActionId,
        _ctx: &mut ModelContext<Self>,
    ) {
        if let Some(abort_handle) = self.pending_requests.remove(action_id) {
            abort_handle.abort();
        }
    }
}

impl Entity for GetRelevantFilesController {
    type Event = GetRelevantFilesControllerEvent;
}
