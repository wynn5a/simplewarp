use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ai::index::locations::CodeContextLocation;
use futures_util::stream::AbortHandle;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity as _};

use crate::ai::agent::AIAgentActionId;
use crate::ai::outline::{OutlineStatus, RepoOutlines};

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

    /// Start a new search query based on the repo outline of `directory`.
    pub fn send_request(
        &mut self,
        directory: &Path,
        partial_path_segments: Option<&Vec<String>>,
        action_id: AIAgentActionId,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), GetRelevantFilesError> {
        // Cancel any previous request for this action.
        self.cancel_request_for_action(&action_id, ctx);
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

    /// Returns the path to the root directory for a codebase search where pwd is `directory`.
    pub fn root_directory_for_search(&self, directory: &Path, app: &AppContext) -> Option<PathBuf> {
        RepoOutlines::as_ref(app)
            .get_outline(directory)
            .map(|(_, root)| root)
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
