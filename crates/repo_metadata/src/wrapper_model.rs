//! Unified repository metadata model.
//!
//! [`RepoMetadataModel`] is the singleton entry point for all repository metadata
//! queries. It holds a handle to [`LocalRepoMetadataModel`] and keys its API by
//! [`RepositoryIdentifier`].

use std::path::Path;

use warp_util::standardized_path::StandardizedPath;
use warpui_core::{AppContext, ModelContext, ModelHandle, SingletonEntity};

use crate::file_tree_store::FileTreeState;
use crate::file_tree_update::MetadataUpdateType;
use crate::local_model::{
    GetContentsArgs, IndexedRepoState, LocalRepoMetadataModel, RepoContents,
    RepositoryMetadataEvent,
};
use crate::repository_identifier::RepositoryIdentifier;
use crate::{RepoMetadataError, StandingQueryResults, StandingQueryResultsDelta};

/// Unified events emitted by the [`RepoMetadataModel`] wrapper.
///
/// These are mapped from the sub-model events into a common enum keyed by
/// [`RepositoryIdentifier`].
#[derive(Debug)]
pub enum RepoMetadataEvent {
    /// A repository was added or updated.
    RepositoryUpdated { id: RepositoryIdentifier },
    /// A repository was removed.
    RepositoryRemoved { id: RepositoryIdentifier },
    /// File trees for repositories were updated.
    FileTreeUpdated { ids: Vec<RepositoryIdentifier> },
    /// A file tree entry was updated.
    FileTreeEntryUpdated {
        id: RepositoryIdentifier,
        /// Specifies whether this event contains a precise delta or requires a conservative
        /// refresh because the entry was replaced without one.
        update_type: MetadataUpdateType,
    },
    /// Stored standing-query paths changed for a repository.
    StandingQueryResultsUpdated {
        id: RepositoryIdentifier,
        delta: StandingQueryResultsDelta,
    },
    /// Updating a repository failed.
    UpdatingRepositoryFailed { id: RepositoryIdentifier },
}

/// Singleton wrapper that provides the repository metadata API.
///
/// All consumers should interact with this type rather than accessing the
/// sub-model directly.
pub struct RepoMetadataModel {
    local: ModelHandle<LocalRepoMetadataModel>,
}

impl RepoMetadataModel {
    /// Creates a new `RepoMetadataModel`, instantiating the local sub-model and
    /// subscribing to its events for forwarding.
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let local = ctx.add_model(LocalRepoMetadataModel::new);
        ctx.subscribe_to_model(&local, Self::forward_local_event);
        Self { local }
    }

    // ── Event forwarding ─────────────────────────────────────────────

    fn forward_local_event(
        &mut self,
        _: ModelHandle<LocalRepoMetadataModel>,
        event: &RepositoryMetadataEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        let unified = match event {
            RepositoryMetadataEvent::RepositoryUpdated { path } => {
                RepoMetadataEvent::RepositoryUpdated {
                    id: RepositoryIdentifier::local(path.clone()),
                }
            }
            RepositoryMetadataEvent::RepositoryRemoved { path } => {
                RepoMetadataEvent::RepositoryRemoved {
                    id: RepositoryIdentifier::local(path.clone()),
                }
            }
            RepositoryMetadataEvent::FileTreeUpdated { paths } => {
                RepoMetadataEvent::FileTreeUpdated {
                    ids: paths
                        .iter()
                        .map(|p| RepositoryIdentifier::local(p.clone()))
                        .collect(),
                }
            }
            RepositoryMetadataEvent::FileTreeEntryUpdated { path, update_type } => {
                RepoMetadataEvent::FileTreeEntryUpdated {
                    id: RepositoryIdentifier::local(path.clone()),
                    update_type: update_type.clone(),
                }
            }
            RepositoryMetadataEvent::StandingQueryResultsUpdated { path, delta } => {
                RepoMetadataEvent::StandingQueryResultsUpdated {
                    id: RepositoryIdentifier::local(path.clone()),
                    delta: delta.clone(),
                }
            }
            RepositoryMetadataEvent::UpdatingRepositoryFailed { path } => {
                RepoMetadataEvent::UpdatingRepositoryFailed {
                    id: RepositoryIdentifier::local(path.clone()),
                }
            }
        };
        ctx.emit(unified);
    }

    // ── Unified query API ────────────────────────────────────────────

    /// Returns the [`FileTreeState`] for a repository identified by `id`.
    pub fn get_repository<'a>(
        &self,
        id: &RepositoryIdentifier,
        ctx: &'a AppContext,
    ) -> Option<&'a FileTreeState> {
        match id {
            RepositoryIdentifier(path) => self.local.as_ref(ctx).get_repository(path),
        }
    }

    pub fn standing_query_results<'a>(
        &self,
        id: &RepositoryIdentifier,
        ctx: &'a AppContext,
    ) -> Option<&'a StandingQueryResults> {
        match id {
            RepositoryIdentifier(path) => self.local.as_ref(ctx).standing_query_results(path),
        }
    }

    /// Returns whether the given repository is indexed.
    pub fn has_repository(&self, id: &RepositoryIdentifier, ctx: &AppContext) -> bool {
        match id {
            RepositoryIdentifier(path) => self.local.as_ref(ctx).has_repository(path),
        }
    }

    /// Returns the current [`IndexedRepoState`] for a repository.
    pub fn repository_state<'a>(
        &self,
        id: &RepositoryIdentifier,
        ctx: &'a AppContext,
    ) -> Option<&'a IndexedRepoState> {
        match id {
            RepositoryIdentifier(path) => self.local.as_ref(ctx).repository_state(path),
        }
    }

    /// Returns a future that resolves once repository indexing has completed at least once.
    ///
    /// Callers should inspect [`Self::repository_state`] after awaiting this future to see whether
    /// indexing succeeded or failed.
    pub fn repository_indexed(
        &self,
        id: &RepositoryIdentifier,
        ctx: &mut ModelContext<Self>,
    ) -> futures::future::BoxFuture<'static, ()> {
        match id {
            RepositoryIdentifier(path) => {
                let path = path.clone();
                self.local
                    .update(ctx, |local, _| local.repository_indexed(&path))
            }
        }
    }

    /// Returns repository contents for the specified repository.
    ///
    /// The number of returned entries is capped; when the repository contains
    /// more matching entries, the result is truncated and
    /// [`RepoContents::truncated`] is set to `true`.
    ///
    /// Returns an error if the repository is not indexed, indexing is pending, or indexing failed.
    pub fn get_repo_contents<'a>(
        &self,
        id: &RepositoryIdentifier,
        args: GetContentsArgs,
        ctx: &'a AppContext,
    ) -> Result<RepoContents<'a>, RepoMetadataError> {
        match id {
            RepositoryIdentifier(path) => self.local.as_ref(ctx).get_repo_contents(path, args),
        }
    }

    /// Finds the repository root that contains the given local path.
    pub fn find_repository_for_path(
        &self,
        path: &Path,
        ctx: &AppContext,
    ) -> Option<StandardizedPath> {
        self.local.as_ref(ctx).find_repository_for_path(path)
    }

    /// Fully indexes a local directory identified by a standardized path.
    pub fn index_local_directory_path(
        &self,
        path: &StandardizedPath,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), RepoMetadataError> {
        let path = path.clone();
        self.local
            .update(ctx, |local, ctx| local.index_directory_path(&path, ctx))
    }

    /// Indexes a local repository from the given repository handle.
    pub fn index_directory(
        &self,
        repository: ModelHandle<crate::repository::Repository>,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), RepoMetadataError> {
        self.local
            .update(ctx, |local, ctx| local.index_directory(repository, ctx))
    }

    /// Lazily indexes a local standalone path with only the first level of children.
    pub fn index_lazy_loaded_path(
        &self,
        path: &StandardizedPath,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), RepoMetadataError> {
        let path = path.clone();
        self.local
            .update(ctx, |local, ctx| local.index_lazy_loaded_path(&path, ctx))
    }

    /// Loads a specific directory inside an already-tracked local tree.
    pub fn load_directory(
        &self,
        repo_root: &StandardizedPath,
        dir_path: &StandardizedPath,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), RepoMetadataError> {
        let repo_root = repo_root.clone();
        let dir_path = dir_path.clone();
        self.local.update(ctx, |local, ctx| {
            local.load_directory(&repo_root, &dir_path, ctx)
        })
    }

    /// Loads a specific directory inside an already-tracked local tree and returns a future that
    /// resolves once the async load has been applied or rejected.
    pub fn load_directory_with_completion(
        &self,
        repo_root: &StandardizedPath,
        dir_path: &StandardizedPath,
        ctx: &mut ModelContext<Self>,
    ) -> Result<futures::future::BoxFuture<'static, Result<(), RepoMetadataError>>, RepoMetadataError>
    {
        let repo_root = repo_root.clone();
        let dir_path = dir_path.clone();
        self.local.update(ctx, |local, ctx| {
            local.load_directory_with_completion(&repo_root, &dir_path, ctx)
        })
    }

    /// Registers paths that must be loaded even when gitignored or beyond the
    /// tree's size limit.
    ///
    /// This delegates to the local model because force-included path matching
    /// happens while building local file trees.
    pub fn register_force_included_paths(
        &self,
        paths: impl IntoIterator<Item = std::path::PathBuf>,
        ctx: &mut ModelContext<Self>,
    ) {
        let paths: Vec<_> = paths.into_iter().collect();
        self.local.update(ctx, |local, _| {
            local.register_force_included_paths(paths);
        });
    }

    pub fn set_project_skill_provider_paths(
        &self,
        paths: impl IntoIterator<Item = std::path::PathBuf>,
        ctx: &mut ModelContext<Self>,
    ) {
        let paths: Vec<_> = paths.into_iter().collect();
        self.local.update(ctx, |local, _| {
            local.set_project_skill_provider_paths(paths);
        });
    }

    /// Removes a lazily-loaded local standalone path from tracking.
    pub fn remove_lazy_loaded_path(&self, path: &StandardizedPath, ctx: &mut ModelContext<Self>) {
        let path = path.clone();
        self.local
            .update(ctx, |local, ctx| local.remove_lazy_loaded_path(&path, ctx));
    }

    /// Removes a repository from tracking.
    pub fn remove_repository(
        &self,
        id: &RepositoryIdentifier,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), RepoMetadataError> {
        match id {
            RepositoryIdentifier(path) => {
                let path = path.clone();
                self.local
                    .update(ctx, |local, ctx| local.remove_repository(&path, ctx))
            }
        }
    }

    /// Returns whether the given local path is tracked as a lazily-loaded standalone path.
    pub fn is_lazy_loaded_path(&self, path: &StandardizedPath, ctx: &AppContext) -> bool {
        self.local.as_ref(ctx).is_lazy_loaded_path(path)
    }
}

impl warpui_core::Entity for RepoMetadataModel {
    type Event = RepoMetadataEvent;
}

impl SingletonEntity for RepoMetadataModel {}

#[cfg(any(test, feature = "test-util"))]
impl RepoMetadataModel {
    /// Inserts repository state directly into the local sub-model for testing.
    pub fn insert_test_state(
        &self,
        repo_path: StandardizedPath,
        state: FileTreeState,
        ctx: &mut ModelContext<Self>,
    ) {
        self.local.update(ctx, |local, _ctx| {
            local.insert_test_state(repo_path, state);
        });
    }

    pub fn insert_test_standing_results(
        &self,
        repo_path: StandardizedPath,
        standing_results: StandingQueryResults,
        ctx: &mut ModelContext<Self>,
    ) {
        self.local.update(ctx, |local, _ctx| {
            local.insert_test_standing_results(repo_path, standing_results);
        });
    }
}
