use serde::{Deserialize, Serialize};

/// Source-control provider hosting a repository.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CodeForge {
    #[default]
    #[serde(rename = "GITHUB")]
    GitHub,
    #[serde(rename = "GITLAB")]
    GitLab,
}

/// Identifies a repository and the source-control provider that hosts it.
///
/// For GitLab, `owner` contains the full, potentially nested namespace.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRepo {
    /// The repository's explicit source-control provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_forge: Option<CodeForge>,
    pub owner: String,
    pub repo: String,
    /// Ref to check out after cloning this repository (commit SHA, branch, or
    /// tag). Absent leaves the clone on the default branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkout_ref: Option<String>,
}

impl SourceRepo {
    pub fn new(code_forge: CodeForge, owner: String, repo: String) -> Self {
        Self {
            code_forge: Some(code_forge),
            owner,
            repo,
            checkout_ref: None,
        }
    }

    /// Returns a copy of this repository pinned to `checkout_ref`.
    pub fn with_checkout_ref(mut self, checkout_ref: Option<String>) -> Self {
        self.checkout_ref = checkout_ref;
        self
    }
}

#[cfg(test)]
#[path = "source_repo_tests.rs"]
mod tests;
