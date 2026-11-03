//! Builds the request that compacts a conversation.
//!
//! A `/compact` request, or one the client sends on its own when the context fills up, asks the
//! model for a summary of the older part of the conversation. The cut, the previous summary to
//! update and the file lists all come from the conversation the request carries, as
//! [`crate::convert`] replays it, so the cut always falls on a turn the replay knows. The rules are
//! [`agent_harness::compaction`]'s; this module maps the proto conversation onto them.

use agent_harness::budget::estimate_tokens;
use agent_harness::compaction::{
    Entry, EntryKind, FileOps, TranscriptItem, estimate_item_tokens, find_cut_point, serialize,
    strip_file_lists, summary_request_text,
};
use serde_json::Value;
use warp_multi_agent_api as api;

use crate::convert::{self, Turn};
use crate::inputs;

/// A summary request, ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    /// The one user turn the summarizing model is shown.
    pub prompt_text: String,
    /// The output cap the client asked for.
    pub max_output_tokens: Option<u32>,
    pub outcome: Outcome,
}

/// What the finished summary records besides its text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// The first message kept verbatim, or empty when the summary replaces everything.
    pub first_kept_message_id: String,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
    /// The context the conversation used before the compaction.
    pub tokens_before: u64,
    /// The estimated tokens that the summary replaces, the previous summary included.
    pub summarized_tokens: u64,
}

/// The plan for a summary request, or `None` when the request asks for something else.
pub(crate) fn plan(request: &api::Request) -> Option<Plan> {
    let Some(api::request::input::Type::SummarizeConversation(summarize)) = request
        .input
        .as_ref()
        .and_then(|input| input.r#type.as_ref())
    else {
        return None;
    };

    let history = convert::history_from_request(request);
    let items = history
        .turns
        .iter()
        .map(transcript_items)
        .collect::<Vec<_>>();
    let entries = history
        .turns
        .iter()
        .zip(&items)
        .map(|(turn, items)| Entry {
            kind: match turn {
                Turn::User(_) => EntryKind::User,
                Turn::Assistant { .. } => EntryKind::Assistant,
                Turn::ToolResults(_) => EntryKind::ToolResults,
            },
            tokens: items.iter().map(estimate_item_tokens).sum(),
        })
        .collect::<Vec<_>>();

    // An older client asks for no tail, and gets the whole conversation summarized.
    let cut = (summarize.keep_recent_tokens > 0)
        .then(|| find_cut_point(&entries, u64::from(summarize.keep_recent_tokens)))
        .flatten();
    let (history_end, prefix, first_kept_message_id) = match cut {
        Some(cut) => {
            // A split turn with nothing before it has no history apart from its prefix, so the
            // prefix is summarized as the history (pi puts "No prior history." in that place).
            let turn_start = cut.turn_start.filter(|start| *start > 0);
            (
                turn_start.unwrap_or(cut.first_kept),
                turn_start.map(|start| start..cut.first_kept),
                history.openers[cut.first_kept].clone(),
            )
        }
        None => (entries.len(), None, String::new()),
    };
    let summarized_end = prefix.as_ref().map_or(history_end, |prefix| prefix.end);

    let flatten =
        |range: std::ops::Range<usize>| items[range].iter().flatten().cloned().collect::<Vec<_>>();
    let history_text = serialize(&flatten(0..history_end));
    let prefix_text = prefix.clone().map(|range| serialize(&flatten(range)));

    let previous_summary = history.summary.as_ref();
    let mut file_ops = FileOps::default();
    if let Some(previous) = previous_summary {
        previous
            .read_files
            .iter()
            .for_each(|path| file_ops.read(path.as_str()));
        previous
            .modified_files
            .iter()
            .for_each(|path| file_ops.modified(path.as_str()));
    }
    for turn in &history.turns[..summarized_end] {
        if let Turn::Assistant { tool_calls, .. } = turn {
            for call in tool_calls {
                record_file_ops(&call.name, &call.arguments, &mut file_ops);
            }
        }
    }
    let (read_files, modified_files) = file_ops.lists();

    let previous_summary_tokens = previous_summary.map_or(0, |previous| {
        estimate_tokens(&inputs::summary_turn(&previous.text))
    });
    let all_tokens =
        previous_summary_tokens + entries.iter().map(|entry| entry.tokens).sum::<u64>();
    let summarized_tokens = previous_summary_tokens
        + entries[..summarized_end]
            .iter()
            .map(|entry| entry.tokens)
            .sum::<u64>();
    let tokens_before = if summarize.context_tokens > 0 {
        summarize.context_tokens
    } else {
        all_tokens
    };

    Some(Plan {
        max_output_tokens: (summarize.max_output_tokens > 0).then_some(summarize.max_output_tokens),
        prompt_text: summary_request_text(
            &history_text,
            prefix_text.as_deref(),
            previous_summary.map(|previous| strip_file_lists(&previous.text)),
            Some(summarize.prompt.as_str()),
        ),
        outcome: Outcome {
            first_kept_message_id,
            read_files,
            modified_files,
            tokens_before,
            summarized_tokens,
        },
    })
}

fn transcript_items(turn: &Turn) -> Vec<TranscriptItem<'_>> {
    match turn {
        Turn::User(text) => vec![TranscriptItem::User(text)],
        Turn::Assistant {
            text,
            reasoning,
            tool_calls,
        } => vec![TranscriptItem::Assistant {
            text,
            reasoning,
            tool_calls: tool_calls
                .iter()
                .map(|call| format_tool_call(&call.name, &call.arguments))
                .collect(),
        }],
        Turn::ToolResults(results) => results
            .iter()
            .map(|result| TranscriptItem::ToolResult(&result.content))
            .collect(),
    }
}

/// A call as `name(key=value, …)`, each value JSON-encoded.
fn format_tool_call(name: &str, arguments: &Value) -> String {
    match arguments {
        Value::Object(fields) => {
            let fields = fields
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}({fields})")
        }
        other => format!("{name}({other})"),
    }
}

/// Records the files a built-in tool call reads or changes.
fn record_file_ops(name: &str, arguments: &Value, file_ops: &mut FileOps) {
    let paths = |list: &str, key: &str| {
        arguments[list]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry[key].as_str())
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    match name {
        "read_files" => paths("files", "path")
            .into_iter()
            .for_each(|path| file_ops.read(path)),
        "apply_file_diffs" => ["diffs", "new_files", "deleted_files"]
            .into_iter()
            .flat_map(|list| paths(list, "file_path"))
            .for_each(|path| file_ops.modified(path)),
        _ => {}
    }
}

#[cfg(test)]
#[path = "compaction_tests.rs"]
mod tests;
