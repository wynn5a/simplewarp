//! Touched-workspace derivation for local-to-cloud handoff (REMOTE-1486).
//!
//! Given the flat list of filesystem paths an agent run has touched and
//! the user's currently-known cloud agent environments, this module produces:
//!
//! 1. A [`TouchedWorkspace`] enumerating the distinct git repos and orphan files the
//!    local agent has touched. Each repo carries a parsed `repo_id` (`<owner>/<repo>`)
//!    derived from its `origin` remote URL, fetched via an async `git` invocation so
//!    derivation never blocks the UI thread.
//! 3. A repo-aware default environment selection that layers on top of the existing
//!    cloud-agent setup recency-sort.
//!
//! Path extraction is sync and pure (no I/O), and the workspace derivation is async
//! (one `git remote get-url origin` per unique repo). Callers run them in sequence
//! off the main thread; see `app/src/workspace/view.rs::start_local_to_cloud_handoff`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use command::Stdio;
use command::r#async::Command;
use futures::future::join_all;
use tokio::fs as tokio_fs;
use warpui::AppContext;
use warpui::r#async::FutureExt as _;

use crate::ai::cloud_environments::{
    CloudAmbientAgentEnvironment, GithubRepo, sort_environments_by_recency,
};
use crate::cloud_object::CloudObjectLookup as _;
use crate::server::ids::SyncId;

/// Soft cap on each git invocation we dispatch. Mirrors the cap used by the cloud-side
/// snapshot pipeline so individual filesystem hiccups don't stall the modal indefinitely.
const GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// The collection of git repos and orphan files the local agent has touched in the
/// active conversation. Drives both the snapshot upload plan and the modal's env-
/// overlap status row.
#[derive(Clone, Debug, Default)]
pub(crate) struct TouchedWorkspace {
    pub repos: Vec<TouchedRepo>,
    /// Files touched outside any `.git` directory.
    /// They're captured as raw file contents in the snapshot manifest.
    pub orphan_files: Vec<PathBuf>,
}

/// A single git repo touched by the local agent.
#[derive(Clone, Debug)]
pub(crate) struct TouchedRepo {
    /// Absolute path to the working tree root (the directory containing `.git`).
    pub git_root: PathBuf,
    /// `<owner>/<repo>` parsed from the `origin` remote URL, when discoverable.
    /// Drives env-overlap matching against `CloudAmbientAgentEnvironment.github_repos`
    /// and the modal's per-repo status row label.
    pub repo_id: Option<GithubRepo>,
}

/// Derive the `TouchedWorkspace` from a flat list of absolute paths.
///
/// Walks each path up to the nearest `.git` directory; paths whose walk-up doesn't
/// find one go into `orphan_files`. For each unique git root, runs
/// `git remote get-url origin` to parse out the `<owner>/<repo>` for env-overlap
/// matching. Errors on the git call are non-fatal — `repo_id` stays `None`.
///
/// `paths` must already be absolute and must come from
/// [`extract_paths_from_conversation`], which only emits paths the agent
/// actually wrote to (plus per-exchange cwds for repo discovery). That gate
/// is what makes the orphan-file branch safe — we never stage a read-only
/// path like `~/.ssh/id_rsa` for upload.
pub(crate) async fn derive_touched_workspace(paths: Vec<PathBuf>) -> TouchedWorkspace {
    if paths.is_empty() {
        return TouchedWorkspace::default();
    }

    let mut git_roots: Vec<PathBuf> = Vec::new();
    let mut orphan_files: Vec<PathBuf> = Vec::new();
    let mut seen_roots: HashSet<PathBuf> = HashSet::new();

    for path in paths {
        match find_git_root(&path).await {
            Some(root) => {
                if seen_roots.insert(root.clone()) {
                    git_roots.push(root);
                }
            }
            None => {
                if tokio_fs::metadata(&path).await.is_ok_and(|m| m.is_file()) {
                    orphan_files.push(path);
                }
            }
        }
    }

    let metadata_futures = git_roots.into_iter().map(|git_root| async move {
        let repo_id = git_origin_url(&git_root)
            .await
            .as_deref()
            .and_then(parse_github_repo);
        TouchedRepo { git_root, repo_id }
    });
    let repos: Vec<TouchedRepo> = join_all(metadata_futures).await;

    TouchedWorkspace {
        repos,
        orphan_files,
    }
}

/// Walk `path` up to find the nearest enclosing `.git` directory and return its parent
/// (the working-tree root). Returns `None` if no `.git` is found.
async fn find_git_root(path: &Path) -> Option<PathBuf> {
    let mut cursor: Option<&Path> = if tokio_fs::metadata(path).await.is_ok_and(|m| m.is_dir()) {
        Some(path)
    } else {
        path.parent()
    };
    while let Some(dir) = cursor {
        let candidate = dir.join(".git");
        if tokio_fs::try_exists(&candidate).await.unwrap_or(false) {
            return Some(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    None
}

/// Run `git remote get-url origin` in `git_root` with a bounded timeout, returning the
/// trimmed remote URL or `None` if the invocation fails, times out, exits non-zero, or
/// yields empty/non-UTF-8 output. [`GIT_COMMAND_TIMEOUT`] caps the call so a stalled git
/// process can't pin the loading state forever.
async fn git_origin_url(git_root: &Path) -> Option<String> {
    let mut command = Command::new("git");
    command
        .args(["remote", "get-url", "origin"])
        .current_dir(git_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let Ok(Ok(output)) = command.output().with_timeout(GIT_COMMAND_TIMEOUT).await else {
        return None;
    };
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Parse a GitHub remote URL of either the SSH (`git@github.com:owner/repo.git`) or
/// HTTPS (`https://github.com/owner/repo[.git]`) flavor into a [`GithubRepo`].
/// Returns `None` for non-GitHub remotes (we only support env-overlap for GitHub today,
/// matching the env-creation flow).
fn parse_github_repo(remote_url: &str) -> Option<GithubRepo> {
    let trimmed = remote_url.trim();
    let path_part = if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("ssh://git@github.com/") {
        rest
    } else {
        return None;
    };

    let path_part = path_part.strip_suffix(".git").unwrap_or(path_part);
    let mut segments = path_part.splitn(2, '/');
    let owner = segments.next()?.to_string();
    let repo = segments.next()?.to_string();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(GithubRepo::new(owner, repo))
}

/// Resolve a single directory path to its enclosing git repo and parsed GitHub
/// remote, if any.
pub(crate) async fn resolve_repo_for_path(path: &Path) -> Option<TouchedRepo> {
    let git_root = find_git_root(path).await?;
    let repo_id = git_origin_url(&git_root)
        .await
        .as_deref()
        .and_then(parse_github_repo);
    Some(TouchedRepo { git_root, repo_id })
}

/// Suggests the available environment whose configured repositories overlap
/// the Git repository containing `path`.
pub fn suggest_handoff_environment(
    path: PathBuf,
    ctx: &AppContext,
) -> impl std::future::Future<Output = Option<SyncId>> + Send + 'static {
    let environments = CloudAmbientAgentEnvironment::get_all(ctx);
    async move {
        let touched_repo = resolve_repo_for_path(&path).await?;
        pick_handoff_overlap_env(
            &TouchedWorkspace {
                repos: vec![touched_repo],
                orphan_files: Vec::new(),
            },
            environments,
        )
    }
}

/// Pick the env that has the most overlap with the touched repos, breaking ties by
/// recency. Returns `None` when no env contains any of the touched repos (or when
/// `envs` is empty / the workspace touched no GitHub-mapped repos).
///
/// This is the "strict" overlap-aware pick used by the handoff pane bootstrap,
/// which calls it unconditionally and applies the result on top of whatever the
/// `EnvironmentSelector`'s `ensure_default_selection` had already picked. When
/// this returns `None`, callers leave the existing selection alone.
pub(crate) fn pick_handoff_overlap_env(
    workspace: &TouchedWorkspace,
    mut envs: Vec<CloudAmbientAgentEnvironment>,
) -> Option<SyncId> {
    if envs.is_empty() {
        return None;
    }

    let touched_repo_ids: Vec<&GithubRepo> = workspace
        .repos
        .iter()
        .filter_map(|r| r.repo_id.as_ref())
        .collect();
    if touched_repo_ids.is_empty() {
        return None;
    }

    // Sort most-recent-first so that ties on overlap count resolve to the most-
    // recently-used env. We then iterate and keep the first-best score.
    sort_environments_by_recency(&mut envs);
    let mut best: Option<(&CloudAmbientAgentEnvironment, usize)> = None;
    for env in &envs {
        let env_repos = &env.model().string_model.github_repos;
        let score = touched_repo_ids
            .iter()
            .filter(|id| env_repos.iter().any(|r| &r == *id))
            .count();
        if score == 0 {
            continue;
        }
        match best {
            None => best = Some((env, score)),
            Some((_, current)) if score > current => best = Some((env, score)),
            _ => {}
        }
    }
    best.map(|(env, _)| env.id)
}

#[cfg(test)]
#[path = "touched_repos_tests.rs"]
mod tests;
