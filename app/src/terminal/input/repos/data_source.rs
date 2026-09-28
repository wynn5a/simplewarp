//! Async data source for the inline repos menu.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use warpui::{AppContext, Entity, SingletonEntity};

use crate::ai::persisted_workspace::PersistedWorkspace;
use crate::search::data_source::{Query, QueryResult};
use crate::search::mixer::{AsyncDataSource, BoxFuture, DataSourceRunErrorWrapper};
use crate::terminal::input::repos::AcceptRepo;
use crate::util::git::RepoGitSummary;

/// Cache of per-repo git summaries (branch + diff stats) keyed by repo path.
///
/// Shared between the data source, which reads it to render results immediately,
/// and the view, which populates it in the background. This lets the menu show
/// the repo list synchronously while the (relatively expensive) git data is
/// lazily loaded and filled in as it arrives.
pub type GitSummaryCache = Arc<Mutex<HashMap<PathBuf, RepoGitSummary>>>;

pub struct RepoMenuDataSource {
    /// Git summaries populated in the background by the view. Reads never block
    /// on git; missing entries simply render without branch/diff-stat suffixes.
    git_summaries: GitSummaryCache,
}

impl RepoMenuDataSource {
    pub fn new(git_summaries: GitSummaryCache) -> Self {
        Self { git_summaries }
    }
}

impl AsyncDataSource for RepoMenuDataSource {
    type Action = AcceptRepo;

    fn run_query(
        &self,
        query: &Query,
        app: &AppContext,
    ) -> BoxFuture<'static, Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper>> {
        let workspace_paths: Vec<PathBuf> = PersistedWorkspace::as_ref(app)
            .workspaces()
            .map(|m| m.path)
            .collect();

        let query_text = query.text.trim().to_lowercase();

        // Snapshot the currently-known git summaries. This is intentionally
        // non-blocking: whatever has been loaded so far is rendered, and the
        // rest fills in when the background load re-runs the query.
        let git_summaries = {
            let cache = self.git_summaries.lock().unwrap_or_else(|e| e.into_inner());
            cache.clone()
        };

        Box::pin(async move {
            {
                use crate::terminal::input::repos::search_item::RepoSearchItem;

                let mut items: Vec<RepoSearchItem> = workspace_paths
                    .into_iter()
                    .map(|path| {
                        let summary = git_summaries.get(&path).cloned();
                        RepoSearchItem::new(path, summary)
                    })
                    .collect();
                items.sort_by(|a, b| a.display_name.cmp(&b.display_name));

                let results: Vec<QueryResult<AcceptRepo>> = if query_text.is_empty() {
                    items.into_iter().map(QueryResult::from).collect()
                } else {
                    items
                        .into_iter()
                        .filter_map(|item| {
                            let match_result = fuzzy_match::match_indices_case_insensitive(
                                &item.display_name,
                                &query_text,
                            )?;
                            if match_result.score < 25 {
                                return None;
                            }
                            Some(QueryResult::from(
                                item.with_name_match_result(Some(match_result)),
                            ))
                        })
                        .collect()
                };

                Ok(results)
            }
        })
    }
}

impl Entity for RepoMenuDataSource {
    type Event = ();
}
