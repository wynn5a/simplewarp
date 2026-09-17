//! Common utilities for agent SDK commands.

use warpui::{AppContext, SingletonEntity as _};

use crate::ai::llms::{LLMId, LLMPreferences};

pub fn validate_agent_mode_base_model_id(
    model_id: &str,
    ctx: &AppContext,
) -> anyhow::Result<LLMId> {
    let llm_prefs = LLMPreferences::as_ref(ctx);
    let valid_ids = llm_prefs
        .get_base_llm_choices_for_agent_mode(ctx)
        .map(|info| info.id.clone())
        .collect::<Vec<_>>();

    classify_agent_mode_base_model_id(model_id, &valid_ids)
}

/// Classifies a user-supplied agent-mode model id against the available model list.
fn classify_agent_mode_base_model_id(model_id: &str, valid_ids: &[LLMId]) -> anyhow::Result<LLMId> {
    let llm_id: LLMId = model_id.into();
    if valid_ids.contains(&llm_id) {
        Ok(llm_id)
    } else {
        let suggestions = valid_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        Err(anyhow::anyhow!(
            "Unknown model id '{model_id}'. Try one of: {suggestions}"
        ))
    }
}

#[cfg(test)]
#[path = "common_tests.rs"]
mod tests;
