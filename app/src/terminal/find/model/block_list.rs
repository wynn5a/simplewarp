//! Find match types for the blocklist.
use std::ops::RangeInclusive;

use warpui::EntityId;

use super::rich_content::RichContentMatchId;
use crate::terminal::GridType;
use crate::terminal::model::blocks::TotalIndex;
use crate::terminal::model::index::Point;
use crate::terminal::model::terminal_model::BlockIndex;

/// Represents a single find match in a grid-based block in the blocklist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockGridMatch {
    /// The type of grid in which the match was found.
    pub grid_type: GridType,

    /// The character index range of the match.
    pub range: RangeInclusive<Point>,

    /// The index of the containing block.
    pub block_index: BlockIndex,

    /// `true` if the match should be filtered out from displayed results (e.g. if the containing
    /// row has been filtered out via block filtering).
    pub is_filtered: bool,
}

/// Represents a single find match in the blocklist.
///
/// Match values are snapshots of the find state that produced them. The grid
/// `range` on `CommandBlock` and the `index` on `RichContent` are captured at
/// scan time and can be invalidated by subsequent block list mutations (new
/// blocks, removals, rich content rescans, etc.). Callers should consume
/// cloned values inline.
///
/// TODO(vkodithala): The `RichContent` variant mirrors `AsyncFocusedAiMatch` in the async
/// path. Both derive `Clone` even though their contents are short-lived;
/// explore removing `Clone` from both in a future PR to enforce the snapshot
/// contract in the type system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockListMatch {
    CommandBlock(BlockGridMatch),
    RichContent {
        match_id: RichContentMatchId,
        view_id: EntityId,
        index: TotalIndex,
    },
}
