use fuzzy_match::FuzzyMatchResult;
use warpui::elements::{Highlight, Text};
use warpui::{AppContext, SingletonEntity};

use crate::appearance::Appearance;
use crate::notebooks::manager::NotebookManager;
use crate::search::result_renderer::ItemHighlightState;
use crate::server::ids::SyncId;

/// Renders the text element used in both command palette and command search
/// that displays notebook content with matching highlights. If there is a match,
/// the content is truncated up to the first match so we display only relevant information.
pub fn render_notebook_matched_content_with_highlight(
    notebook_id: SyncId,
    model_data: &str,
    content_match_result: &Option<FuzzyMatchResult>,
    highlight_state: ItemHighlightState,
    app: &AppContext,
) -> Text {
    let appearance = Appearance::as_ref(app);
    let parsed_raw_text = NotebookManager::as_ref(app)
        .notebook_raw_text(notebook_id)
        .unwrap_or(model_data);

    if let Some(match_result) = content_match_result {
        let first_match_index = match_result.matched_indices.first().unwrap_or(&0);
        // We only want the content starting at the match
        let matched_text_slice = parsed_raw_text
            .get(*first_match_index..)
            .unwrap_or_default();
        // Adjust the highlight indexes to be offset from the first index
        let adjusted_content_match_result: Vec<usize> = match_result
            .matched_indices
            .iter()
            .map(|idx| *idx - first_match_index)
            .collect();

        Text::new_inline(
            matched_text_slice.to_owned(),
            appearance.monospace_font_family(),
            appearance.monospace_font_size() - 2.,
        )
        .with_color(highlight_state.sub_text_fill(appearance).into_solid())
        .with_single_highlight(
            Highlight::new()
                .with_foreground_color(highlight_state.main_text_fill(appearance).into_solid()),
            adjusted_content_match_result,
        )
    } else {
        Text::new_inline(
            parsed_raw_text.to_owned(),
            appearance.ui_font_family(),
            appearance.monospace_font_size() - 2.,
        )
        .with_color(highlight_state.sub_text_fill(appearance).into_solid())
    }
}
