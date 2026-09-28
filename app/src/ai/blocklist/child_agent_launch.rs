//! Frontend-neutral preparation and settings propagation for local Oz children.
use warpui::{AppContext, EntityId, SingletonEntity as _};

use crate::AIExecutionProfilesModel;
use crate::ai::ambient_agents::AmbientAgentTaskId;
use crate::ai::ambient_agents::task::normalize_orchestrator_agent_name;
use crate::ai::llms::{LLMId, LLMPreferences};

/// Server-side state prepared before a frontend creates the child's surface.
pub struct PreparedLocalOzChildLaunch {
    pub task_id: AmbientAgentTaskId,
    pub conversation_name: String,
}

/// Mints the local-only task id shared by the GUI hidden-pane launch path.
/// No server row is created: the id only stamps the child conversation and
/// controller so local state can tell children apart.
pub fn prepare_local_oz_child_launch(name: &str) -> PreparedLocalOzChildLaunch {
    let agent_name = normalize_orchestrator_agent_name(name);
    PreparedLocalOzChildLaunch {
        task_id: AmbientAgentTaskId::new(),
        conversation_name: agent_name.unwrap_or_default(),
    }
}

/// Copies the parent's execution profile and effective base model to a child
/// surface before its first request is sent.
pub fn inherit_child_agent_settings(
    parent_surface_id: EntityId,
    child_surface_id: EntityId,
    ctx: &mut AppContext,
) {
    let parent_profile_id = AIExecutionProfilesModel::as_ref(ctx)
        .active_profile(Some(parent_surface_id), ctx)
        .id()
        .clone();
    AIExecutionProfilesModel::handle(ctx).update(ctx, |profiles, ctx| {
        profiles.set_active_profile(child_surface_id, parent_profile_id, ctx);
    });

    let parent_base_model_id = LLMPreferences::as_ref(ctx)
        .get_active_base_model(ctx, Some(parent_surface_id))
        .id
        .clone();
    LLMPreferences::handle(ctx).update(ctx, |preferences, ctx| {
        preferences.update_preferred_agent_mode_llm(&parent_base_model_id, child_surface_id, ctx);
    });
}

/// Applies a non-empty run-wide model override after parent settings have
/// been inherited.
pub fn apply_child_agent_model_override(
    child_surface_id: EntityId,
    model_id: Option<&str>,
    ctx: &mut AppContext,
) {
    let Some(model_id) = model_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return;
    };
    let model_id = LLMId::from(model_id);
    LLMPreferences::handle(ctx).update(ctx, |preferences, ctx| {
        preferences.set_agent_mode_llm_override(child_surface_id, model_id, ctx);
    });
}
