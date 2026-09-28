mod file_outline;
pub mod locations;

pub use file_outline::{Outline, Symbol, build_outline};
use native::*;
pub use repo_metadata::entry::{is_git_internal_path, should_watch_directory_in_git_path};
pub use repo_metadata::{
    BuildTreeError, DirectoryEntry, Entry, FileId, FileMetadata, matches_gitignores,
};

mod native {
    use std::thread::available_parallelism;

    pub(super) const MAX_PARALLEL_THREADS: usize = 2;

    fn create_thread_pool() -> Option<rayon::ThreadPool> {
        let num_threads = available_parallelism()
            .map(|parallelism| (parallelism.get() / 2).clamp(1, MAX_PARALLEL_THREADS))
            .unwrap_or(MAX_PARALLEL_THREADS);

        rayon::ThreadPoolBuilder::new()
            .thread_name(|index| format!("warp-code-indexing-{index}"))
            .num_threads(num_threads)
            .build()
            .ok()
    }

    lazy_static::lazy_static! {
        pub(super) static ref THREADPOOL: Option<rayon::ThreadPool> = create_thread_pool();
    }
}
