//! Shared git "action" orchestration: the commit-chain, push, create-PR, and
//! view-PR workflows behind the code-review git buttons.
//!
//! These compose the single-command primitives in [`crate::util::git`]
//! into the end-to-end actions a button triggers.
//! They are intentionally backend-agnostic: the local code-review dialog and
//! the remote-server daemon both call them, so local and remote behave
//! identically. Git ops are host-scoped and not tied to a diff-state model, so
//! this logic lives here rather than on a model.
//!
//! Callers own everything *around* the action: UI (toasts, telemetry, dialog
//! lifecycle), transport/model (applying the returned delta to a
//! `DiffStateModel`, building wire responses), and any execution-time guards
//! (e.g. the daemon's `git_operation_in_progress` backstop).

use std::path::Path;

use crate::code_review::diff_state::CommitChainMode;
use crate::util::git::{self, Commit, PrInfo};

/// Runs the commit chain — always commits, then optionally pushes, then
/// optionally creates a PR per `mode` — and returns the post-chain delta
/// (refreshed unpushed commits + upstream ref) plus any created PR. The delta
/// is computed once after the whole chain settles.
///
/// When the chain creates a PR, the title/body come from `gh pr create
/// --fill`; server-side AI generation is gone, so there is no AI path.
pub async fn run_commit_chain(
    repo_path: &Path,
    mode: CommitChainMode,
    message: &str,
    include_unstaged: bool,
    branch: &str,
    path_env: Option<&str>,
) -> anyhow::Result<(Vec<Commit>, Option<String>, Option<PrInfo>)> {
    git::run_commit(repo_path, message, include_unstaged, path_env).await?;
    let pr_info = match mode {
        CommitChainMode::CommitOnly => None,
        CommitChainMode::CommitAndPush => {
            git::run_push(repo_path, branch, path_env).await?;
            None
        }
        CommitChainMode::CommitAndCreatePr => {
            git::run_push(repo_path, branch, path_env).await?;
            Some(create_pr(repo_path, path_env).await?)
        }
    };
    let (commits, upstream_ref) = git::compute_unpushed_state(repo_path).await;
    Ok((commits, upstream_ref, pr_info))
}

/// Pushes `branch` (setting upstream) and returns the refreshed
/// unpushed/upstream delta.
pub async fn run_push(
    repo_path: &Path,
    branch: &str,
    path_env: Option<&str>,
) -> anyhow::Result<(Vec<Commit>, Option<String>)> {
    git::run_push(repo_path, branch, path_env).await?;
    Ok(git::compute_unpushed_state(repo_path).await)
}

/// Creates a PR with `gh pr create --fill`.
pub async fn create_pr(repo_path: &Path, path_env: Option<&str>) -> anyhow::Result<PrInfo> {
    git::create_pr(repo_path, None, None, path_env).await
}

/// Validates that the working tree has changes to summarize. AI commit-message
/// generation is gone (the server wall fell with the cloud-run round), so this
/// always reports unavailability and the dialog keeps its manual placeholder.
pub async fn generate_commit_message(
    repo_path: &Path,
    include_unstaged: bool,
) -> anyhow::Result<String> {
    let diff = git::get_diff_for_commit_message(repo_path, include_unstaged).await?;
    // Skip the round trip when there's nothing to summarize.
    if diff.trim().is_empty() {
        anyhow::bail!("no changes to generate a commit message from");
    }
    anyhow::bail!("AI commit message generation is unavailable in this build")
}
