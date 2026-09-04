use std::path::PathBuf;

use warpui::{AppContext, ModelContext};

use crate::ai::agent::{SearchCodebaseFailureReason, SearchCodebaseResult};
use crate::ai::blocklist::SessionContext;
use crate::ai::get_relevant_files::controller::GetRelevantFilesController;

pub(super) enum RemoteSearchRequest {
    Ready(SearchCodebaseResult),
}

pub(super) fn root_directory_for_search(
    session_context: &SessionContext,
    requested_codebase_path: Option<&str>,
    _app: &AppContext,
) -> Option<PathBuf> {
    requested_codebase_path
        .is_none()
        .then(|| session_context.current_working_directory().clone())
        .flatten()
        .map(PathBuf::from)
}

pub(super) fn send_request(
    _query: String,
    _partial_paths: Option<Vec<String>>,
    _session_context: SessionContext,
    _requested_codebase_path: Option<String>,
    _action_id: crate::ai::agent::AIAgentActionId,
    _ctx: &mut ModelContext<GetRelevantFilesController>,
) -> RemoteSearchRequest {
    RemoteSearchRequest::Ready(SearchCodebaseResult::Failed {
        reason: SearchCodebaseFailureReason::CodebaseNotIndexed,
        message: "Remote codebase search is not enabled.".to_string(),
    })
}
