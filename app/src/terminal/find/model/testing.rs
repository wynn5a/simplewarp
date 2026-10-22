//! Exports helper test-only methods for use in unit and integration tests.
use super::{BlockListMatch, TerminalFindModel};
use crate::terminal::block_list_element::GridType;
use crate::terminal::model::terminal_model::BlockIndex;

impl TerminalFindModel {
    /// The match count excluding filter-hidden matches.
    pub fn visible_block_list_match_count(&self) -> usize {
        self.async_find_controller.match_count()
    }

    /// Returns the block index of the focused match, if it is in a command block.
    pub fn focused_match_block_index(&self) -> Option<BlockIndex> {
        match self.focused_block_list_match()? {
            BlockListMatch::CommandBlock(block_match) => Some(block_match.block_index),
            BlockListMatch::RichContent { .. } => None,
        }
    }

    /// Returns `true` if the block at `block_index` has any match.
    pub fn has_match_in_block(&self, block_index: BlockIndex) -> bool {
        [GridType::PromptAndCommand, GridType::Output]
            .into_iter()
            .any(|grid_type| {
                self.async_find_controller
                    .matches_for_block_grid(block_index, grid_type)
                    .is_some_and(|matches| !matches.is_empty())
            })
    }
}
