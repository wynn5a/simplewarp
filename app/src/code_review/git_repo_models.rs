use std::collections::HashMap;

use repo_metadata::repositories::DetectedRepositories;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::{Entity, ModelContext, ModelHandle, SingletonEntity, WeakModelHandle};

use super::git_repo_model::GitRepoStatusModel;
use super::github_repo_model::GitHubRepoModel;

// ── GitRepoModels (singleton cache) ─────────────────────────────────────────

/// Singleton model that acts as a cache / factory for per-repository
/// [`GitRepoStatusModel`] and [`GitHubRepoModel`] instances.
///
/// Multiple terminals in the same repo share a single sub-model.  When the last
/// strong handle to a sub-model is dropped, the models are torn down automatically.
pub struct GitRepoModels {
    // Per-repo status / GitHub-info models. Callers in the same repo share an entry, and it is
    // torn down when the last strong handle is dropped.
    git_status_models: HashMap<LocalOrRemotePath, WeakModelHandle<GitRepoStatusModel>>,
    github_repo_models: HashMap<LocalOrRemotePath, WeakModelHandle<GitHubRepoModel>>,
}
impl GitRepoModels {
    pub fn new() -> Self {
        Self {
            git_status_models: HashMap::new(),
            github_repo_models: HashMap::new(),
        }
    }

    /// Get or create the watcher-backed per-repo status model for `repo`.
    ///
    /// Multiple callers in the same repo share one model (cached by
    /// `LocalOrRemotePath`); it is torn down when the last strong handle is
    /// dropped.
    ///
    /// Callers hold the returned `ModelHandle` for as long as they need updates.
    pub fn subscribe(
        &mut self,
        repo: &LocalOrRemotePath,
        ctx: &mut ModelContext<Self>,
    ) -> anyhow::Result<ModelHandle<GitRepoStatusModel>> {
        if let Some(handle) = self
            .git_status_models
            .get(repo)
            .and_then(|weak| weak.upgrade(ctx))
        {
            return Ok(handle);
        }

        let LocalOrRemotePath::Local(repo_path) = repo;
        let Some(repository_model) =
            DetectedRepositories::as_ref(ctx).get_local_watched_repo_for_path(repo_path, ctx)
        else {
            anyhow::bail!(
                "No watched repository found for path: {}",
                repo_path.display()
            );
        };
        let repo_path = repo_path.clone();
        let handle = ctx.add_model(|ctx| GitRepoStatusModel::new(repo_path, repository_model, ctx));

        self.git_status_models
            .insert(repo.clone(), handle.downgrade());
        Ok(handle)
    }

    /// Get or create the `gh`-driven per-repo GitHub-info model for `repo`.
    ///
    /// The model subscribes to the sibling git status model to track
    /// the current branch and fetches PR / repository info on creation, on
    /// branch change, and on a periodic timer. Multiple callers in the same
    /// repo share one model (cached by `LocalOrRemotePath`).
    ///
    /// Callers hold the returned `ModelHandle` for as long as they need updates.
    pub fn subscribe_github_repo(
        &mut self,
        repo: &LocalOrRemotePath,
        ctx: &mut ModelContext<Self>,
    ) -> anyhow::Result<ModelHandle<GitHubRepoModel>> {
        if let Some(handle) = self
            .github_repo_models
            .get(repo)
            .and_then(|weak| weak.upgrade(ctx))
        {
            return Ok(handle);
        }

        // GitHubRepoModel needs a sibling GitRepoStatusModel for branch info.
        let git_status = self.subscribe(repo, ctx)?;
        let LocalOrRemotePath::Local(repo_path) = repo;
        let repo_path = repo_path.clone();
        let handle = ctx.add_model(|ctx| GitHubRepoModel::new(repo_path, git_status, ctx));

        self.github_repo_models
            .insert(repo.clone(), handle.downgrade());
        Ok(handle)
    }
}

impl Default for GitRepoModels {
    fn default() -> Self {
        Self::new()
    }
}
impl Entity for GitRepoModels {
    type Event = ();
}

impl SingletonEntity for GitRepoModels {}
