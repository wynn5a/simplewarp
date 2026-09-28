//! Notebook interaction types shared by the editor and its views.

/// A selection/navigation mode within the notebook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionMode {
    /// Navigate between command/code blocks and embedded workflows.
    Command,
    /// Navigate with a text cursor/selection.
    Text,
}
