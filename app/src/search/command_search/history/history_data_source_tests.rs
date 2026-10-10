use super::*;

fn entries(commands: &[&str]) -> Vec<Arc<HistoryEntry>> {
    commands
        .iter()
        .map(|command| Arc::new(HistoryEntry::command_only(*command)))
        .collect()
}

fn bonus_for(ranked: &[(Arc<HistoryEntry>, i64)], command: &str) -> i64 {
    ranked
        .iter()
        .find(|(entry, _)| entry.command == command)
        .map(|(_, bonus)| *bonus)
        .unwrap_or_else(|| panic!("{command} missing from {ranked:?}"))
}

#[test]
fn repeated_commands_collapse_to_their_latest_run() {
    let ranked =
        collapse_with_rank_bonus(&entries(&["ls", "git status", "ls", "cargo test", "ls"]));

    let commands: Vec<&str> = ranked
        .iter()
        .map(|(entry, _)| entry.command.as_str())
        .collect();
    assert_eq!(commands, ["git status", "cargo test", "ls"]);
}

#[test]
fn frequently_run_commands_get_a_larger_bonus_than_single_runs_at_the_same_recency() {
    let ranked = collapse_with_rank_bonus(&entries(&["make", "make", "make", "make", "build"]));
    let single_run = collapse_with_rank_bonus(&entries(&["make", "build"]));

    assert!(bonus_for(&ranked, "make") > bonus_for(&single_run, "make"));
}

#[test]
fn recent_commands_outrank_older_ones_with_the_same_run_count() {
    let ranked = collapse_with_rank_bonus(&entries(&["old", "middle", "new"]));

    assert!(bonus_for(&ranked, "new") > bonus_for(&ranked, "middle"));
    assert!(bonus_for(&ranked, "middle") > bonus_for(&ranked, "old"));
}

#[test]
fn usage_bonus_is_capped() {
    let many_runs = vec!["make"; 10_000];
    let ranked = collapse_with_rank_bonus(&entries(&many_runs));

    // The only command is also the newest, so it earns the full recency bonus on top of the cap.
    assert_eq!(
        bonus_for(&ranked, "make"),
        (MAX_USAGE_BONUS + MAX_RECENCY_BONUS) as i64
    );
}

#[test]
fn a_single_command_gets_no_bonus() {
    let ranked = collapse_with_rank_bonus(&entries(&["ls"]));

    assert_eq!(bonus_for(&ranked, "ls"), 0);
}

#[test]
fn ranking_empty_history_yields_nothing() {
    assert!(collapse_with_rank_bonus(&[]).is_empty());
}
