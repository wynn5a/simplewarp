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

use warp_multi_agent_api as api;

use crate::mcp::McpCatalog;
use crate::{context, tools};

/// The most characters of `APPEND_SYSTEM.md` that go to the model.
const MAX_APPENDED_SYSTEM_CHARS: usize = 60_000;

/// What a request tells the model besides the conversation itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prompt {
    /// The system prompt: the fixed text (with [`MEMORY_SECTION`] when `remember` is offered),
    /// then what the client says about the machine and the project (see
    /// [`context::environment`]), the MCP servers, and last the user's own `APPEND_SYSTEM.md`.
    /// It stays the same across the requests of a conversation, so a provider can cache it with
    /// the conversation prefix.
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
        let supported_tools = request
            .settings
            .as_ref()
            .map(|settings| settings.supported_tools.as_slice())
            .unwrap_or_default();
        if tools::offers(api::ToolType::Remember, supported_tools) {
            system.push_str(MEMORY_SECTION);
        }
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

    /// Appends the user's own text to the very end of the system prompt, after everything the
    /// client contributes, so that it can neither displace nor reorder the cached prefix. Blank
    /// text changes nothing.
    pub fn with_appended_system(mut self, text: Option<&str>) -> Self {
        let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
            return self;
        };
        self.system.push_str("\n\n");
        match text.char_indices().nth(MAX_APPENDED_SYSTEM_CHARS) {
            Some((index, _)) => {
                self.system.push_str(&text[..index]);
                self.system
                    .push_str("\n(cut: the rest of APPEND_SYSTEM.md is too long to include)");
            }
            None => self.system.push_str(text),
        }
        self.system.push('\n');
        self
    }
}

/// The largest `APPEND_SYSTEM.md` that is read at all. Anything bigger is not a prompt addition,
/// and reading it for every request would cost more than it could add.
const MAX_APPENDED_SYSTEM_FILE_BYTES: u64 = 256 * 1024;

/// Reads `~/.agents/APPEND_SYSTEM.md`. A file that is missing, too large or cannot be read counts
/// as none.
pub fn read_appended_system() -> Option<String> {
    let path = dirs::home_dir()?.join(".agents").join("APPEND_SYSTEM.md");
    let read = std::fs::metadata(&path).and_then(|metadata| {
        if metadata.len() > MAX_APPENDED_SYSTEM_FILE_BYTES {
            let len = metadata.len();
            log::debug!("Skipping APPEND_SYSTEM.md: {len} bytes is over the size limit");
            return Ok(None);
        }
        std::fs::read_to_string(&path).map(Some)
    });
    match read {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            log::debug!("Failed to read APPEND_SYSTEM.md: {}", error.kind());
            None
        }
    }
}

/// How the agent uses the `remember` tool. It is part of the system prompt only when the tool is
/// offered.
const MEMORY_SECTION: &str = "
# Memory

`remember` saves one fact for future conversations. Saved facts reach every later conversation \
as rules (look for \"Agent memory\" under \"User rules\" and \"Project rules\"), and the user \
approves each one unless they chose otherwise.

Remember stable things that will matter again: a preference the user stated, a project \
convention (how to build and test, code style, where things live), or a pitfall that cost time \
and how to avoid it. Use scope `project` for what holds only in the current repository and \
`global` for the user's general preferences.

Do not remember the state or progress of the current task, anything you can read from the code \
when you need it, guesses the user has not confirmed, or secrets: passwords, tokens, keys or \
private data. Save a fact when the user asks you to remember it, or ask the user first when you \
find one worth keeping.

Keep each memory to one fact in a sentence or two. Check the rules you already have first and do \
not save what is already there. If the user declines a memory, do not ask to save it again.
";

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
