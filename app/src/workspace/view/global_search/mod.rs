use warp_ripgrep::search::Submatch;
use warp_util::local_or_remote_path::LocalOrRemotePath;

pub struct SearchConfig {
    pub use_regex: bool,
    pub use_case_sensitivity: bool,
}

/// A single global search match: one line in one file.
#[derive(Clone, Debug)]
pub struct GlobalSearchMatch {
    pub location: LocalOrRemotePath,
    pub line_number: u32,
    /// Original 1-based character column in the file. This is captured
    /// before display-only whitespace trimming so opening a result navigates
    /// to the correct location.
    pub column_num: Option<usize>,
    pub line_text: String,
    pub submatches: Vec<Submatch>,
}

#[path = "model.rs"]
pub mod model;
pub mod view;
