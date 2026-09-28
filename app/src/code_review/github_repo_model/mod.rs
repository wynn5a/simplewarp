mod model;
pub use model::GitHubRepoModel;

#[derive(Debug)]
pub enum GitHubRepoEvent {
    /// Emitted when `pr_info` changes value (fetch result differs from
    /// cached, branch change cleared the cache, etc.).
    PrInfoChanged,
    /// Emitted when `repository_info` changes value.
    RepositoryInfoChanged,
}
