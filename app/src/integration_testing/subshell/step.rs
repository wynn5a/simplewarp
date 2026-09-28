use std::time::Duration;

use warpui::integration::{AssertionOutcome, TestStep};
use warpui::{async_assert, async_assert_eq};

use crate::integration_testing::step::assert_no_pending_model_events;
use crate::integration_testing::terminal::wait_until_bootstrapped_pane;
use crate::integration_testing::view_getters::{single_terminal_view, terminal_view};
use crate::terminal::model::rich_content::RichContentType;
use crate::terminal::view::WithinBlockBanner;

pub fn enter_local_subshell_command(shell: &str) -> TestStep {
    TestStep::new(&format!("Enter local subshell command for {shell}"))
        .with_input_string(shell, Some(&["enter"]))
        // Wait for shell line editor to become active before moving to next test step.
        .set_post_step_pause(Duration::from_millis(50))
}

pub fn assert_subshell_banner_is_showing() -> TestStep {
    TestStep::new("Assert the Warpify banner is visible")
        .add_assertion(move |app, window_id| {
            let terminal_view = single_terminal_view(app, window_id);
            terminal_view.read(app, |view, _ctx| {
                async_assert!(matches!(
                    view.model
                        .lock()
                        .block_list_mut()
                        .active_block()
                        .block_banner(),
                    Some(WithinBlockBanner::WarpifyBanner(..))
                ))
            })
        })
        // Wait for outstanding model events to finish before moving to the next step
        .add_named_assertion("no pending model events", assert_no_pending_model_events())
        .set_post_step_pause(Duration::from_millis(50))
}

pub fn trigger_subshell_bootstrap() -> TestStep {
    TestStep::new("Trigger subshell bootstrap").with_keystrokes(&["ctrl-i"])
}

pub fn assert_subshell_is_bootstrapped(tab_index: usize, pane_index: usize) -> TestStep {
    wait_until_bootstrapped_pane(tab_index, pane_index).add_named_assertion(
        "Subshell info block was displayed and no extraneous blocks added",
        move |app, window_id| {
            let terminal_view = terminal_view(app, window_id, tab_index, pane_index);
            terminal_view.read(app, |view, _ctx| {
                let model = view.model.lock();

                let Some((success_block_index, rich_content_type)) = model
                    .block_list()
                    .last_non_hidden_rich_content_block_after_block(None)
                    .map(|(success_block_index, block)| (success_block_index, block.content_type))
                else {
                    return AssertionOutcome::failure("No rich content block found!".to_owned());
                };

                match rich_content_type {
                    Some(RichContentType::WarpifySuccessBlock) => {}
                    _ => {
                        return AssertionOutcome::failure(
                            "Warpify success block wasn't added to the blocklist".to_owned(),
                        );
                    }
                }

                let success_block_index: usize = success_block_index.into();
                // Make sure there are no non-in-band-generator blocks added to the blocklist in
                // between the static block and the active block (which is not yet finished).
                async_assert_eq!(
                    model.block_list().blocks()[success_block_index + 1..]
                        .iter()
                        .filter(|block| !block.is_in_band_command_block() && block.finished())
                        .count(),
                    0,
                    "Added extraneous blocks to the block list.",
                )
            })
        },
    )
}
