use warp_cli::CliCommand;
use warp_cli::agent::Harness;

use super::{command_requires_auth, reconcile_task_harness};

const TASK_ID: &str = "00000000-0000-0000-0000-000000000001";

#[test]
fn logout_does_not_require_auth() {
    assert!(!command_requires_auth(&CliCommand::Logout));
}

#[test]
fn reconcile_task_harness_adopts_task_harness_when_cli_uses_default() {
    let mut selected_harness = Harness::Oz;
    let harness = reconcile_task_harness(TASK_ID, &mut selected_harness, Harness::Claude)
        .expect("default harness should adopt task harness");

    assert_eq!(selected_harness, Harness::Claude);
    assert_eq!(harness.harness(), Harness::Claude);
}

#[test]
fn reconcile_task_harness_allows_matching_explicit_harness() {
    let mut selected_harness = Harness::Claude;
    let harness = reconcile_task_harness(TASK_ID, &mut selected_harness, Harness::Claude)
        .expect("matching harness should succeed");

    assert_eq!(selected_harness, Harness::Claude);
    assert_eq!(harness.harness(), Harness::Claude);
}

#[test]
fn reconcile_task_harness_rejects_explicit_mismatch() {
    let mut selected_harness = Harness::Gemini;
    let err = reconcile_task_harness(TASK_ID, &mut selected_harness, Harness::Claude)
        .expect_err("mismatched harness should fail");

    assert_eq!(selected_harness, Harness::Gemini);
    assert!(err.to_string().contains("Task"));
    assert!(err.to_string().contains("--harness gemini"));
    assert!(err.to_string().contains("claude"));
}
