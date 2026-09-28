mod model;
pub use model::GitRepoStatusModel;

use super::diff_state::DiffStats;
pub use super::git_repo_models::GitRepoModels;
use crate::context_chips::display_chip::GitBranchTrackingStatus;

/// Public metadata exposed to consumers — the subset of diff metadata
/// that the git chip (prompt display, agent view footer) needs.
#[derive(Debug, Clone)]
pub struct GitStatusMetadata {
    pub current_branch_name: String,
    pub main_branch_name: String,
    pub stats_against_head: DiffStats,
    pub branch_tracking_status: GitBranchTrackingStatus,
}

// ── GitRepoStatusModel ──────────────────────────────────────────────────────

#[derive(Debug)]
pub enum GitRepoStatusEvent {
    /// Emitted whenever the metadata changes (branch name, diff stats, etc.).
    MetadataChanged,
}
