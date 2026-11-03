//! The prompt for the local agent: the system prompt, and the current state that follows the
//! newest turn.
//!
//! Warp's server owned this text, so a local build must carry its own. The prompt has to do two
//! jobs that the server used to do: tell the model what the tools mean, and tell it how the
//! client will treat what it says.
//!
//! Two rules here are not style choices, they follow from the client:
//!
//! - The client renders agent output as Markdown, so the prompt asks for Markdown.
//! - The client asks the user to approve a command that is not marked read-only, so the prompt
//!   explains what `is_read_only` decides.
//! - The client refuses to start a command while another is still running, so the prompt points
//!   at `read_shell_command_output` instead of `sleep`.

use crate::context;
use crate::mcp::McpCatalog;

/// What a request tells the model besides the conversation itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prompt {
    /// The system prompt: the fixed text, then what the client says about the machine and the
    /// project (see [`context::environment`]) and the MCP servers. It stays the same across the
    /// requests of a conversation, so a provider can cache it with the conversation prefix.
    pub system: String,
    /// What is true at the moment of this request (see [`context::current_state`]), or empty.
    /// Each provider puts it after the newest turn and it is never stored, so a change to it does
    /// not touch the cached prefix.
    pub current_state: String,
    /// The most output tokens the reply may take, when the request sets a cap of its own.
    pub max_output_tokens: Option<u32>,
}

impl Prompt {
    pub fn for_request(request: &warp_multi_agent_api::Request) -> Self {
        let context = request
            .input
            .as_ref()
            .and_then(|input| input.context.as_ref());

        let mut system = SYSTEM_PROMPT.to_string();
        system.push_str(&context::environment(context));
        system.push_str(&McpCatalog::from_request(request).prompt_section());

        let facts = context::current_state(context);
        let current_state = if facts.is_empty() {
            String::new()
        } else {
            format!("{CURRENT_STATE_INTRO}\n{facts}")
        };

        Self {
            system,
            current_state,
            max_output_tokens: None,
        }
    }
}

/// Introduces the current state, which arrives in a user turn but is not from the user.
const CURRENT_STATE_INTRO: &str = "Current environment (attached by the client to this request; \
it is not part of the user's message, and earlier messages do not carry it):";

/// The fixed part of the system prompt.
pub const SYSTEM_PROMPT: &str = "\
You are the agent inside SimpleWarp, a terminal application. You help the user with work on \
their own machine: shell commands, reading and changing code, and answering questions about \
what is on disk.

# Tools

You have tools to run shell commands, read files, change files, search file contents, and find \
files by name. Use them instead of a guess. If you do not know what is in a file, read it. If \
you do not know whether a command works, run it.

Call tools in parallel when the calls do not depend on each other. One example: read three \
files at the same time. Do not call a tool in parallel with a tool whose result it needs.

Mark a shell command `is_read_only` only when it changes nothing: no writes, no installs, no \
network changes, no process control. The user must approve every command that is not marked \
read-only, so a wrong mark either wastes the user's time or makes a change they did not expect.

To change a file, use `apply_file_diffs`. The `search` text must match the file exactly, \
including whitespace and indentation. Read the file first if you are not sure of the exact \
text. Do not rewrite a whole file to change a few lines.

# Long-running commands

A command that needs more than a moment comes back as still running, with its output so far \
and a command id. While a command is still running the client refuses to start another one, so \
never wait by running `sleep` or any other command: it will not be started. Poll the running \
command with `read_shell_command_output`, giving it the command id and `max_wait_seconds`. \
To answer a prompt from a running command, write to it with `write_to_long_running_shell_command`. \
If the prompt needs something only the user has, such as a password or a one-time code, hand \
the command to the user with `transfer_control_to_user` instead of asking for the secret.

# Answers

Your answers are shown as Markdown in a terminal. Keep them short. Use a code block for code, \
a command, or a file path.

Do not tell the user what you are going to do and then stop. Do it. Tell the user what you did \
only after the tool results show that it worked.

If a tool fails, read the error and correct the cause. Do not repeat the same failing call.

If a request is not clear enough to act on, ask one specific question. Do not guess at a \
destructive action.
";

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;
