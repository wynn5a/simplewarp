use warp_editor::content::version::BufferVersion;
use warp_util::path::LineAndColumnArg;

#[derive(Debug, Clone, Copy)]
pub enum ScrollWheelBehavior {
    AlwaysHandle,
}

impl ScrollWheelBehavior {
    pub fn should_handle(&self) -> bool {
        matches!(self, Self::AlwaysHandle)
    }
}

#[derive(Clone)]
pub enum ScrollPosition {
    LineAndColumn(LineAndColumnArg),
    FocusedDiffHunk,
}

/// We don't want to scroll to the provided line number until the content has
/// been loaded from the file and layout has occurred to update the viewport size.
/// This struct is used to track the state of the scroll trigger.
pub struct ScrollTrigger {
    pub minimum_applicable_version: BufferVersion,
    pub position: ScrollPosition,
}

impl ScrollTrigger {
    /// Create a new scroll trigger that will jump to the provided line number
    /// after the provided version has been loaded and a layout update has occurred.
    pub fn new(position: ScrollPosition, version: BufferVersion) -> Self {
        Self {
            position,
            minimum_applicable_version: version,
        }
    }
}
