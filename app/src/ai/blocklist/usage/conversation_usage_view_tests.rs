//! Click-handler regression tests for [`ConversationUsageView`].
//!
//! The original bug was that clicks on the "View details" / "Show N more"
//! affordances did nothing because the view was created via `add_view`
//! instead of `add_typed_action_view`, so the framework had no handler
//! registered for `ConversationUsageViewAction::*` and silently logged
//! `Dispatched action has no handlers: ToggleDetailsExpanded`.
//!
//! The fix lives at the view-creation site in `terminal/view.rs`. These
//! tests are a defense-in-depth layer that exercises the view's
//! `handle_action` implementation directly, so:
//!
//! * If the `TypedActionView` impl is removed or broken, the test won't
//!   compile (compile-time guard).
//! * If the handler logic for toggling `details_expanded` / resetting
//!   `show_all_clicked` regresses, the assertions below will fail
//!   (runtime guard).
//!
//! The tests use the same `view.update(&mut app, |view, ctx|
//! view.handle_action(...))` pattern as the existing
//! other view tests so they stay decoupled from the
//! framework's render path (which needs `Appearance` / theme singletons
//! that aren't relevant to the handler's correctness).

use std::collections::HashMap;

use super::*;
use crate::persistence::model::{ModelTokenUsage, PRIMARY_AGENT_CATEGORY};

fn placeholder_usage_info() -> ConversationUsageInfo {
    ConversationUsageInfo {
        credits_spent: 0.0,
        platform_credits_spent: 0.0,
        credits_spent_for_last_block: None,
        tool_calls: 0,
        models: Vec::new(),
        context_window_usage: 0.0,
        context_window_segments: Vec::new(),
        files_changed: 0,
        lines_added: 0,
        lines_removed: 0,
        commands_executed: 0,
    }
}

#[test]
fn custom_endpoint_models_use_the_external_key_icon_bucket() {
    let view = ConversationUsageView::new(
        ConversationUsageInfo {
            models: vec![ModelTokenUsage {
                model_id: "Friendly alias".to_string(),
                custom_endpoint_tokens: 6,
                custom_endpoint_token_usage_by_category: HashMap::from([(
                    PRIMARY_AGENT_CATEGORY.to_string(),
                    6,
                )]),
                ..Default::default()
            }],
            ..placeholder_usage_info()
        },
        None,
        MouseStateHandle::default(),
    );

    assert_eq!(
        view.collect_models_by_category()
            .get(PRIMARY_AGENT_CATEGORY),
        Some(&vec![("Friendly alias".to_string(), true)])
    );
}
