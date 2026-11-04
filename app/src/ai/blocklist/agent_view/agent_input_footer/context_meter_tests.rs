use agent_harness::budget::ContextUsage;
use agent_harness::compaction::CompactionSettings;
use persistence::model::ConversationTokenUsage;

use super::{ConversationActivity, MeterLevel, context_meter, token_capsules};

const WINDOW: u32 = 200_000;

fn usage(tokens: u64) -> ContextUsage {
    ContextUsage::from_tokens(tokens)
}

#[test]
fn idle_meter_shows_percent_and_offers_compaction() {
    let meter = context_meter(
        usage(76_000),
        WINDOW,
        CompactionSettings::default(),
        ConversationActivity::Idle,
    );
    assert_eq!(meter.label, "38%");
    assert_eq!(meter.usage_label, "76.0k/200k");
    assert_eq!(meter.level, MeterLevel::Normal);
    assert!((meter.fill - 0.38).abs() < 1e-6);
    assert_eq!(meter.tooltip, "Context: 76k of 200k tokens (38%)");
    assert_eq!(
        meter.tooltip_detail,
        "Auto-compacts at 184k · Click to compact now"
    );
    assert!(meter.can_compact);
}

#[test]
fn warns_from_eighty_percent() {
    let meter = context_meter(
        usage(160_000),
        WINDOW,
        CompactionSettings::default(),
        ConversationActivity::Idle,
    );
    assert_eq!(meter.level, MeterLevel::Warning);
}

#[test]
fn critical_past_the_auto_compaction_threshold() {
    let meter = context_meter(
        usage(190_000),
        WINDOW,
        CompactionSettings::default(),
        ConversationActivity::Idle,
    );
    assert_eq!(meter.level, MeterLevel::Critical);
}

#[test]
fn without_auto_compaction_only_overflow_is_critical() {
    let settings = CompactionSettings {
        enabled: false,
        ..CompactionSettings::default()
    };
    let near = context_meter(usage(190_000), WINDOW, settings, ConversationActivity::Idle);
    assert_eq!(near.level, MeterLevel::Warning);
    assert!(near.tooltip_detail.starts_with("Auto-compaction is off"));

    let over = context_meter(usage(210_000), WINDOW, settings, ConversationActivity::Idle);
    assert_eq!(over.level, MeterLevel::Critical);
    assert_eq!(over.label, "105%");
    assert_eq!(over.fill, 1.);
}

#[test]
fn busy_conversations_cannot_be_compacted_from_the_meter() {
    let running = context_meter(
        usage(76_000),
        WINDOW,
        CompactionSettings::default(),
        ConversationActivity::Running,
    );
    assert!(!running.can_compact);
    assert!(
        running
            .tooltip_detail
            .ends_with("Compact once the agent is idle")
    );

    let compacting = context_meter(
        usage(76_000),
        WINDOW,
        CompactionSettings::default(),
        ConversationActivity::Compacting,
    );
    assert!(!compacting.can_compact);
    assert_eq!(compacting.label, "Compacting…");
}

#[test]
fn usage_label_uses_the_compact_window() {
    let meter = context_meter(
        usage(104_800),
        1_000_000,
        CompactionSettings::default(),
        ConversationActivity::Idle,
    );
    assert_eq!(meter.label, "10%");
    assert_eq!(meter.usage_label, "104.8k/1M");
}

#[test]
fn no_token_capsules_before_any_report() {
    assert_eq!(token_capsules(ConversationTokenUsage::default()), None);
}

#[test]
fn token_capsules_count_input_and_output() {
    let [input, output] = token_capsules(ConversationTokenUsage {
        input: 104_900,
        cached_input: 80_000,
        cache_write: 0,
        output: 8_000,
    })
    .unwrap();
    assert_eq!(input.label, "104.9k");
    assert_eq!(
        input.tooltip,
        "Input: 104.9k tokens sent in this conversation"
    );
    assert_eq!(input.tooltip_detail, "80.0k read from cache");
    assert_eq!(output.label, "8.0k");
    assert_eq!(
        output.tooltip,
        "Output: 8.0k tokens generated in this conversation"
    );
}
