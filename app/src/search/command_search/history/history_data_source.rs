use std::collections::HashMap;
use std::sync::Arc;

use futures_lite::future::yield_now;
use warpui::{AppContext, SingletonEntity};

use super::HistorySearchItem;
use crate::search::async_snapshot_data_source::AsyncSnapshotDataSource;
use crate::search::command_search::searcher::CommandSearchItemAction;
use crate::search::data_source::{Query, QueryResult};
use crate::search::mixer::{BoxFuture, DataSourceRunErrorWrapper};
use crate::settings::AISettings;
use crate::terminal;
use crate::terminal::HistoryEntry;
use crate::terminal::model::session::SessionId;

/// Added to the fuzzy score per natural-log step of how many times a command was run.
const USAGE_BONUS_PER_LN_RUN: f64 = 6.;
const MAX_USAGE_BONUS: f64 = 24.;
/// Bonus for the most recent command; scales down linearly to zero for the oldest.
const MAX_RECENCY_BONUS: f64 = 12.;

pub(crate) struct HistorySnapshot {
    commands: Arc<[Arc<HistoryEntry>]>,
    query_text: String,
}

/// Creates an async data source for shell history commands.
#[cfg(test)]
pub fn history_data_source(
    commands: Vec<HistoryEntry>,
) -> AsyncSnapshotDataSource<HistorySnapshot, CommandSearchItemAction> {
    let commands: Arc<[Arc<HistoryEntry>]> = commands.into_iter().map(Arc::new).collect();
    history_data_source_from_shared(commands)
}

fn history_data_source_from_shared(
    commands: Arc<[Arc<HistoryEntry>]>,
) -> AsyncSnapshotDataSource<HistorySnapshot, CommandSearchItemAction> {
    AsyncSnapshotDataSource::new(
        move |query: &Query, _app: &AppContext| HistorySnapshot {
            // Historical commands are all stored as Arcs (with COW semantics and very infrequent writes),
            // so cloning the commands to pass them in to the async sort function is a negligible cost.
            commands: commands.clone(),
            query_text: query.text.clone(),
        },
        fuzzy_match_history,
    )
}

pub(crate) fn history_data_source_for_session(
    session_id: SessionId,
    history_model: &terminal::History,
    app: &AppContext,
) -> AsyncSnapshotDataSource<HistorySnapshot, CommandSearchItemAction> {
    let include_agent_commands = *AISettings::as_ref(app).include_agent_commands_in_history;
    let commands: Arc<[Arc<HistoryEntry>]> = history_model
        .commands_shared(session_id)
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| include_agent_commands || !entry.is_agent_executed)
        .collect();
    history_data_source_from_shared(commands)
}

/// Collapses repeated commands to their most recent run, in chronological order, pairing each with
/// a ranking bonus derived from how often and how recently it was run.
fn collapse_with_rank_bonus(commands: &[Arc<HistoryEntry>]) -> Vec<(Arc<HistoryEntry>, i64)> {
    let mut last_run_and_count: HashMap<&str, (usize, usize)> = HashMap::new();
    for (index, entry) in commands.iter().enumerate() {
        let (last_run, count) = last_run_and_count
            .entry(entry.command.as_str())
            .or_default();
        *last_run = index;
        *count += 1;
    }

    let newest_index = commands.len().saturating_sub(1).max(1) as f64;
    commands
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let (last_run, count) = last_run_and_count[entry.command.as_str()];
            (index == last_run).then(|| {
                let usage_bonus =
                    (USAGE_BONUS_PER_LN_RUN * (count as f64).ln()).min(MAX_USAGE_BONUS);
                let recency_bonus = MAX_RECENCY_BONUS * index as f64 / newest_index;
                (entry.clone(), (usage_bonus + recency_bonus).round() as i64)
            })
        })
        .collect()
}

pub(crate) fn fuzzy_match_history(
    snapshot: HistorySnapshot,
) -> BoxFuture<'static, Result<Vec<QueryResult<CommandSearchItemAction>>, DataSourceRunErrorWrapper>>
{
    Box::pin(async move {
        let mut results = Vec::new();
        let ranked_commands = collapse_with_rank_bonus(&snapshot.commands);

        // History entries are cheap to match (single short string), so we use a large chunk
        // size to reduce yield overhead while still allowing cancellation of stale queries.
        for chunk in ranked_commands.chunks(512) {
            for (entry, rank_bonus) in chunk {
                if let Some(match_result) = fuzzy_match::match_indices_case_insensitive(
                    entry.command.as_str(),
                    snapshot.query_text.as_str(),
                ) {
                    results.push(
                        HistorySearchItem {
                            entry: entry.clone(),
                            match_result,
                            rank_bonus: *rank_bonus,
                        }
                        .into(),
                    );
                }
            }
            yield_now().await;
        }

        Ok(results)
    })
}

#[cfg(test)]
#[path = "history_data_source_tests.rs"]
mod tests;
