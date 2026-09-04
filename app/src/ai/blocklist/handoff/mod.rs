//! Client-side pieces of the local-to-cloud Oz conversation handoff:
//!
//! - Payload types (`HandoffLaunchAttachments`, `PendingCloudLaunch`) carry the
//!   compose/auto-submit request from the input; the pipeline that used to
//!   consume them (`prepare_handoff`/`execute_handoff`) was removed when
//!   `FeatureFlag::OzHandoff` was folded permanently off (round 4an, part
//!   1/2) — the payload types themselves are kept because the still-live
//!   compose UI (`terminal/input.rs`) still constructs them.
//! - `snapshot`: gives the (removed) pipeline one local/remote snapshot-upload
//!   interface. Kept: also used directly by `remote_server::server_model`.
//! - `touched_repos`: walks the conversation's action history to collect every
//!   filesystem path the local agent has touched, groups those paths into git
//!   roots and orphan files, and exposes the env-overlap pick used by the
//!   handoff pane bootstrap. Kept: `derive_touched_workspace` is used directly
//!   by `remote_server::handoff_snapshot` (the SSH-remote daemon's own,
//!   unrelated handoff-snapshot RPC handler).

use super::PendingAttachment;
use crate::server::server_api::ai::AttachmentInput;

#[cfg(feature = "local_fs")]
pub(crate) mod snapshot;
#[cfg(feature = "local_fs")]
pub(crate) mod touched_repos;

#[cfg(feature = "local_fs")]
#[allow(unused_imports)]
pub use touched_repos::suggest_handoff_environment;

/// Prompt attachments represented for both cloud submission and local restoration.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
#[derive(Debug, Clone, Default)]
pub struct HandoffLaunchAttachments {
    /// Serialized attachments sent in the cloud agent request.
    pub request_attachments: Vec<AttachmentInput>,
    /// Local attachment models restored into the source input after failure.
    pub display_attachments: Vec<PendingAttachment>,
}

/// Carries the auto-submit payload for `& query` and `/handoff query`.
/// `request_attachments` feed the spawn request while `display_attachments`
/// are restored into the source input on failure.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
#[derive(Debug, Clone)]
pub struct PendingCloudLaunch {
    /// Optional prompt submitted with the handoff.
    pub prompt: String,
    /// Attachments transferred from the source input.
    pub attachments: HandoffLaunchAttachments,
}
