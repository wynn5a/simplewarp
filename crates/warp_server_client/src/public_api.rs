use warp_errors::{ErrorExt, register_error};

/// Typed error for HTTP operations so retry classifiers can inspect status failures.
#[derive(Debug, thiserror::Error)]
#[error("HTTP request failed with status {status}: {body}")]
pub struct HttpStatusError {
    pub status: u16,
    pub body: String,
}

impl ErrorExt for HttpStatusError {
    fn is_actionable(&self) -> bool {
        !matches!(self.status, 408 | 429)
    }
}

register_error!(HttpStatusError);
