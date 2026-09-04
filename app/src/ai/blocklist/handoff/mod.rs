//! Client-side pieces of the local-to-cloud Oz conversation handoff:
//!
//! - Payload types (`HandoffLaunchAttachments`, `PendingCloudLaunch`) carry the
//!   auto-submit request; the pipeline that used to consume them
//!   (`prepare_handoff`/`execute_handoff`) and the compose UI that used to
//!   construct them were both removed when `FeatureFlag::OzHandoff` was folded
//!   permanently off (round 4an) — the payload types themselves are kept
//!   because `workspace/view.rs`'s handoff stubs still construct them.
//! - `snapshot`: gives the (removed) pipeline one local/remote snapshot-upload
//!   interface. Kept: also used directly by `remote_server::server_model`.
//! - `touched_repos`: walks a flat list of filesystem paths the local agent
//!   has touched and groups them into git roots and orphan files. Kept:
//!   `derive_touched_workspace` is used directly by
//!   `remote_server::handoff_snapshot` (the SSH-remote daemon's own,
//!   unrelated handoff-snapshot RPC handler).

use super::PendingAttachment;
use crate::server::server_api::ai::AttachmentInput;

#[cfg(feature = "local_fs")]
pub(crate) mod snapshot;
#[cfg(feature = "local_fs")]
pub(crate) mod touched_repos;

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
