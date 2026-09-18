//! Client-side payload types of the local-to-cloud Oz conversation handoff:
//!
//! `HandoffLaunchAttachments` and `PendingCloudLaunch` carry the auto-submit
//! request. The pipeline that used to consume them (`prepare_handoff`/
//! `execute_handoff`), the compose UI that used to construct them, and the
//! snapshot-upload pipeline they fed were all removed when
//! `FeatureFlag::OzHandoff` was folded permanently off (round 4an) — the
//! payload types themselves are kept because `workspace/view.rs`'s handoff
//! stubs still construct them.

use super::PendingAttachment;
use crate::ai::ambient_agents::task::AttachmentInput;

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
