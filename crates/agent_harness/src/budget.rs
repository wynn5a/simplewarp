//! How full a conversation's context window is.
//!
//! Exact counts come from the provider, after each request. Between requests the app only has
//! text, which [`estimate_tokens`] turns into an approximate count, so [`ContextUsage`] is the
//! last exact report plus estimates of what was added since.

/// The share of the window above which the usage is shown as a warning.
pub const WARNING_FRACTION: f64 = 0.8;

/// Approximate tokens in `text`, rounded up.
///
/// One token per four characters, the common rule of thumb for English prose and code with the
/// BPE tokenizers the supported providers use. A real tokenizer would be exact for one provider and
/// wrong for the others, and the estimate only bridges the gap until the provider reports the
/// exact count for the next request.
pub fn estimate_tokens(text: &str) -> u64 {
    estimate_tokens_for_chars(text.chars().count() as u64)
}

/// [`estimate_tokens`] for text of `chars` characters, for text that arrives in pieces.
pub fn estimate_tokens_for_chars(chars: u64) -> u64 {
    chars.div_ceil(4)
}

/// The window to measure usage against: the model's own window, capped by the user's limit.
///
/// Either may be unknown. With neither, there is nothing to measure against.
pub fn effective_window(model_window: Option<u32>, limit: Option<u32>) -> Option<u32> {
    match (model_window, limit.filter(|limit| *limit > 0)) {
        (Some(window), Some(limit)) => Some(window.min(limit)),
        (window, None) => window,
        (None, limit) => limit,
    }
}

/// How many tokens a conversation occupies in the model's context.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContextUsage {
    /// Input tokens of the newest request the provider reported on, cached ones included. This is
    /// the whole context as sent, not a sum over requests.
    last_reported_input: u64,
    /// Output tokens of that request's reply, which the next request carries as history.
    last_reported_output: u64,
    /// Estimated tokens of the request in flight, which that report does not count.
    estimated_since: u64,
}

impl ContextUsage {
    /// Usage restored from a stored total, with nothing estimated on top.
    pub fn from_tokens(tokens: u64) -> Self {
        Self {
            last_reported_input: tokens,
            ..Self::default()
        }
    }

    /// Replaces the usage with a provider's report for one request, dropping earlier estimates:
    /// the report already counts what they stood for.
    pub fn record_report(&mut self, input: u64, output: u64) {
        *self = Self {
            last_reported_input: input,
            last_reported_output: output,
            estimated_since: 0,
        };
    }

    /// Sets the estimate for the request in flight, which the next report will count exactly.
    ///
    /// It replaces an earlier estimate rather than adding to it: a request that is sent again
    /// carries the same content, and counting it twice would overstate the usage. The cost is that
    /// a request that failed without a report and is followed by a different one is not counted
    /// until the next report.
    pub fn set_estimate(&mut self, tokens: u64) {
        self.estimated_since = tokens;
    }

    /// True once a provider has reported on a request, or a stored total was restored.
    pub fn has_report(&self) -> bool {
        self.last_reported_input > 0
    }

    /// The tokens the conversation occupies: the last report plus the estimates since.
    pub fn used_tokens(&self) -> u64 {
        self.last_reported_input
            .saturating_add(self.last_reported_output)
            .saturating_add(self.estimated_since)
    }

    /// The share of `window` in use, which may exceed 1. `None` for an empty window.
    pub fn fraction_used(&self, window: u32) -> Option<f64> {
        (window > 0).then(|| self.used_tokens() as f64 / f64::from(window))
    }

    /// True when the usage is at or above [`WARNING_FRACTION`] of `window`.
    pub fn is_near_limit(&self, window: u32) -> bool {
        self.fraction_used(window)
            .is_some_and(|fraction| fraction >= WARNING_FRACTION)
    }
}

/// A token count in the compact form the footer shows, rounded to the nearest unit: `950`, `42k`,
/// `1.2M`.
pub fn format_tokens(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    let thousands = (tokens + 500) / 1_000;
    if thousands < 1_000 {
        return format!("{thousands}k");
    }
    let tenths_of_millions = (tokens + 50_000) / 100_000;
    match tenths_of_millions % 10 {
        0 => format!("{}M", tenths_of_millions / 10),
        tenth => format!("{}.{tenth}M", tenths_of_millions / 10),
    }
}

#[cfg(test)]
#[path = "budget_tests.rs"]
mod tests;
