mod alt_screen;
pub mod async_find;
mod block_list;
mod rich_content;
#[cfg(any(test, feature = "integration_tests"))]
mod testing;

use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::sync::Arc;

use alt_screen::{AltScreenFindRun, run_find_on_alt_screen};
pub use async_find::{AsyncFindController, AsyncFindStatus};
pub use block_list::{BlockGridMatch, BlockListMatch};
use parking_lot::FairMutex;
use rich_content::FindableRichContentHandle;
pub use rich_content::{FindableRichContentView, RichContentMatchId};
use settings::Setting as _;
use warpui::{AppContext, Entity, EntityId, ModelContext, SingletonEntity, ViewHandle};

use crate::settings::InputModeSettings;
use crate::terminal::block_list_element::GridType;
use crate::terminal::block_list_viewport::InputMode;
use crate::terminal::model::TerminalModel;
use crate::terminal::model::grid::grid_handler::GridHandler;
use crate::terminal::model::index::Point;
use crate::terminal::model::terminal_model::BlockIndex;
use crate::view_components::find::{FindDirection, FindEvent, FindModel};

/// Pre-computed find data for rendering a single block.
///
/// Matches are pre-converted from the async controller's absolute coordinates to relative
/// `Point`s, with matches truncated from scrollback dropped.
pub struct BlockFindRenderData {
    /// Command grid matches.
    command_matches: Vec<RangeInclusive<Point>>,
    /// Output grid matches.
    output_matches: Vec<RangeInclusive<Point>>,
    /// Focused range in the command grid, if any.
    focused_command_range: Option<RangeInclusive<Point>>,
    /// Focused range in the output grid, if any.
    focused_output_range: Option<RangeInclusive<Point>>,
}

impl BlockFindRenderData {
    fn new(
        controller: &AsyncFindController,
        block_index: BlockIndex,
        command_grid: Option<&GridHandler>,
        output_grid: Option<&GridHandler>,
    ) -> Self {
        // Highlights are rendered in *original* grid coordinates (the renderer maps them to
        // displayed positions itself).
        let command_matches = command_grid
            .and_then(|grid| {
                controller
                    .matches_for_block_grid(block_index, GridType::PromptAndCommand)
                    .map(|matches| {
                        matches
                            .iter()
                            .filter_map(|m| m.to_original_range(grid))
                            .collect::<Vec<_>>()
                    })
            })
            .unwrap_or_default();

        // Convert output grid matches, skipping any hidden by an active block
        // filter so highlights match the (filtered) visible content. Uses
        // original coordinates (see command_matches above).
        let output_matches = output_grid
            .and_then(|grid| {
                controller
                    .matches_for_block_grid(block_index, GridType::Output)
                    .map(|matches| {
                        matches
                            .iter()
                            .filter(|m| !m.is_filtered)
                            .filter_map(|m| m.to_original_range(grid))
                            .collect::<Vec<_>>()
                    })
            })
            .unwrap_or_default();

        // Get focused match ranges (also in original coordinates).
        let focused_match = controller.focused_terminal_match();
        let focused_command_range = focused_match
            .as_ref()
            .filter(|m| m.block_index == block_index && m.grid_type == GridType::PromptAndCommand)
            .and_then(|m| command_grid.and_then(|grid| m.range.to_original_range(grid)));
        let focused_output_range = focused_match
            .as_ref()
            .filter(|m| m.block_index == block_index && m.grid_type == GridType::Output)
            .and_then(|m| output_grid.and_then(|grid| m.range.to_original_range(grid)));

        Self {
            command_matches,
            output_matches,
            focused_command_range,
            focused_output_range,
        }
    }

    /// Returns an iterator over match ranges for the command grid.
    pub fn command_grid_matches(&self) -> impl Iterator<Item = &RangeInclusive<Point>> {
        self.command_matches.iter()
    }

    /// Returns an iterator over match ranges for the output grid.
    pub fn output_grid_matches(&self) -> impl Iterator<Item = &RangeInclusive<Point>> {
        self.output_matches.iter()
    }

    /// Returns the focused match range if it's in the specified grid.
    pub fn focused_range_for_grid(&self, grid_type: GridType) -> Option<RangeInclusive<Point>> {
        match grid_type {
            GridType::PromptAndCommand => self.focused_command_range.clone(),
            GridType::Output => self.focused_output_range.clone(),
            _ => None,
        }
    }
}

/// `TerminalView`-scoped model for the find bar.
pub struct TerminalFindModel {
    terminal_model: Arc<FairMutex<TerminalModel>>,

    rich_content_views: HashMap<EntityId, Box<dyn FindableRichContentHandle>>,

    /// The most recent find "run" on the alt screen, if any.
    alt_screen_find_run: Option<AltScreenFindRun>,

    /// `true` if the find bar is open.
    is_find_bar_open: bool,

    /// Runs find over the block list on a background thread.
    pub(crate) async_find_controller: AsyncFindController,
}

impl FindModel for TerminalFindModel {
    fn focused_match_index(&self) -> Option<usize> {
        if self.terminal_model.lock().is_alt_screen_active() {
            self.alt_screen_find_run
                .as_ref()
                .and_then(|run| run.focused_match_index())
        } else {
            self.async_find_controller.focused_match_index()
        }
    }

    fn match_count(&self) -> usize {
        if self.terminal_model.lock().is_alt_screen_active() {
            self.alt_screen_find_run
                .as_ref()
                .map(|run| run.matches().len())
                .unwrap_or(0)
        } else {
            self.async_find_controller.match_count()
        }
    }

    fn default_find_direction(&self, app: &AppContext) -> FindDirection {
        let input_mode = *InputModeSettings::as_ref(app).input_mode.value();
        match input_mode {
            InputMode::PinnedToBottom | InputMode::Waterfall => FindDirection::Up,
            InputMode::PinnedToTop => FindDirection::Down,
        }
    }

    fn is_scanning(&self) -> bool {
        self.is_async_find_scanning()
    }
}

impl TerminalFindModel {
    pub fn new(terminal_model: Arc<FairMutex<TerminalModel>>) -> Self {
        Self {
            async_find_controller: AsyncFindController::new(terminal_model.clone()),
            terminal_model,
            rich_content_views: HashMap::new(),
            alt_screen_find_run: None,
            is_find_bar_open: false,
        }
    }
    pub fn register_findable_rich_content_view<T: FindableRichContentView>(
        &mut self,
        view_handle: ViewHandle<T>,
    ) {
        let view_id = view_handle.id();
        let boxed_handle: Box<dyn FindableRichContentHandle> = Box::new(view_handle.clone());

        self.async_find_controller
            .register_rich_content_view(view_id, Box::new(view_handle));

        self.rich_content_views.insert(view_id, boxed_handle);
    }

    /// Returns `true` if the find bar is currently open.
    pub(crate) fn is_find_bar_open(&self) -> bool {
        self.is_find_bar_open
    }

    /// Updates find bar visibility.
    pub(crate) fn set_is_find_bar_open(&mut self, is_open: bool) {
        self.is_find_bar_open = is_open;
    }

    /// Returns the last find run for the alt screen.
    pub(crate) fn alt_screen_find_run(&self) -> Option<&AltScreenFindRun> {
        self.alt_screen_find_run.as_ref()
    }

    /// Returns the currently focused match as a `BlockListMatch`.
    pub(crate) fn focused_block_list_match(&self) -> Option<BlockListMatch> {
        let model = self.terminal_model.lock();
        if model.is_alt_screen_active() {
            // Alt screen doesn't use BlockListMatch.
            return None;
        }

        // The focused match is either a terminal match or an AI match (or neither).
        let controller = &self.async_find_controller;
        if let Some(async_match) = controller.focused_terminal_match() {
            let block = model.block_list().block_at(async_match.block_index)?;
            let grid = match async_match.grid_type {
                GridType::PromptAndCommand => block.prompt_and_command_grid().grid_handler(),
                GridType::Output => block.output_grid().grid_handler(),
                _ => return None,
            };
            let range = async_match.range.to_range(grid)?;
            return Some(BlockListMatch::CommandBlock(BlockGridMatch {
                block_index: async_match.block_index,
                grid_type: async_match.grid_type,
                range,
                is_filtered: false,
            }));
        }
        controller
            .focused_ai_match()
            .map(|ai_match| BlockListMatch::RichContent {
                match_id: ai_match.match_id,
                view_id: ai_match.view_id,
                index: ai_match.total_index,
            })
    }

    /// Returns the focused rich content (AI) match id, if any.
    ///
    /// AI block rendering uses this to apply the focused-match highlight color.
    pub(crate) fn focused_rich_content_match_id(&self) -> Option<RichContentMatchId> {
        if self.terminal_model.lock().is_alt_screen_active() {
            return None;
        }

        self.async_find_controller
            .focused_ai_match()
            .map(|m| m.match_id)
    }

    /// Returns find render data for a specific block, if find is active.
    ///
    /// Note: This method does NOT check if alt screen is active. It is intended
    /// for use during blocklist rendering where the caller has already determined
    /// that we are not in alt screen mode. Callers who need alt screen checking
    /// should do so before calling this method.
    ///
    /// The grid handlers are needed to convert from absolute to relative coordinates and filter
    /// truncated matches.
    pub(crate) fn find_render_data_for_block(
        &self,
        block_index: BlockIndex,
        command_grid: Option<&GridHandler>,
        output_grid: Option<&GridHandler>,
    ) -> Option<BlockFindRenderData> {
        let controller = &self.async_find_controller;
        controller
            .has_active_find()
            .then(|| BlockFindRenderData::new(controller, block_index, command_grid, output_grid))
    }

    /// Returns `FindOptions` applied to the active find run, if any.
    pub fn active_find_options(&self) -> Option<&FindOptions> {
        if self.terminal_model.lock().is_alt_screen_active() {
            self.alt_screen_find_run.as_ref().map(|run| run.options())
        } else {
            self.async_find_controller.find_options()
        }
    }

    /// Runs find with the given `options` on the alt screen or blocklist (depending on which is
    /// active).
    pub fn run_find(&mut self, options: FindOptions, ctx: &mut ModelContext<Self>) {
        if self.terminal_model.lock().is_alt_screen_active() {
            self.alt_screen_find_run = Some(run_find_on_alt_screen(
                options,
                self.terminal_model.lock().alt_screen(),
            ));
            ctx.emit(FindEvent::RanFind);
            return;
        }

        let block_sort_direction = InputModeSettings::as_ref(ctx)
            .input_mode
            .value()
            .block_sort_direction();

        log::trace!(
            "[async_find] Starting async find with query: {:?}",
            options.query
        );
        self.async_find_controller
            .start_find(&options, block_sort_direction, ctx);
        ctx.emit(FindEvent::RanFind);
    }

    /// Reruns find with the same options applied to the current run, to be called if terminal
    /// contents have changed since the last find run.
    pub fn rerun_find_on_active_grid(&mut self, ctx: &mut ModelContext<Self>) {
        if self.terminal_model.lock().is_alt_screen_active() {
            if let Some(old_find_state) = self.alt_screen_find_run.take() {
                self.alt_screen_find_run =
                    Some(old_find_state.rerun(self.terminal_model.lock().alt_screen()));
                ctx.emit(FindEvent::RanFind);
            }
            return;
        }

        if !self.async_find_controller.has_active_find() {
            return;
        }

        // Get the active block index and dirty range info.
        // We use active_block_index() (not last_non_hidden_block_by_index) because
        // the active block is where output is being written, even if it's still
        // "empty" and would be filtered out by the default BlockFilter.
        let mut model = self.terminal_model.lock();
        let active_block_index = model.block_list().active_block_index();

        // Consume dirty ranges from both grids. We need mutable access
        // because take_find_dirty_rows_range is destructive.
        let active_block = model.block_list_mut().active_block_mut();
        let output_dirty_info = active_block
            .grid_of_type_mut(GridType::Output)
            .and_then(|grid| {
                let dirty = grid.grid_handler_mut().take_find_dirty_rows_range()?;
                let truncated = grid.grid_handler().num_lines_truncated();
                Some((dirty, GridType::Output, truncated))
            });
        let command_dirty_info = active_block
            .grid_of_type_mut(GridType::PromptAndCommand)
            .and_then(|grid| {
                let dirty = grid.grid_handler_mut().take_find_dirty_rows_range()?;
                let truncated = grid.grid_handler().num_lines_truncated();
                Some((dirty, GridType::PromptAndCommand, truncated))
            });

        // Drop the model lock before emitting events.
        drop(model);

        self.invalidate_async_find_block(active_block_index, output_dirty_info, ctx);
        if let Some(info) = command_dirty_info {
            self.invalidate_async_find_block(active_block_index, Some(info), ctx);
        }
    }

    /// Focus the "next" match, depending on the given [`FindDirection`], in the active find run's
    /// list of matches.
    ///
    /// If there is no focused match, focuses the first match in the list.
    pub fn focus_next_find_match(
        &mut self,
        find_direction: FindDirection,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.terminal_model.lock().is_alt_screen_active() {
            if let Some(alt_screen_find_run) = self.alt_screen_find_run.as_mut() {
                alt_screen_find_run.focus_next_match(find_direction);
            }
        } else {
            self.async_find_controller.focus_next_match(find_direction);
        }
        ctx.emit(FindEvent::UpdatedFocusedMatch);
    }

    /// Notifies every registered rich-content child view (e.g. AI blocks) to
    /// drop its cached find state and repaint, **without** touching the active
    /// find run's options/config.
    ///
    /// Callers that just need stale highlights to disappear (e.g.
    /// `close_find_bar`) must use this rather than [`Self::clear_matches`].
    /// `clear_matches` routes through `AsyncFindController::clear_results`, which also drops
    /// `current_find_options` — losing the query that `open_find_bar` later
    /// reads back via [`Self::active_find_options`] to restore the previous
    /// search.
    pub fn clear_rich_content_matches(&self, ctx: &mut ModelContext<Self>) {
        for view in self.rich_content_views.values() {
            view.clear_matches(ctx);
        }
    }

    /// Clears matches in the active find run, if any.
    pub fn clear_matches(&mut self, ctx: &mut ModelContext<Self>) {
        if self.terminal_model.lock().is_alt_screen_active() {
            if let Some(run) = self.alt_screen_find_run.take() {
                self.alt_screen_find_run = Some(run.cleared());
            }
        } else {
            self.async_find_controller.clear_results(ctx);
        }
        ctx.emit(FindEvent::RanFind);
    }

    /// Updates matches in the active find run for the block at the `block_index`, which is
    /// presumed to be filtered.
    ///
    /// Under the hood, this does not result in a new find run, but updates state on the matches
    /// for the existing find run, which is used to determine if matches should be represented in
    /// the find bar UI (e.g. match count, focused match index).
    pub fn update_matches_for_filtered_block(
        &mut self,
        block_index: BlockIndex,
        ctx: &mut ModelContext<Self>,
    ) {
        // Recompute which matches are hidden by the (already-applied) block filter and treat
        // filtered rows as if they do not exist for search, which keeps the match count, focus
        // traversal and highlights consistent.
        self.async_find_controller
            .recompute_filtered_for_block(block_index);
        ctx.emit(FindEvent::RanFind);
    }

    /// Returns true if an async find operation is currently scanning.
    pub fn is_async_find_scanning(&self) -> bool {
        self.async_find_controller.is_scanning()
    }

    /// Invalidates results for a specific block in async find.
    ///
    /// This should be called when a block's content changes.
    ///
    /// # Arguments
    /// * `block_index` - The index of the block that changed.
    /// * `dirty_info` - If provided, a `(row_range, grid_type, num_lines_truncated)`
    ///   tuple describing the dirty region. If `None`, a full block rescan is enqueued.
    /// * `ctx` - The model context.
    pub fn invalidate_async_find_block(
        &mut self,
        block_index: BlockIndex,
        dirty_info: Option<(RangeInclusive<usize>, GridType, u64)>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.async_find_controller
            .invalidate_block(block_index, dirty_info);
        ctx.emit(FindEvent::RanFind);
    }

    /// Notifies async find that a block has completed.
    ///
    /// This should be called when a command finishes, so that the completed block
    /// (which now has its final output) gets scanned for matches if find is active.
    /// Uses the dirty range accumulated during execution for incremental scanning.
    pub fn notify_block_completed(
        &mut self,
        block_index: BlockIndex,
        ctx: &mut ModelContext<Self>,
    ) {
        // Check if there's an active find before acquiring the lock.
        if !self.async_find_controller.has_active_find() {
            return;
        }

        log::trace!(
            "[async_find] notify_block_completed: block_index={:?}",
            block_index
        );

        // Get the dirty range from the completed block's output grid.
        // We need mutable access to consume the dirty range.
        let (dirty_range, num_lines_truncated) = {
            let mut model = self.terminal_model.lock();
            model
                .block_list_mut()
                .block_at_mut(block_index)
                .and_then(|block| block.grid_of_type_mut(GridType::Output))
                .map(|output_grid| {
                    let dirty_range = output_grid.grid_handler_mut().take_find_dirty_rows_range();
                    let num_lines_truncated = output_grid.grid_handler().num_lines_truncated();
                    (dirty_range, num_lines_truncated)
                })
                .unwrap_or((None, 0))
        };

        // Use invalidate_async_find_block which handles the dirty range properly.
        let dirty_info = dirty_range.map(|range| (range, GridType::Output, num_lines_truncated));
        self.invalidate_async_find_block(block_index, dirty_info, ctx);
    }

    /// Returns the async find controller.
    pub fn async_find_controller(&self) -> &AsyncFindController {
        &self.async_find_controller
    }
}

impl Entity for TerminalFindModel {
    type Event = FindEvent;
}

/// Parameters for a find "run".
#[derive(Debug, Clone, Default)]
pub struct FindOptions {
    /// The find query, if any.
    pub query: Option<Arc<String>>,

    /// `true` if the find should be case-sensitive.
    pub is_case_sensitive: bool,

    /// `true` if the query should be matched as a regex pattern.
    pub is_regex_enabled: bool,

    /// If `Some()`, the find run only surfaces matches that are in blocks with the provided
    /// indices. If `None`, the find run surfaces matches across the entire blocklist.
    ///
    /// This is ignored when applied to alt screen find runs.
    pub blocks_to_include_in_results: Option<Vec<BlockIndex>>,
}

impl FindOptions {
    pub fn with_is_case_sensitive(mut self, is_case_sensitive: bool) -> Self {
        self.is_case_sensitive = is_case_sensitive;
        self
    }

    pub fn with_is_regex_enabled(mut self, is_regex_enabled: bool) -> Self {
        self.is_regex_enabled = is_regex_enabled;
        self
    }

    pub fn with_query(mut self, query: Option<impl Into<Arc<String>>>) -> Self {
        self.query = query.map(Into::into);
        self
    }

    pub fn with_blocks_to_include_in_results(
        mut self,
        block_indices: Option<impl IntoIterator<Item = BlockIndex>>,
    ) -> Self {
        self.blocks_to_include_in_results =
            block_indices.map(|indices| indices.into_iter().collect());
        self
    }
}

impl std::fmt::Debug for TerminalFindModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FindModel")
            .field("terminal_model", &"<TerminalModel>")
            .field(
                "rich_content_views",
                &self.rich_content_views.keys().collect::<Vec<_>>(),
            )
            .field("alt_screen_find_run", &self.alt_screen_find_run)
            .field("is_find_bar_open", &self.is_find_bar_open)
            .finish()
    }
}
