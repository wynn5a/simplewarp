use std::collections::HashMap;

#[cfg(feature = "local_fs")]
use repo_metadata::repositories::DetectedRepositories;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::{Entity, ModelContext, ModelHandle, SingletonEntity, WeakModelHandle};

use super::git_repo_model::GitRepoStatusModel;
#[cfg(feature = "local_fs")]
use super::git_repo_model::new_local_git_repo_status_model;
use super::github_repo_model::GitHubRepoModel;
#[cfg(feature = "local_fs")]
use super::github_repo_model::LocalGitHubRepoModel;

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

    /// Get or create the watcher-backed per-repo status model for `repo`. Remote repositories
    /// have no status model.
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

        let handle = match repo {
            LocalOrRemotePath::Local(repo_path) => {
                #[cfg(feature = "local_fs")]
                {
                    let Some(repository_model) = DetectedRepositories::as_ref(ctx)
                        .get_local_watched_repo_for_path(repo_path, ctx)
                    else {
                        anyhow::bail!(
                            "No watched repository found for path: {}",
                            repo_path.display()
                        );
                    };
                    new_local_git_repo_status_model(repo_path.clone(), repository_model, ctx)
                }
                #[cfg(not(feature = "local_fs"))]
                {
                    anyhow::bail!(
                        "No watched repository found for path: {}",
                        repo_path.display()
                    );
                }
            }
            LocalOrRemotePath::Remote(remote_path) => {
                anyhow::bail!(
                    "Git status is unavailable for remote repository: {}",
                    remote_path.path
                );
            }
        };

        self.git_status_models
            .insert(repo.clone(), handle.downgrade());
        Ok(handle)
    }

    /// Get or create the `gh`-driven per-repo GitHub-info model for `repo`. Remote repositories
    /// have no GitHub-info model.
    ///
    /// The local backend subscribes to the sibling git status model to track
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

        let handle = match repo {
            LocalOrRemotePath::Local(repo_path) => {
                #[cfg(feature = "local_fs")]
                {
                    // LocalGitHubRepoModel needs a sibling GitRepoStatusModel for
                    // branch info.
                    let git_status = self.subscribe(repo, ctx)?;
                    let repo_path = repo_path.clone();
                    let inner =
                        ctx.add_model(|ctx| LocalGitHubRepoModel::new(repo_path, git_status, ctx));
                    ctx.add_model(|ctx| {
                        ctx.subscribe_to_model(&inner, |me, _, event, ctx| {
                            GitHubRepoModel::forward_event(me, event, ctx)
                        });
                        GitHubRepoModel::Local(inner)
                    })
                }
                #[cfg(not(feature = "local_fs"))]
                {
                    anyhow::bail!(
                        "Local GitHub repo info is unavailable without local_fs: {}",
                        repo_path.display()
                    );
                }
            }
            LocalOrRemotePath::Remote(remote_path) => {
                anyhow::bail!(
                    "GitHub repo info is unavailable for remote repository: {}",
                    remote_path.path
                );
            }
        };

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
