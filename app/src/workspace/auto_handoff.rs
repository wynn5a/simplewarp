use warpui::AppContext;

use super::AutoCloudHandoffTrigger;

/// `AutoCloudHandoffController` (which used to drive the sleep-triggered and
/// URI-triggered auto-handoff-to-cloud flow) was removed when
/// `FeatureFlag::OzHandoff` was folded permanently off (round 4an, part
/// 1/2) — it required a Warp account/server, which this build never has.
/// Kept as a no-op so the URI entry point (`app/src/uri/mod.rs`) keeps
/// compiling; the real macOS-sleep trigger path (`SystemStats` subscription)
/// was removed along with the controller.
pub(crate) fn trigger_auto_handoff_to_cloud(_trigger: AutoCloudHandoffTrigger, _ctx: &mut AppContext) {
}
