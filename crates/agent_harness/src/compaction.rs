//! Compaction: replacing the older part of a conversation with a structured summary so the rest
//! fits in the model's context window.
//!
//! Ported from pi's compaction (`@mariozechner/pi-coding-agent` 0.73.1, `docs/compaction.md`):
//! compact once the context is within `reserve_tokens` of the window, keep roughly the newest
//! `keep_recent_tokens` verbatim, and summarize everything before that in a fixed Markdown
//! structure, updating the previous summary instead of starting over.

use std::collections::BTreeSet;

use crate::budget::estimate_tokens_for_chars;

/// Tokens left free below the window before compaction triggers.
pub const DEFAULT_RESERVE_TOKENS: u32 = 16_384;

/// Estimated tokens at the end of the conversation kept verbatim.
pub const DEFAULT_KEEP_RECENT_TOKENS: u32 = 20_000;

/// The range a token-count setting is clamped to.
pub const MIN_TOKEN_SETTING: u32 = 1_000;
pub const MAX_TOKEN_SETTING: u32 = 1_000_000;

/// The smallest output cap a summary request gets.
const MIN_SUMMARY_OUTPUT_TOKENS: u32 = 1_024;

/// The longest tool result, in characters, that a summary request shows in full.
pub const TOOL_RESULT_MAX_CHARS: usize = 2_000;

/// When and how much to compact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionSettings {
    /// Whether the agent compacts on its own; `/compact` works either way.
    pub enabled: bool,
    pub reserve_tokens: u32,
    pub keep_recent_tokens: u32,
}

impl Default for CompactionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            reserve_tokens: DEFAULT_RESERVE_TOKENS,
            keep_recent_tokens: DEFAULT_KEEP_RECENT_TOKENS,
        }
    }
}

impl CompactionSettings {
    /// The settings with each token count clamped to [`MIN_TOKEN_SETTING`]..=[`MAX_TOKEN_SETTING`],
    /// so a hand-edited settings file cannot turn compaction into nonsense.
    pub fn clamped(self) -> Self {
        let clamp = |tokens: u32| tokens.clamp(MIN_TOKEN_SETTING, MAX_TOKEN_SETTING);
        Self {
            reserve_tokens: clamp(self.reserve_tokens),
            keep_recent_tokens: clamp(self.keep_recent_tokens),
            ..self
        }
    }

    /// The usage above which a conversation is compacted.
    pub fn threshold(&self, window: u32) -> u64 {
        u64::from(window.saturating_sub(self.reserve_tokens))
    }

    /// The tokens to keep verbatim in a window of this size.
    ///
    /// Capped at half the threshold, so that one compaction brings a small window back under it:
    /// a 32k window with the default reserve has a threshold of 16,384, below the default keep.
    /// 0 when the reserve is as large as the window: nothing is kept, and a summary replaces the
    /// whole conversation.
    pub fn keep_recent_for(&self, window: u32) -> u32 {
        let half_threshold = u32::try_from(self.threshold(window) / 2).unwrap_or(u32::MAX);
        self.keep_recent_tokens.min(half_threshold)
    }

    /// The output cap of a summary request: four fifths of the reserve (pi's rule, so the summary
    /// fits in the room the reserve keeps), and no more than a quarter of the window when it is
    /// known, so a small model's input and output still fit together.
    pub fn summary_max_output_tokens(&self, window: Option<u32>) -> u32 {
        let from_reserve = self.reserve_tokens / 5 * 4;
        let cap = window.map_or(from_reserve, |window| from_reserve.min(window / 4));
        cap.max(MIN_SUMMARY_OUTPUT_TOKENS)
    }
}

/// Whether a conversation using `used` tokens of `window` should be compacted before its next
/// request.
///
/// `last_compacted_to` is the usage the previous compaction left, if there was one. When that was
/// still above the threshold, the kept tail alone is too big, and compacting again only helps
/// once another `keep_recent` worth of new content can move the cut past it.
pub fn should_compact(
    used: u64,
    window: u32,
    settings: &CompactionSettings,
    last_compacted_to: Option<u64>,
) -> bool {
    let threshold = settings.threshold(window);
    // A reserve as large as the window would compact before every request.
    if !settings.enabled || threshold == 0 || used <= threshold {
        return false;
    }
    match last_compacted_to {
        Some(after) if after > threshold => {
            used >= after.saturating_add(u64::from(settings.keep_recent_for(window)))
        }
        Some(_) | None => true,
    }
}

/// What a conversation entry is, as far as cutting is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    User,
    /// A reply, with any tool calls it made.
    Assistant,
    /// The results of the tool calls in the entry before.
    ToolResults,
}

/// One entry of the conversation, oldest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    /// Estimated tokens.
    pub tokens: u64,
}

/// Where to cut a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutPoint {
    /// The first entry kept verbatim. Everything before it is summarized.
    pub first_kept: usize,
    /// When the cut falls inside a turn, the user entry that opened it. The entries from there to
    /// the cut are summarized as that turn's prefix, separately from the history before it.
    pub turn_start: Option<usize>,
}

/// Picks the cut that keeps about `keep_recent_tokens` of the newest entries.
///
/// Walking back from the newest entry, the cut is the first user or assistant entry at or after
/// the one where the budget is reached. Tool results are never a cut, so a call and its results
/// stay on the same side. When the whole conversation is within the budget the newest user or
/// assistant entry is the cut, so that compacting always makes progress. `None` when nothing
/// would be summarized.
pub fn find_cut_point(entries: &[Entry], keep_recent_tokens: u64) -> Option<CutPoint> {
    let is_cut_point = |index: usize| match entries[index].kind {
        EntryKind::User | EntryKind::Assistant => true,
        EntryKind::ToolResults => false,
    };
    let newest_cut_point = (0..entries.len())
        .rev()
        .find(|index| is_cut_point(*index))?;

    let mut accumulated = 0_u64;
    let mut cut = newest_cut_point;
    for index in (0..entries.len()).rev() {
        accumulated = accumulated.saturating_add(entries[index].tokens);
        if accumulated >= keep_recent_tokens {
            cut = (index..entries.len())
                .find(|candidate| is_cut_point(*candidate))
                .unwrap_or(newest_cut_point);
            break;
        }
    }

    if cut == 0 {
        return None;
    }
    let turn_start = match entries[cut].kind {
        EntryKind::User => None,
        EntryKind::Assistant | EntryKind::ToolResults => (0..cut)
            .rev()
            .find(|index| entries[*index].kind == EntryKind::User),
    };
    Some(CutPoint {
        first_kept: cut,
        turn_start,
    })
}

/// The files a conversation read and modified, gathered from its tool calls.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileOps {
    read: BTreeSet<String>,
    modified: BTreeSet<String>,
}

impl FileOps {
    pub fn read(&mut self, path: impl Into<String>) {
        self.read.insert(path.into());
    }

    pub fn modified(&mut self, path: impl Into<String>) {
        self.modified.insert(path.into());
    }

    /// The files only read, and the files modified, each sorted.
    pub fn lists(&self) -> (Vec<String>, Vec<String>) {
        let read = self
            .read
            .iter()
            .filter(|path| !self.modified.contains(*path))
            .cloned()
            .collect();
        (read, self.modified.iter().cloned().collect())
    }
}

const READ_FILES_OPEN: &str = "<read-files>";
const READ_FILES_CLOSE: &str = "</read-files>";
const MODIFIED_FILES_OPEN: &str = "<modified-files>";
const MODIFIED_FILES_CLOSE: &str = "</modified-files>";

/// The file lists as the tagged blocks that end a summary, or nothing when both are empty.
pub fn format_file_lists(read: &[String], modified: &[String]) -> String {
    let mut sections = Vec::new();
    if !read.is_empty() {
        sections.push(format!(
            "{READ_FILES_OPEN}\n{}\n{READ_FILES_CLOSE}",
            read.join("\n")
        ));
    }
    if !modified.is_empty() {
        sections.push(format!(
            "{MODIFIED_FILES_OPEN}\n{}\n{MODIFIED_FILES_CLOSE}",
            modified.join("\n")
        ));
    }
    if sections.is_empty() {
        return String::new();
    }
    format!("\n\n{}", sections.join("\n\n"))
}

/// A summary without the file-list blocks that [`format_file_lists`] appended to it.
pub fn strip_file_lists(summary: &str) -> &str {
    let start = [READ_FILES_OPEN, MODIFIED_FILES_OPEN]
        .iter()
        .filter_map(|tag| summary.rfind(&format!("\n\n{tag}\n")))
        .min();
    match start {
        Some(start) => summary[..start].trim_end(),
        None => summary,
    }
}

/// One message of the conversation to summarize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptItem<'a> {
    User(&'a str),
    Assistant {
        text: &'a str,
        reasoning: &'a str,
        /// Each call as `name(key=value, …)`, values JSON-encoded.
        tool_calls: Vec<String>,
    },
    ToolResult(&'a str),
}

/// The conversation as labelled text.
///
/// The model is asked to summarize text rather than shown the messages themselves, so that it
/// does not carry on the conversation. Tool results are cut to [`TOOL_RESULT_MAX_CHARS`]: the
/// summary needs what a result showed, not all of it.
pub fn serialize(items: &[TranscriptItem]) -> String {
    let mut parts = Vec::new();
    for item in items {
        match item {
            TranscriptItem::User(text) => {
                if !text.is_empty() {
                    parts.push(format!("[User]: {text}"));
                }
            }
            TranscriptItem::Assistant {
                text,
                reasoning,
                tool_calls,
            } => {
                if !reasoning.is_empty() {
                    parts.push(format!("[Assistant thinking]: {reasoning}"));
                }
                if !text.is_empty() {
                    parts.push(format!("[Assistant]: {text}"));
                }
                if !tool_calls.is_empty() {
                    parts.push(format!("[Assistant tool calls]: {}", tool_calls.join("; ")));
                }
            }
            TranscriptItem::ToolResult(content) => {
                if !content.is_empty() {
                    parts.push(format!(
                        "[Tool result]: {}",
                        truncate_for_summary(content, TOOL_RESULT_MAX_CHARS)
                    ));
                }
            }
        }
    }
    parts.join("\n\n")
}

/// Estimated tokens of a transcript item, as [`find_cut_point`] counts them.
pub fn estimate_item_tokens(item: &TranscriptItem) -> u64 {
    let chars = match item {
        TranscriptItem::User(text) | TranscriptItem::ToolResult(text) => text.chars().count(),
        TranscriptItem::Assistant {
            text,
            reasoning,
            tool_calls,
        } => {
            text.chars().count()
                + reasoning.chars().count()
                + tool_calls
                    .iter()
                    .map(|call| call.chars().count())
                    .sum::<usize>()
        }
    };
    estimate_tokens_for_chars(chars as u64)
}

fn truncate_for_summary(text: &str, max_chars: usize) -> String {
    let Some((cut, _)) = text.char_indices().nth(max_chars) else {
        return text.to_string();
    };
    let truncated = text[cut..].chars().count();
    format!(
        "{}\n\n[... {truncated} more characters truncated]",
        &text[..cut]
    )
}

/// The system prompt of a summary request.
pub const SUMMARIZATION_SYSTEM_PROMPT: &str = "You are a context summarization assistant. Your \
task is to read a conversation between a user and an AI coding assistant, then produce a \
structured summary following the exact format specified.

Do NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY \
output the structured summary.";

const SUMMARIZATION_PROMPT: &str = "The messages above are a conversation to summarize. Create a \
structured context checkpoint summary that another LLM will use to continue the work.

Use this EXACT format:

## Goal
[What is the user trying to accomplish? Can be multiple items if the session covers different tasks.]

## Constraints & Preferences
- [Any constraints, preferences, or requirements mentioned by user]
- [Or \"(none)\" if none were mentioned]

## Progress
### Done
- [x] [Completed tasks/changes]

### In Progress
- [ ] [Current work]

### Blocked
- [Issues preventing progress, if any]

## Key Decisions
- **[Decision]**: [Brief rationale]

## Next Steps
1. [Ordered list of what should happen next]

## Critical Context
- [Any data, examples, or references needed to continue]
- [Or \"(none)\" if not applicable]

Keep each section concise. Preserve exact file paths, function names, and error messages.";

const UPDATE_SUMMARIZATION_PROMPT: &str = "The messages above are NEW conversation messages to \
incorporate into the existing summary provided in <previous-summary> tags.

Update the existing structured summary with new information. RULES:
- PRESERVE all existing information from the previous summary
- ADD new progress, decisions, and context from the new messages
- UPDATE the Progress section: move items from \"In Progress\" to \"Done\" when completed
- UPDATE \"Next Steps\" based on what was accomplished
- PRESERVE exact file paths, function names, and error messages
- If something is no longer relevant, you may remove it

Use this EXACT format:

## Goal
[Preserve existing goals, add new ones if the task expanded]

## Constraints & Preferences
- [Preserve existing, add new ones discovered]

## Progress
### Done
- [x] [Include previously done items AND newly completed items]

### In Progress
- [ ] [Current work - update based on progress]

### Blocked
- [Current blockers - remove if resolved]

## Key Decisions
- **[Decision]**: [Brief rationale] (preserve all previous, add new)

## Next Steps
1. [Update based on current state]

## Critical Context
- [Preserve important context, add new if needed]

Keep each section concise. Preserve exact file paths, function names, and error messages.";

/// pi summarizes a split turn's prefix in a second request; here one request asks for both
/// summaries, so the streamed summary stays a single message.
const TURN_PREFIX_PROMPT: &str = "The messages in <turn-prefix> tags are the PREFIX of a turn \
that was too large to keep. The SUFFIX (recent work) is retained and not shown here.

After the summary above, add a line with `---`, then a line `**Turn Context (split turn):**`, \
then summarize the prefix to provide context for the retained suffix:

## Original Request
[What did the user ask for in this turn?]

## Early Progress
- [Key decisions and work done in the prefix]

## Context for Suffix
- [Information needed to understand the retained recent work]

Be concise. Focus on what's needed to understand the kept suffix.";

/// The text of a summary request: the serialized `history`, the split turn's `turn_prefix`, the
/// `previous_summary` to update, and the user's `/compact` `instructions`.
pub fn summary_request_text(
    history: &str,
    turn_prefix: Option<&str>,
    previous_summary: Option<&str>,
    instructions: Option<&str>,
) -> String {
    let mut text = format!("<conversation>\n{history}\n</conversation>\n\n");
    if let Some(prefix) = turn_prefix {
        text.push_str(&format!("<turn-prefix>\n{prefix}\n</turn-prefix>\n\n"));
    }
    match previous_summary {
        Some(previous) => {
            text.push_str(&format!(
                "<previous-summary>\n{previous}\n</previous-summary>\n\n"
            ));
            text.push_str(UPDATE_SUMMARIZATION_PROMPT);
        }
        None => text.push_str(SUMMARIZATION_PROMPT),
    }
    if turn_prefix.is_some() {
        text.push_str("\n\n");
        text.push_str(TURN_PREFIX_PROMPT);
    }
    if let Some(instructions) = instructions.map(str::trim).filter(|text| !text.is_empty()) {
        text.push_str(&format!("\n\nAdditional focus: {instructions}"));
    }
    text
}

#[cfg(test)]
#[path = "compaction_tests.rs"]
mod tests;
