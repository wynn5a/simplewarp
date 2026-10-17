mod native;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use ignore::gitignore::Gitignore;
pub use native::build_outline;

use crate::index::{Entry, FileId};

/// Builds an "outline" of all files and directories under a path.
#[derive(Debug)]
pub struct Outline {
    /// Tree representation of the outlined directory.
    root: Entry,

    /// Mapping the leaf file nodes to their outline.
    file_id_to_outline: HashMap<FileId, FileOutline>,

    /// List of gitignore patterns.
    gitignores: Vec<Arc<Gitignore>>,
}

impl Outline {
    pub fn to_symbols_by_file(
        &self,
        partial_path_segments: Option<&Vec<String>>,
    ) -> HashMap<PathBuf, FileOutline> {
        let mut queue = VecDeque::from([&self.root]);
        let mut file_to_symbols = HashMap::new();

        // Iteratively print the files while preserving their traversal order.
        while let Some(entry) = queue.pop_front() {
            match entry {
                Entry::Directory(directory) => {
                    queue.extend(&directory.children);
                }
                Entry::File(file) => {
                    let Some(relative_file_path) = file.path.strip_prefix(self.root.path()) else {
                        continue;
                    };

                    if partial_path_segments.is_some_and(|paths| {
                        !paths.is_empty()
                            && !paths
                                .iter()
                                .any(|partial_path| relative_file_path.contains(partial_path))
                    }) {
                        continue;
                    }

                    if let Some(file_outline) = self.file_id_to_outline.get(&file.file_id) {
                        file_to_symbols
                            .insert(file.path.to_local_path_lossy(), file_outline.clone());
                    }
                }
            }
        }

        file_to_symbols
    }
}

/// An identifier symbol in the code file. For now this is just the top-level functions.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    /// The type prefix to the symbol. This is language specific.
    /// For example, for a function in rust, this will be "fn". Note that this could be
    /// empty if the symbol type does not have a prefix (e.g. methods in javascript).
    pub type_prefix: Option<String>,
    /// Line comments attached to the symbol.
    pub comment: Option<Vec<String>>,
    /// The starting line number of the symbol (1-indexed).
    pub line_number: usize,
}

/// Represents the "outline" of a file with all the identifier symbols of interest.
#[derive(Debug, Clone, Default)]
pub struct FileOutline {
    symbols: Option<Vec<Symbol>>,
}

impl FileOutline {
    /// Get the symbols from the outline.
    pub fn symbols(&self) -> Option<&Vec<Symbol>> {
        self.symbols.as_ref()
    }
}
