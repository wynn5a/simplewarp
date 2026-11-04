//! What the footer's usage capsules show for a conversation: how full the context window is, how
//! urgent that is, whether clicking it compacts the conversation, and the tokens sent and received.

use agent_harness::budget::{ContextUsage, WARNING_FRACTION, format_tokens};
use agent_harness::compaction::CompactionSettings;
use persistence::model::ConversationTokenUsage;

use crate::ai::conversation_details_panel::format_token_count;

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
    /// "used/window", e.g. `104.8k/1M`.
    pub usage_label: String,
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
        usage_label: format!(
            "{}/{}",
            format_token_count(used),
            format_tokens(u64::from(window))
        ),
        tooltip: format!(
            "Context: {} of {} tokens ({percent}%)",
            format_tokens(used),
            format_tokens(u64::from(window))
        ),
        tooltip_detail: format!("{auto_compaction} · {action}"),
        can_compact: activity == ConversationActivity::Idle,
    }
}

/// One of the capsules counting the tokens a conversation has sent or received so far.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct TokenCapsule {
    pub label: String,
    pub tooltip: String,
    pub tooltip_detail: String,
}

/// The input and output capsules, or `None` before any request has reported its usage.
pub(super) fn token_capsules(usage: ConversationTokenUsage) -> Option<[TokenCapsule; 2]> {
    if usage == ConversationTokenUsage::default() {
        return None;
    }
    let mut cache = Vec::new();
    if usage.cached_input != 0 {
        cache.push(format!(
            "{} read from cache",
            format_token_count(usage.cached_input)
        ));
    }
    if usage.cache_write != 0 {
        cache.push(format!(
            "{} written to cache",
            format_token_count(usage.cache_write)
        ));
    }
    let input_detail = if cache.is_empty() {
        "Summed over every request, each of which resends the conversation".to_owned()
    } else {
        cache.join(" · ")
    };
    Some([
        TokenCapsule {
            label: format_token_count(usage.input),
            tooltip: format!(
                "Input: {} tokens sent in this conversation",
                format_token_count(usage.input)
            ),
            tooltip_detail: input_detail,
        },
        TokenCapsule {
            label: format_token_count(usage.output),
            tooltip: format!(
                "Output: {} tokens generated in this conversation",
                format_token_count(usage.output)
            ),
            tooltip_detail: "Summed over every reply".to_owned(),
        },
    ])
}

#[cfg(test)]
#[path = "context_meter_tests.rs"]
mod tests;
