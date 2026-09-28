use warpui::{AppContext, SingletonEntity};

use crate::FeatureFlag;
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::workspaces::workspace::UgcCollectionEnablementSetting;

/// Returns `true` if we should collect UGC (user-generated content) telemetry for AI features.
///
/// This should apply to telemetry events that include user-generated content, like queries or
/// outputs, but need not be checked for regular metadata telemetry events.
///
/// For example, a metadata event that records if a user toggled Pair/Dispatch mode does not
/// require this check, but an event that logs the input buffer for natural language detection
/// _does_ need to check this.
pub fn should_collect_ai_ugc_telemetry(app: &AppContext) -> bool {
    match UserWorkspaces::as_ref(app).get_ugc_collection_enablement_setting() {
        UgcCollectionEnablementSetting::Disable => false,
        UgcCollectionEnablementSetting::Enable => true,
        UgcCollectionEnablementSetting::RespectUserSetting => {
            FeatureFlag::AgentModeAnalytics.is_enabled()
        }
    }
}
