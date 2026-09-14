use ai::skills::{SkillProvider, SkillReference, SkillScope};
use fuzzy_match::{FuzzyMatchResult, match_indices_case_insensitive};
use ordered_float::OrderedFloat;
use warp_core::ui::icons::Icon;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::{AppContext, SingletonEntity as _};

use crate::ai::skills::SkillManager;

/// Surface-neutral skill selection result shared by GUI and TUI menus.
#[derive(Clone)]
pub struct SelectableSkill {
    pub name: String,
    pub reference: SkillReference,
    pub description: String,
    pub scope: SkillScope,
    pub provider: SkillProvider,
    pub icon_override: Option<Icon>,
    pub name_match_result: Option<FuzzyMatchResult>,
    pub score: OrderedFloat<f64>,
}

/// Returns skills available for selection in the active input surface.
///
/// This owns the shared discovery, bundled-skill policy, fuzzy matching, and
/// ordering used by both frontend adapters.
pub fn query_selectable_skills(
    working_directory: Option<&LocalOrRemotePath>,
    include_bundled: bool,
    query_text: &str,
    app: &AppContext,
) -> Vec<SelectableSkill> {
    let skill_manager = SkillManager::as_ref(app);
    let query_text = query_text.trim();
    let mut results = skill_manager
        .get_skills_for_working_directory(working_directory, app)
        .into_iter()
        .filter(|skill| include_bundled || skill.scope != SkillScope::Bundled)
        .filter_map(|skill| {
            let (name_match_result, score) = if query_text.is_empty() {
                (None, OrderedFloat(f64::MIN))
            } else {
                let match_result = match_indices_case_insensitive(skill.name.as_str(), query_text)?;
                if query_text.len() > 1 && match_result.score < 10 {
                    return None;
                }
                let score = OrderedFloat(match_result.score as f64);
                (Some(match_result), score)
            };

            Some(SelectableSkill {
                name: skill.name,
                reference: skill.reference,
                description: skill.description,
                scope: skill.scope,
                provider: skill.provider,
                icon_override: skill.icon_override,
                name_match_result,
                score,
            })
        })
        .collect::<Vec<_>>();

    // Inline menus render lower-ranked results first and select from the end.
    // Reverse alphabetical tie-breaking puts the alphabetically first skill at
    // the selected end of an unfiltered result list.
    results.sort_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then_with(|| right.name.to_lowercase().cmp(&left.name.to_lowercase()))
    });
    results
}
