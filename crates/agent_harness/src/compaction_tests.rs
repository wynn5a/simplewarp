use super::*;

fn user(tokens: u64) -> Entry {
    Entry {
        kind: EntryKind::User,
        tokens,
    }
}

fn assistant(tokens: u64) -> Entry {
    Entry {
        kind: EntryKind::Assistant,
        tokens,
    }
}

fn results(tokens: u64) -> Entry {
    Entry {
        kind: EntryKind::ToolResults,
        tokens,
    }
}

#[test]
fn compaction_triggers_past_the_reserve() {
    let settings = CompactionSettings::default();
    assert!(!should_compact(183_616, 200_000, &settings, None));
    assert!(should_compact(183_617, 200_000, &settings, None));

    let disabled = CompactionSettings {
        enabled: false,
        ..settings
    };
    assert!(!should_compact(199_000, 200_000, &disabled, None));

    // A reserve that swallows the whole window never triggers.
    assert!(!should_compact(10_000, 16_000, &settings, None));
}

#[test]
fn a_compaction_that_stayed_above_the_threshold_waits_for_new_content() {
    let settings = CompactionSettings::default();
    // The last compaction got well below the threshold: no extra condition.
    assert!(should_compact(190_000, 200_000, &settings, Some(20_000)));
    // It did not: compacting again only helps after another keep_recent of new content.
    assert!(!should_compact(200_000, 200_000, &settings, Some(190_000)));
    assert!(!should_compact(209_999, 200_000, &settings, Some(190_000)));
    assert!(should_compact(210_000, 200_000, &settings, Some(190_000)));
}

#[test]
fn keep_recent_is_capped_at_half_the_threshold() {
    let settings = CompactionSettings::default();
    assert_eq!(settings.keep_recent_for(32_768), 8_192);
    assert_eq!(settings.keep_recent_for(200_000), 20_000);
    assert_eq!(settings.keep_recent_for(10_000), 0);
}

#[test]
fn the_cut_keeps_the_newest_turns_within_the_budget() {
    let entries = [
        user(100),
        assistant(5_000),
        user(100),
        assistant(5_000),
        user(100),
        assistant(3_000),
    ];
    // 3,000 + 100 + 5,000 reaches 8,000 at the second assistant entry, which is a cut point.
    assert_eq!(
        find_cut_point(&entries, 8_000),
        Some(CutPoint {
            first_kept: 3,
            turn_start: Some(2),
        })
    );
    // 3,000 + 100 reaches 3,100 at the third user entry.
    assert_eq!(
        find_cut_point(&entries, 3_100),
        Some(CutPoint {
            first_kept: 4,
            turn_start: None,
        })
    );
}

#[test]
fn the_cut_never_separates_a_tool_call_from_its_result() {
    let entries = [
        user(100),
        assistant(100),
        results(9_000),
        assistant(100),
        results(100),
        assistant(100),
    ];
    // The budget is reached on the big results, which cannot be a cut, so the cut moves on to the
    // reply after them, keeping the results together with their call on the summarized side.
    let cut = find_cut_point(&entries, 5_000).unwrap();
    assert_eq!(cut.first_kept, 3);
    assert_ne!(entries[cut.first_kept].kind, EntryKind::ToolResults);
}

#[test]
fn a_turn_longer_than_the_budget_is_split_at_an_assistant_entry() {
    let entries = [
        user(100),
        assistant(100),
        user(100),
        assistant(4_000),
        results(4_000),
        assistant(4_000),
        results(4_000),
        assistant(1_000),
    ];
    let cut = find_cut_point(&entries, 8_000).unwrap();
    assert_eq!(cut.first_kept, 5);
    assert_eq!(entries[cut.first_kept].kind, EntryKind::Assistant);
    assert_eq!(cut.turn_start, Some(2));
}

#[test]
fn a_conversation_within_the_budget_keeps_only_its_newest_entry() {
    let entries = [user(100), assistant(100), results(100), assistant(100)];
    assert_eq!(
        find_cut_point(&entries, 20_000),
        Some(CutPoint {
            first_kept: 3,
            turn_start: Some(0),
        })
    );
}

#[test]
fn nothing_before_the_first_entry_is_nothing_to_compact() {
    assert_eq!(find_cut_point(&[user(100)], 20_000), None);
    // The budget is reached on the first entry, so the cut would keep everything.
    assert_eq!(find_cut_point(&[user(50_000), assistant(10)], 100), None);
    assert_eq!(find_cut_point(&[user(50_000), results(10)], 100), None);
    assert_eq!(find_cut_point(&[results(100)], 10), None);
    assert_eq!(find_cut_point(&[], 10), None);
}

#[test]
fn files_read_then_modified_are_listed_as_modified() {
    let mut ops = FileOps::default();
    ops.read("src/b.rs");
    ops.read("src/a.rs");
    ops.read("src/a.rs");
    ops.modified("src/b.rs");
    ops.modified("README.md");
    assert_eq!(
        ops.lists(),
        (
            vec!["src/a.rs".to_string()],
            vec!["README.md".to_string(), "src/b.rs".to_string()],
        )
    );
}

#[test]
fn file_lists_are_formatted_as_tagged_blocks() {
    let read = vec!["a.rs".to_string()];
    let modified = vec!["b.rs".to_string(), "c.rs".to_string()];
    let blocks = format_file_lists(&read, &modified);
    assert_eq!(
        blocks,
        "\n\n<read-files>\na.rs\n</read-files>\n\n<modified-files>\nb.rs\nc.rs\n</modified-files>"
    );
    assert_eq!(format_file_lists(&[], &[]), "");
    assert_eq!(
        format_file_lists(&[], &modified),
        "\n\n<modified-files>\nb.rs\nc.rs\n</modified-files>"
    );

    let summary = format!("## Goal\nShip it.{blocks}");
    assert_eq!(strip_file_lists(&summary), "## Goal\nShip it.");
    assert_eq!(strip_file_lists("## Goal\nShip it."), "## Goal\nShip it.");
}

#[test]
fn the_transcript_is_labelled_and_tool_results_are_truncated() {
    let long_result = "x".repeat(2_500);
    let text = serialize(&[
        TranscriptItem::User("Fix the build"),
        TranscriptItem::Assistant {
            text: "Reading it.",
            reasoning: "Need the manifest.",
            tool_calls: vec![r#"read_files(files=[{"path":"Cargo.toml"}])"#.to_string()],
        },
        TranscriptItem::ToolResult(&long_result),
    ]);
    let expected_result = format!(
        "[Tool result]: {}\n\n[... 500 more characters truncated]",
        "x".repeat(2_000)
    );
    assert_eq!(
        text,
        format!(
            "[User]: Fix the build\n\n[Assistant thinking]: Need the manifest.\n\n\
             [Assistant]: Reading it.\n\n\
             [Assistant tool calls]: read_files(files=[{{\"path\":\"Cargo.toml\"}}])\n\n\
             {expected_result}"
        )
    );
}

#[test]
fn truncation_counts_characters_not_bytes() {
    let result = "日".repeat(2_001);
    let text = serialize(&[TranscriptItem::ToolResult(&result)]);
    assert!(text.starts_with(&format!("[Tool result]: {}", "日".repeat(2_000))));
    assert!(text.ends_with("[... 1 more characters truncated]"));
}

#[test]
fn the_request_text_wraps_the_conversation_and_asks_for_the_fixed_structure() {
    let text = summary_request_text("[User]: hi", None, None, None);
    assert!(text.starts_with("<conversation>\n[User]: hi\n</conversation>\n\n"));
    for heading in [
        "## Goal",
        "## Constraints & Preferences",
        "## Progress",
        "### Done",
        "### In Progress",
        "### Blocked",
        "## Key Decisions",
        "## Next Steps",
        "## Critical Context",
    ] {
        assert!(text.contains(heading), "missing {heading}");
    }
    assert!(text.contains("The messages above are a conversation to summarize"));
    assert!(!text.contains("<previous-summary>"));
    assert!(!text.contains("<turn-prefix>"));
}

#[test]
fn an_update_carries_the_previous_summary() {
    let text = summary_request_text("[User]: more", None, Some("## Goal\nOld goal"), None);
    assert!(text.contains("<previous-summary>\n## Goal\nOld goal\n</previous-summary>"));
    assert!(text.contains("NEW conversation messages to incorporate"));
}

#[test]
fn a_split_turn_asks_for_the_turn_context_too() {
    let text = summary_request_text("", Some("[User]: big task"), None, None);
    assert!(text.contains("<turn-prefix>\n[User]: big task\n</turn-prefix>"));
    assert!(text.contains("**Turn Context (split turn):**"));
    assert!(text.contains("## Original Request"));
}

#[test]
fn custom_instructions_are_added_as_a_focus() {
    let text = summary_request_text("[User]: hi", None, None, Some("  the tests  "));
    assert!(text.ends_with("\n\nAdditional focus: the tests"));
    assert!(!summary_request_text("[User]: hi", None, None, Some(" ")).contains("Additional"));
}

#[test]
fn token_settings_are_clamped_to_a_sane_range() {
    let settings = CompactionSettings {
        enabled: true,
        reserve_tokens: 0,
        keep_recent_tokens: u32::MAX,
    }
    .clamped();
    assert_eq!(settings.reserve_tokens, MIN_TOKEN_SETTING);
    assert_eq!(settings.keep_recent_tokens, MAX_TOKEN_SETTING);
    assert_eq!(
        CompactionSettings::default().clamped(),
        CompactionSettings::default()
    );
}

#[test]
fn the_summary_output_cap_fits_the_reserve_and_the_window() {
    let settings = CompactionSettings::default();
    assert_eq!(settings.summary_max_output_tokens(None), 13_104);
    assert_eq!(settings.summary_max_output_tokens(Some(200_000)), 13_104);
    assert_eq!(settings.summary_max_output_tokens(Some(32_768)), 8_192);
    assert_eq!(settings.summary_max_output_tokens(Some(2_000)), 1_024);
}
