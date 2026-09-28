use std::future::Future;
use std::path::PathBuf;

use anyhow::Result;
use futures::StreamExt as _;
use instant::Instant;
use num_traits::SaturatingSub;
use regex::escape;
use string_offset::ByteOffset;
use warp_errors::report_error;
use warp_ripgrep::search::{Match as RipgrepMatch, Submatch};
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::r#async::SpawnedFutureHandle;
use warpui::{Entity, ModelContext, ModelSpawner};

use crate::workspace::view::global_search::view::GlobalSearchEvent;
use crate::workspace::view::global_search::{GlobalSearchMatch, SearchConfig};

const START_BATCH_AFTER_COUNT: usize = 50;
const MAX_BATCH_SIZE: usize = 512;
const MAX_BATCH_AGE_MS: u64 = 4000;

/// Aggregate state for one logical search across all of its sources.
struct ActiveSearch {
    search_id: u32,
    remaining_sources: usize,
    completed_sources: usize,
    local_source_failed: bool,
    total_match_count: usize,
    /// True when any source hit its match cap.
    capped: bool,
}

/// Result of one search source that ran to completion.
struct SourceResult {
    match_count: usize,
    capped: bool,
}

pub struct GlobalSearch {
    /// Spawned search tasks for the current search.
    search_handles: Vec<SpawnedFutureHandle>,
    /// Aggregate completion state for the current search.
    active_search: Option<ActiveSearch>,
    // track the search ID so that we only show results for the current search
    next_search_id: u32,
}

impl Entity for GlobalSearch {
    type Event = GlobalSearchEvent;
}

async fn flush_batch(
    spawner: &ModelSpawner<GlobalSearch>,
    search_id: u32,
    batch: &mut Vec<GlobalSearchMatch>,
) {
    if batch.is_empty() {
        return;
    }

    let items = std::mem::take(batch);

    let _ = spawner
        .spawn(move |_me, ctx| {
            ctx.emit(GlobalSearchEvent::ProgressBatch { search_id, items });
        })
        .await;
}

impl GlobalSearch {
    pub fn new() -> Self {
        GlobalSearch {
            search_handles: Vec::new(),
            active_search: None,
            next_search_id: 1,
        }
    }

    pub fn abort_search(&mut self) {
        for handle in self.search_handles.drain(..) {
            handle.abort();
        }
        self.active_search = None;
    }

    pub fn run_search(
        &mut self,
        pattern: String,
        roots: Vec<LocalOrRemotePath>,
        search_config: SearchConfig,
        ctx: &mut ModelContext<Self>,
    ) {
        if !self.search_handles.is_empty() {
            log::info!("GlobalSearch: aborting previous search");
        }
        self.abort_search();

        let search_id = self.next_search_id;
        self.next_search_id += 1;

        let effective_pattern = if search_config.use_regex {
            pattern
        } else {
            escape(&pattern)
        };
        let ignore_case = !search_config.use_case_sensitivity;
        let multiline = effective_pattern.contains('\n');

        let local_roots: Vec<PathBuf> = roots.into_iter().map(PathBuf::from).collect();

        ctx.emit(GlobalSearchEvent::Started { search_id });
        let source_count = usize::from(!local_roots.is_empty());
        if source_count == 0 {
            ctx.emit(GlobalSearchEvent::Completed {
                search_id,
                total_match_count: 0,
                capped: false,
                local_source_failed: false,
            });
            return;
        }

        self.active_search = Some(ActiveSearch {
            search_id,
            remaining_sources: source_count,
            completed_sources: 0,
            local_source_failed: false,
            total_match_count: 0,
            capped: false,
        });

        if !local_roots.is_empty() {
            self.spawn_local_search(
                search_id,
                effective_pattern,
                local_roots,
                ignore_case,
                multiline,
                ctx,
            );
        }
    }

    fn spawn_local_search(
        &mut self,
        search_id: u32,
        pattern: String,
        roots: Vec<PathBuf>,
        ignore_case: bool,
        multiline: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let spawner = ctx.spawner();
        self.spawn_source(
            search_id,
            async move {
                let result = Self::run_warp_ripgrep_cli(
                    search_id,
                    pattern,
                    roots,
                    ignore_case,
                    multiline,
                    spawner,
                )
                .await;
                match result {
                    Ok(match_count) => Some(SourceResult {
                        match_count,
                        capped: false,
                    }),
                    Err(err) => {
                        report_error!(
                            err.context("GlobalSearch: warp_ripgrep CLI search failed or aborted")
                        );
                        None
                    }
                }
            },
            ctx,
        );
    }

    /// Spawns one search source and routes its outcome into the shared completion accounting.
    /// Sources emit their matches via `Progress`/`ProgressBatch` while running and log their own
    /// failures.
    fn spawn_source(
        &mut self,
        search_id: u32,
        source: impl Future<Output = Option<SourceResult>> + Send + 'static,
        ctx: &mut ModelContext<Self>,
    ) {
        let task = ctx.spawn(source, move |me, outcome, ctx| {
            me.handle_source_completed(search_id, outcome, ctx);
        });
        self.search_handles.push(task);
    }

    /// Records the completion of one search source (`None` when the source
    /// failed; the source already logged the failure). When all sources have
    /// finished, emits `Completed` (or `Failed` when every source failed).
    fn handle_source_completed(
        &mut self,
        search_id: u32,
        outcome: Option<SourceResult>,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(active) = self.active_search.as_mut() else {
            return;
        };
        if active.search_id != search_id {
            return;
        }

        match outcome {
            Some(SourceResult {
                match_count,
                capped,
            }) => {
                active.completed_sources += 1;
                active.total_match_count += match_count;
                active.capped |= capped;
            }
            None => active.local_source_failed = true,
        }

        active.remaining_sources = active.remaining_sources.saturating_sub(1);
        if active.remaining_sources > 0 {
            return;
        }

        let active = self
            .active_search
            .take()
            .expect("active search was checked above");
        if active.completed_sources == 0 {
            ctx.emit(GlobalSearchEvent::Failed {
                search_id,
                error: "Global search failed.".to_string(),
            });
        } else {
            ctx.emit(GlobalSearchEvent::Completed {
                search_id,
                total_match_count: active.total_match_count,
                capped: active.capped,
                local_source_failed: active.local_source_failed,
            });
        }
    }

    async fn run_warp_ripgrep_cli(
        search_id: u32,
        pattern: String,
        roots: Vec<PathBuf>,
        ignore_case: bool,
        multiline: bool,
        spawner: ModelSpawner<GlobalSearch>,
    ) -> Result<usize> {
        let roots_display: Vec<_> = roots.iter().map(|r| r.display().to_string()).collect();
        log::info!(
            "GlobalSearch: starting warp_ripgrep CLI search with pattern={pattern}, roots={:?}",
            roots_display
        );

        let patterns = &[pattern];
        let stream =
            warp_ripgrep::search::search_streaming(patterns, &roots, ignore_case, multiline)?;
        futures::pin_mut!(stream);

        let mut total_match_count: usize = 0;
        let mut num_unbatched_emitted: usize = 0;
        let mut batch: Vec<GlobalSearchMatch> = Vec::new();
        let mut last_batch_flush_at = Instant::now();

        while let Some(raw_match) = stream.next().await {
            // Expand each submatch into its own result row (matching
            // the old per-submatch behavior). Each row gets the line
            // text trimmed up to that particular submatch.
            for per_submatch in Self::expand_submatches(Self::local_match_to_global(raw_match)) {
                total_match_count += 1;

                if num_unbatched_emitted < START_BATCH_AFTER_COUNT {
                    num_unbatched_emitted += 1;

                    let _ = spawner
                        .spawn(move |_me, ctx| {
                            ctx.emit(GlobalSearchEvent::Progress {
                                search_id,
                                result: per_submatch,
                            });
                        })
                        .await;
                } else {
                    batch.push(per_submatch);

                    let too_big = batch.len() >= MAX_BATCH_SIZE;
                    let too_old =
                        last_batch_flush_at.elapsed().as_millis() >= MAX_BATCH_AGE_MS as u128;

                    if too_big || too_old {
                        flush_batch(&spawner, search_id, &mut batch).await;
                        last_batch_flush_at = Instant::now();
                    }
                }
            }
        }

        if !batch.is_empty() {
            flush_batch(&spawner, search_id, &mut batch).await;
        }

        Ok(total_match_count)
    }

    fn local_match_to_global(m: RipgrepMatch) -> GlobalSearchMatch {
        GlobalSearchMatch {
            location: LocalOrRemotePath::Local(m.file_path),
            line_number: m.line_number,
            column_num: None,
            line_text: m.line_text,
            submatches: m.submatches,
        }
    }

    /// Expand a single match (which may contain multiple submatches
    /// on the same line) into one result per submatch. Each result gets the
    /// line text trimmed of leading whitespace up to that submatch.
    fn expand_submatches(m: GlobalSearchMatch) -> Vec<GlobalSearchMatch> {
        if m.submatches.len() <= 1 {
            let submatch = m.submatches.into_iter().next();
            let column_num = Self::column_from_submatch(&m.line_text, submatch.as_ref());
            return vec![Self::trim_leading_whitespace_for_submatch(
                &m.line_text,
                m.location,
                m.line_number,
                column_num,
                submatch,
            )];
        }

        m.submatches
            .into_iter()
            .map(|sub| {
                let column_num = Self::column_from_submatch(&m.line_text, Some(&sub));
                Self::trim_leading_whitespace_for_submatch(
                    &m.line_text,
                    m.location.clone(),
                    m.line_number,
                    column_num,
                    Some(sub),
                )
            })
            .collect()
    }

    /// Returns the original 1-based character column for a submatch.
    fn column_from_submatch(line_text: &str, submatch: Option<&Submatch>) -> Option<usize> {
        let byte_start = submatch?.byte_start.as_usize();
        if byte_start > line_text.len() || !line_text.is_char_boundary(byte_start) {
            return None;
        }
        Some(line_text[..byte_start].chars().count() + 1)
    }

    /// Trim leading whitespace from a line up to the given submatch,
    /// adjusting the submatch offset accordingly.
    fn trim_leading_whitespace_for_submatch(
        original_line: &str,
        location: LocalOrRemotePath,
        line_number: u32,
        column_num: Option<usize>,
        submatch: Option<Submatch>,
    ) -> GlobalSearchMatch {
        let submatch_start = submatch
            .as_ref()
            .map(|s| s.byte_start)
            .unwrap_or(ByteOffset::zero());

        let mut leading_trimmed_bytes = ByteOffset::zero();
        for (byte_index, ch) in original_line.char_indices() {
            if byte_index >= submatch_start.as_usize() {
                break;
            }
            if !ch.is_ascii_whitespace() {
                break;
            }
            leading_trimmed_bytes += ch.len_utf8();
        }

        let trimmed_line = original_line[leading_trimmed_bytes.as_usize()..].to_string();

        let submatches = if let Some(sub) = submatch {
            vec![Submatch {
                byte_start: sub.byte_start.saturating_sub(&leading_trimmed_bytes),
                byte_end: sub.byte_end.saturating_sub(&leading_trimmed_bytes),
            }]
        } else {
            Vec::new()
        };

        GlobalSearchMatch {
            location,
            line_number,
            column_num,
            line_text: trimmed_line,
            submatches,
        }
    }
}

impl Default for GlobalSearch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
