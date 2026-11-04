//! What the footer's context meter shows for a conversation: how full the window is, how urgent
//! that is, and whether clicking it compacts the conversation.

use agent_harness::budget::{ContextUsage, WARNING_FRACTION, format_tokens};
use agent_harness::compaction::CompactionSettings;

/// What the active conversation is doing, as far as compacting it on demand is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConversationActivity {
    Idle,
    Running,
    Compacting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MeterLevel {
    Normal,
    /// At or above [`WARNING_FRACTION`] of the window.
    Warning,
    /// Past the auto-compaction threshold, or over the window itself.
    Critical,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ContextMeter {
    /// Share of the window filled by the bar, clamped to `0.0..=1.0`.
    pub fill: f32,
    pub level: MeterLevel,
    pub label: String,
    pub tooltip: String,
    pub tooltip_detail: String,
    pub can_compact: bool,
}

pub(super) fn context_meter(
    usage: ContextUsage,
    window: u32,
    compaction: CompactionSettings,
    activity: ConversationActivity,
) -> ContextMeter {
    let used = usage.used_tokens();
    let fraction = usage.fraction_used(window).unwrap_or(0.);
    let percent = (fraction * 100.).round() as u64;
    let threshold = compaction.threshold(window);

    let level = if fraction >= 1. || (compaction.enabled && used >= threshold) {
        MeterLevel::Critical
    } else if fraction >= WARNING_FRACTION {
        MeterLevel::Warning
    } else {
        MeterLevel::Normal
    };

    let label = match activity {
        ConversationActivity::Compacting => "Compacting…".to_owned(),
        ConversationActivity::Idle | ConversationActivity::Running => format!("{percent}%"),
    };

    let auto_compaction = if compaction.enabled {
        format!("Auto-compacts at {}", format_tokens(threshold))
    } else {
        "Auto-compaction is off".to_owned()
    };
    let action = match activity {
        ConversationActivity::Idle => "Click to compact now",
        ConversationActivity::Running => "Compact once the agent is idle",
        ConversationActivity::Compacting => "Summarizing earlier messages",
    };

    ContextMeter {
        fill: fraction.clamp(0., 1.) as f32,
        level,
        label,
        tooltip: format!(
            "Context: {} of {} tokens ({percent}%)",
            format_tokens(used),
            format_tokens(u64::from(window))
        ),
        tooltip_detail: format!("{auto_compaction} · {action}"),
        can_compact: activity == ConversationActivity::Idle,
    }
}

#[cfg(test)]
#[path = "context_meter_tests.rs"]
mod tests;
