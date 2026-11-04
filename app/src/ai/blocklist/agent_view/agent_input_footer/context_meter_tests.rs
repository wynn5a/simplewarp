use agent_harness::budget::ContextUsage;
use agent_harness::compaction::CompactionSettings;

use super::{ConversationActivity, MeterLevel, context_meter};

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
