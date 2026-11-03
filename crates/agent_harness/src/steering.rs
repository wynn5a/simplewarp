//! Steering: a prompt submitted while the agent works is held in the conversation's queue and sent
//! with the next tool results, so it reaches the model at the next step without interrupting the
//! tools that are running.
//!
//! Ported from pi's `steer`. A steer that no step picks up stays a queued prompt, so it is sent
//! when the task ends, or goes back to the input when the user stops the task.

/// A row already in a conversation's queue, as far as steering is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuedRow {
    /// A steer. `editing` is true while the user is editing it.
    Steer { editing: bool },
    /// A row that must stay at the head of the queue until it unlocks.
    Locked,
    /// Any other queued prompt or command.
    Other,
}

/// Where a newly submitted steer goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Appended to the text of the steer at this index.
    MergeInto(usize),
    /// Inserted as a new row at this index.
    InsertAt(usize),
}

/// Where a new steer goes in `rows`.
///
/// Steers belong to the task that is running, so they go ahead of prompts queued for after it,
/// behind the steers and locked rows already at the head. A steer the user is not editing at the
/// end of that run takes the new text, so there is usually one steer row holding everything the
/// next step will carry.
pub fn placement(rows: &[QueuedRow]) -> Placement {
    let head = rows
        .iter()
        .take_while(|row| match row {
            QueuedRow::Steer { .. } | QueuedRow::Locked => true,
            QueuedRow::Other => false,
        })
        .count();
    match head.checked_sub(1).map(|last| (last, rows[last])) {
        Some((last, QueuedRow::Steer { editing: false })) => Placement::MergeInto(last),
        Some((_, QueuedRow::Steer { editing: true } | QueuedRow::Locked | QueuedRow::Other))
        | None => Placement::InsertAt(head),
    }
}

/// `earlier` followed by `later`, each trimmed, separated by a blank line. A blank part is dropped.
pub fn merge(earlier: &str, later: &str) -> String {
    match (earlier.trim(), later.trim()) {
        ("", only) | (only, "") => only.to_string(),
        (earlier, later) => format!("{earlier}\n\n{later}"),
    }
}

/// The rows a step sends with its tool results, in queue order: every steer the user is not
/// editing. A step whose results belong to a subagent's task carries none, since a steer is
/// addressed to the agent the user talks to.
pub fn steers_to_inject(rows: &[QueuedRow], results_on_root_task_only: bool) -> Vec<usize> {
    if !results_on_root_task_only {
        return Vec::new();
    }
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| match row {
            QueuedRow::Steer { editing: false } => Some(index),
            QueuedRow::Steer { editing: true } | QueuedRow::Locked | QueuedRow::Other => None,
        })
        .collect()
}

#[cfg(test)]
#[path = "steering_tests.rs"]
mod tests;
