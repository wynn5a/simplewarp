pub mod conversations;
mod data_sources;
mod files;
mod filter_chip_renderer;
pub mod launch_config;
pub mod mixer;
pub mod navigation;
#[cfg_attr(not(feature = "local_tty"), allow(dead_code))]
pub mod new_session;
pub mod render_util;
pub mod repos;
mod selected_items;
pub mod separator_search_item;
pub mod tabs;
pub mod view;
mod zero_state;

use filter_chip_renderer::FilterChipRenderer;
pub use mixer::{CommandPaletteMixer, ItemSummary};
pub use selected_items::SelectedItems;
use serde::{Deserialize, Serialize};
pub use view::View;

pub mod styles {
    pub const SEARCH_ITEM_TEXT_PADDING: f32 = 4.;
}

/// Where a command palette open/toggle was triggered from. The source selects which palette
/// instance is used (e.g. the dedicated Ctrl+Tab palette) when more than one exists.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum PaletteSource {
    PrefixChange,
    Keybinding,
    CtrlTab { shift_pressed_initially: bool },
    WarpDrive,
    QuitModal,
    LogOutModal,
    IntegrationTest,
    ConversationManager,
    ContextChip,
    PaneHeader,
    AgentTip,
    TitleBarSearchBar,
}
