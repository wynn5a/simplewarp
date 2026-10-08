//! The text that stands in for the request inputs that are not a plain user question.
//!
//! Warp's server expanded these into prompts of its own: `/compact` into a summarizing prompt,
//! `/init` into "write the project rules", a skill into its instructions, and so on. A local
//! build has to carry that text itself, or the model sees no new instruction at all.
//!
//! Each input is expanded in two places that must agree: when it arrives in `Request::input`, and
//! when it comes back as a stored message in the task history (see [`crate::emit`], which stores
//! it). Both call the functions here.

use std::fmt::Write as _;

use warp_multi_agent_api as api;

/// What `/init` asks for. The client sends an input with no fields, and the server supplied
/// this text.
pub const INIT_PROJECT_RULES: &str = "\
Create an AGENTS.md file for this project, at the root of the repository that holds the \
working directory.

AGENTS.md tells a coding agent how to work in the project. Explore before you write: read the \
README and the build files, list the top-level directories, and look at how the tests are run. \
Then write a short file that covers:

- what the project is, in one or two sentences
- how to build it, run it, and run its tests, with the exact commands
- how the code is laid out, and where to look for what
- the conventions to follow: style, naming, error handling, and anything the repository does \
differently from the usual

Write only what you checked in the project. Do not invent commands. If AGENTS.md already \
exists, read it and improve it instead of replacing it. When the file is written, say what you \
put in it.";

/// What `/continue` asks for.
pub const RESUME_CONVERSATION: &str =
    "Continue from where you left off. If the last step failed, correct the cause and carry on.";

/// What the summary is for, and how it should read, when the user runs `/compact`.
///
/// The summary replaces the conversation, so it has to hold what the model needs to carry on
/// without the rest. See [`summary_turn`].
const SUMMARIZE_CONVERSATION: &str = "\
Summarize this conversation so that the work can continue from your summary alone. Everything \
before it will be dropped.

Cover, in this order:

- what the user asked for, and any constraints or preferences they gave
- what has been done: files read or changed, commands run, and what they showed
- what was decided, and why
- the current state: what works, what is broken, and any error text that matters
- what is left to do, as the next steps

Keep the specific details a later step would need: file paths, command lines, names, versions, \
and exact error messages. Leave out the pleasantries and anything that was tried and dropped. \
Do not call any tool. Write plain Markdown.";

/// The model-facing text for `/compact`, with the user's own instructions when they gave any.
pub fn summarize_conversation(prompt: &str) -> String {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return SUMMARIZE_CONVERSATION.to_string();
    }
    format!("{SUMMARIZE_CONVERSATION}\n\nThe user asked for this focus:\n{prompt}")
}

/// The turn that takes the place of a conversation that was summarized.
pub fn summary_turn(summary: &str) -> String {
    format!(
        "This conversation was compacted. What came before it is replaced by this summary, \
         which you wrote:\n\n{summary}\n\nContinue the work from here."
    )
}

pub fn create_new_project(query: &str) -> String {
    format!(
        "I want to start a new project: {query}\n\nAsk me one question if something essential is \
         missing. Otherwise set it up in a new directory under the working directory, using the \
         shell and file tools, and tell me how to run it."
    )
}

pub fn clone_repository(url: &str) -> String {
    format!(
        "Clone {url} into the working directory with git. Then say what the repository \
         contains, and how to build and test it."
    )
}

/// A skill's instructions, then whatever the user asked alongside it.
pub fn invoke_skill(skill: Option<&api::Skill>, query: &str) -> String {
    let mut out = String::new();
    if let Some(skill) = skill {
        let name = skill
            .descriptor
            .as_ref()
            .map(|descriptor| descriptor.name.as_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("skill");
        let content = skill
            .content
            .as_ref()
            .map(|content| content.content.as_str())
            .unwrap_or_default();
        let _ = write!(
            out,
            "The user invoked the skill `{name}`. Follow its instructions:\n\n{content}"
        );
    }
    let query = query.trim();
    if !query.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(query);
    }
    out
}

/// The review comments the user sent back to the agent, with the code each is about.
pub fn code_review(comments: &[api::ReviewComment], diff_set: Option<&api::DiffSet>) -> String {
    let mut out = String::from(
        "I reviewed your changes and left comments. Address each one, then say what you changed.\n",
    );
    for (index, comment) in comments.iter().enumerate() {
        let _ = write!(out, "\n{}. ", index + 1);
        match comment.comment_target.as_ref() {
            Some(api::review_comment::CommentTarget::CommentedLine(hunk)) => {
                let _ = write!(out, "In `{}`", hunk.file_path);
                if let Some(range) = hunk.line_range.as_ref()
                    && range.end > 0
                {
                    let _ = write!(out, ", lines {}-{}", range.start, range.end);
                }
                out.push_str(":\n");
                if !hunk.diff_content.is_empty() {
                    let _ = writeln!(out, "```diff\n{}\n```", hunk.diff_content.trim_end());
                }
            }
            Some(api::review_comment::CommentTarget::CommentedFile(file)) => {
                let _ = writeln!(out, "On the file `{}`:", file.file_path);
            }
            Some(api::review_comment::CommentTarget::CommentedDiffset(_)) | None => {
                out.push_str("On the changes as a whole:\n");
            }
        }
        let _ = writeln!(out, "{}", comment.comment.trim());
    }
    if let Some(diff_set) = diff_set
        && !diff_set.hunks.is_empty()
    {
        out.push_str("\nThe changes under review:\n");
        for hunk in &diff_set.hunks {
            let _ = write!(
                out,
                "\n`{}`:\n```diff\n{}\n```\n",
                hunk.file_path,
                hunk.diff_content.trim_end()
            );
        }
    }
    out
}

#[cfg(test)]
#[path = "inputs_tests.rs"]
mod tests;
