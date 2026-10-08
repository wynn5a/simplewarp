//! Renders the context the client attaches to a request into text for the model.
//!
//! Warp's server read `Request::input::context` and the user query's referenced attachments and
//! built the model prompt from them. A local build has to do that itself, or the model never sees
//! the working directory, the project rules, or anything the user attached.
//!
//! Two kinds of text come out of here, and they go to different places:
//!
//! - [`environment`] describes where the agent is running: the working directory, OS, shell, git
//!   state, project rules and available skills. The client sends it with every request, including
//!   the ones that only return tool results, and it is not stored in the conversation history, so
//!   it goes in the system prompt, where a replayed history gets it again.
//! - [`attachments`] is what the user attached to this one message: terminal blocks, files,
//!   selected text and diffs. It follows the user's text in the same turn.

use std::collections::HashMap;
use std::fmt::Write as _;

use chrono::{DateTime, Utc};
use warp_multi_agent_api as api;
use warp_multi_agent_api::attachment::Value as AttachmentValue;

/// The most characters of one attachment or rule file that go to the model.
const MAX_ITEM_CHARS: usize = 60_000;

/// The most characters of attachments that go to the model for one message.
const MAX_TOTAL_CHARS: usize = 200_000;

/// Describes the machine and the project, for the system prompt. Empty when the request carries
/// no context.
pub fn environment(context: Option<&api::InputContext>) -> String {
    let Some(context) = context else {
        return String::new();
    };
    let mut out = String::new();

    let mut facts = Vec::new();
    if let Some(directory) = &context.directory {
        if !directory.pwd.is_empty() {
            facts.push(format!("Working directory: {}", directory.pwd));
        }
        if !directory.home.is_empty() {
            facts.push(format!("Home directory: {}", directory.home));
        }
    }
    if let Some(os) = &context.operating_system {
        let mut line = os.platform.clone();
        if !os.distribution.is_empty() {
            let _ = write!(line, " ({})", os.distribution);
        }
        if !line.is_empty() {
            facts.push(format!("Operating system: {line}"));
        }
    }
    if let Some(shell) = &context.shell
        && !shell.name.is_empty()
    {
        let mut line = shell.name.clone();
        if !shell.version.is_empty() {
            let _ = write!(line, " {}", shell.version);
        }
        facts.push(format!("Shell: {line}"));
    }
    if let Some(time) = &context.current_time
        && let Some(time) = DateTime::<Utc>::from_timestamp(time.seconds, 0)
    {
        facts.push(format!(
            "Current time: {}",
            time.format("%Y-%m-%d %H:%M UTC")
        ));
    }
    if let Some(git) = &context.git {
        if let Some(repository) = &git.repository
            && !repository.name.is_empty()
        {
            let mut line = String::new();
            if !repository.owner.is_empty() {
                let _ = write!(line, "{}/", repository.owner);
            }
            line.push_str(&repository.name);
            if !repository.host.is_empty() {
                let _ = write!(line, " on {}", repository.host);
            }
            facts.push(format!("Git repository: {line}"));
        }
        if !git.branch.is_empty() {
            facts.push(format!("Git branch: {}", git.branch));
        } else if !git.head.is_empty() {
            facts.push(format!("Git HEAD (detached): {}", git.head));
        }
        if let Some(pull_request) = &git.pull_request
            && pull_request.number > 0
        {
            let mut line = format!("#{}", pull_request.number);
            if !pull_request.base_branch.is_empty() {
                let _ = write!(line, " into {}", pull_request.base_branch);
            }
            if !pull_request.url.is_empty() {
                let _ = write!(line, " ({})", pull_request.url);
            }
            facts.push(format!("Pull request for this branch: {line}"));
        }
    }
    for codebase in &context.codebases {
        if !codebase.path.is_empty() {
            facts.push(format!("Codebase: {} at {}", codebase.name, codebase.path));
        }
    }
    if !facts.is_empty() {
        out.push_str("\n# Environment\n\n");
        for fact in facts {
            let _ = writeln!(out, "- {fact}");
        }
    }

    for rules in &context.project_rules {
        let has_active = rules
            .active_rule_files
            .iter()
            .any(|file| !file.content.is_empty());
        if !has_active && rules.additional_rule_file_paths.is_empty() {
            continue;
        }
        if rules.root_path.is_empty() {
            // Rules with no project are the user's own, for all their work.
            out.push_str("\n# User rules\n\n");
            out.push_str("The user set these rules for all their work. Follow them.\n");
        } else {
            out.push_str("\n# Project rules\n\n");
            let _ = writeln!(
                out,
                "The user wrote these rules for the project at `{}`. Follow them.",
                rules.root_path
            );
        }
        for file in &rules.active_rule_files {
            if file.content.is_empty() {
                continue;
            }
            let _ = write!(out, "\n## {}\n\n", file.file_path);
            out.push_str(&fenced(&file.content, MAX_ITEM_CHARS));
        }
        if !rules.additional_rule_file_paths.is_empty() {
            out.push_str(
                "\nMore rule files exist. Read one with `read_files` when its location is \
                 relevant to the task:\n",
            );
            for path in &rules.additional_rule_file_paths {
                let _ = writeln!(out, "- {path}");
            }
        }
    }

    if let Some(skills) = &context.updated_skills_context
        && !skills.available_skills.is_empty()
    {
        out.push_str("\n# Skills\n\nThe user has these skills installed:\n");
        for skill in &skills.available_skills {
            let location = match &skill.skill_reference {
                Some(api::skill_descriptor::SkillReference::Path(path)) => path.as_str(),
                _ => "",
            };
            let _ = write!(out, "- {}: {}", skill.name, skill.description);
            if !location.is_empty() {
                let _ = write!(out, " ({location})");
            }
            out.push('\n');
        }
    }

    out
}

/// Renders what the user attached to one message, or an empty string when nothing is attached.
///
/// `referenced` holds the attachments that the query text names by key. `context` holds the
/// attachments that the client puts on the whole request.
pub fn attachments(
    context: Option<&api::InputContext>,
    referenced: &HashMap<String, api::Attachment>,
) -> String {
    let mut sections = Vec::new();

    let mut keys: Vec<_> = referenced.keys().collect();
    keys.sort();
    for key in keys {
        if let Some(text) = render_attachment(&referenced[key]) {
            sections.push(format!("Attachment `{key}`:\n{text}"));
        }
    }

    if let Some(context) = context {
        #[allow(deprecated)]
        for command in &context.executed_shell_commands {
            sections.push(render_command(command));
        }
        for selected in &context.selected_text {
            if !selected.text.is_empty() {
                sections.push(format!(
                    "Text the user selected:\n{}",
                    fenced(&selected.text, MAX_ITEM_CHARS)
                ));
            }
        }
        for file in &context.files {
            if let Some(content) = &file.content {
                sections.push(render_file(content));
            }
        }
        if !context.images.is_empty() {
            // The providers here are called with text only, so say so rather than let the model
            // answer as though it had seen the image.
            sections.push(format!(
                "The user attached {} image(s). This agent cannot see images, so say so if the \
                 answer depends on them.",
                context.images.len()
            ));
        }
    }

    let mut out = String::new();
    let mut used = 0;
    for section in sections {
        used += section.len();
        if used > MAX_TOTAL_CHARS {
            out.push_str("\n\n(More attachments were left out because they are too large.)");
            break;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&section);
    }
    out
}

fn render_attachment(attachment: &api::Attachment) -> Option<String> {
    Some(match attachment.value.as_ref()? {
        AttachmentValue::PlainText(text) => fenced(text, MAX_ITEM_CHARS),
        AttachmentValue::ExecutedShellCommand(command) => render_command(command),
        AttachmentValue::RunningShellCommand(running) => {
            let mut out = format!("A command that is still running: `{}`", running.command);
            if let Some(snapshot) = &running.snapshot {
                let _ = write!(
                    out,
                    " (command id `{}`). Its output so far:\n{}",
                    snapshot.command_id,
                    fenced(&snapshot.output, MAX_ITEM_CHARS)
                );
            }
            out
        }
        AttachmentValue::DiffSet(diff_set) => render_diff_set(diff_set),
        #[allow(deprecated)]
        AttachmentValue::DiffHunk(hunk) => format!(
            "Diff of `{}`:\n{}",
            hunk.file_path,
            fenced(&hunk.diff_content, MAX_ITEM_CHARS)
        ),
        AttachmentValue::DocumentContent(document) => fenced(&document.content, MAX_ITEM_CHARS),
        AttachmentValue::FilePathReference(reference) => {
            format!("A file on disk: `{}`", reference.file_path)
        }
        // Warp Drive objects came from a service that this build does not have.
        AttachmentValue::DriveObject(_) => return None,
    })
}

fn render_command(command: &api::ExecutedShellCommand) -> String {
    format!(
        "Terminal command `{}` (exit code {}):\n{}",
        command.command,
        command.exit_code,
        fenced(&command.output, MAX_ITEM_CHARS)
    )
}

fn render_file(file: &api::FileContent) -> String {
    let range = file
        .line_range
        .as_ref()
        .filter(|range| range.end > 0)
        .map(|range| format!(", lines {}-{}", range.start, range.end))
        .unwrap_or_default();
    format!(
        "File `{}`{range}:\n{}",
        file.file_path,
        fenced(&file.content, MAX_ITEM_CHARS)
    )
}

fn render_diff_set(diff_set: &api::DiffSet) -> String {
    let mut out = String::from("Changes in the working tree:");
    for hunk in &diff_set.hunks {
        let _ = write!(
            out,
            "\n\n`{}` (+{} -{}):\n{}",
            hunk.file_path,
            hunk.lines_added,
            hunk.lines_removed,
            fenced(&hunk.diff_content, MAX_ITEM_CHARS)
        );
    }
    out
}

/// Wraps text in a code fence that the text itself cannot close, cutting it to `limit`
/// characters first.
fn fenced(text: &str, limit: usize) -> String {
    let (text, cut) = match text.char_indices().nth(limit) {
        Some((index, _)) => (&text[..index], true),
        None => (text, false),
    };
    let longest_run = text
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let fence = "`".repeat((longest_run + 1).max(3));
    let mut out = format!("{fence}\n{text}");
    if !text.ends_with('\n') {
        out.push('\n');
    }
    if cut {
        out.push_str("(cut: the rest is too long to include)\n");
    }
    out.push_str(&fence);
    out
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
